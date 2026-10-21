//! The preedit: the in-progress, not-yet-committed text an IME shows inline
//! while the user is still typing it.
//!
//! Platform preedit cursors arrive in three incompatible units:
//!
//! | Platform | Unit | Source |
//! | --- | --- | --- |
//! | Win32 IMM/TSF | UTF-16 code unit index | `CompForm.dwIndex` / `ITextStore` |
//! | macOS marked text | UTF-16 code unit range | `setMarkedText` attributed string |
//! | Web `InputEvent` | UTF-16 code unit index | `getTargetRanges()` |
//! | Wayland / X11 | byte offset | `preedit_string` + cursor byte index |
//!
//! All four are *code unit* or *byte* positions, and all four can land inside
//! a grapheme cluster. [`Preedit`] converts every one of them into a
//! grapheme-cluster index, which is the only unit a renderer can safely slice
//! a string at.

use crate::grapheme;
use alloc::string::String;

/// An in-progress composition, with the caret expressed in grapheme clusters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preedit {
    text: String,
    caret: usize,
    extent: usize,
}

impl Preedit {
    /// An empty preedit.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A preedit over `text` with the caret at the end and no selection.
    ///
    /// ```
    /// # use input_kit::Preedit;
    /// let p = Preedit::at_end("にほ");
    /// assert_eq!(p.text(), "にほ");
    /// assert_eq!(p.caret(), 2);
    /// ```
    #[must_use]
    pub fn at_end(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = grapheme::count(&text);
        Self {
            text,
            caret,
            extent: caret,
        }
    }

    /// A preedit with an explicit grapheme-cluster caret and selection.
    ///
    /// The caret and extent are clamped into range, so a stale cursor from the
    /// platform cannot produce an out-of-bounds value.
    #[must_use]
    pub fn new(text: impl Into<String>, caret: usize, extent: usize) -> Self {
        let text = text.into();
        let len = grapheme::count(&text);
        Self {
            text,
            caret: caret.min(len),
            extent: extent.min(len),
        }
    }

    /// A preedit whose caret comes from a **UTF-16 code unit** index, as
    /// Win32, macOS, and the web report it.
    ///
    /// An offset past the end of the string clamps forward. An offset that
    /// splits a surrogate pair or a grapheme cluster rounds down to the
    /// enclosing cluster, so the caret is always renderable.
    ///
    /// ```
    /// # use input_kit::Preedit;
    /// // "e" + combining acute is one grapheme; UTF-16 index 1 is inside it.
    /// let p = Preedit::from_utf16("cafe\u{0301}", 4);
    /// assert_eq!(p.caret(), 3);
    /// ```
    #[must_use]
    pub fn from_utf16(text: impl Into<String>, caret: u16) -> Self {
        let text = text.into();
        let unit = usize::from(caret);
        let caret = if unit <= utf16_len(&text) {
            grapheme::nearest_index(&text, utf16_byte_offset(&text, unit)).unwrap_or(0)
        } else {
            grapheme::count(&text)
        };
        Self::new(text, caret, caret)
    }

    /// A preedit whose caret comes from a **byte offset**, as Wayland and X11
    /// report it. An offset inside a cluster rounds down.
    ///
    /// ```
    /// # use input_kit::Preedit;
    /// assert_eq!(Preedit::from_bytes("cafe\u{0301}", 5).caret(), 3);
    /// ```
    #[must_use]
    pub fn from_bytes(text: impl Into<String>, caret: usize) -> Self {
        let text = text.into();
        let caret = grapheme::nearest_index(&text, caret).unwrap_or(0);
        Self::new(text, caret, caret)
    }

    /// The preedit text, to render with an underline or a background.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret, as a grapheme-cluster index into [`Preedit::text`].
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selection end, as a grapheme-cluster index.
    #[must_use]
    pub fn extent(&self) -> usize {
        self.extent
    }

    /// Whether the IME has an active selection within the preedit.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.caret != self.extent
    }

    /// The caret as a **byte offset** into [`Preedit::text`], for a renderer
    /// that slices.
    #[must_use]
    pub fn caret_byte(&self) -> usize {
        grapheme::byte_offset(&self.text, self.caret).unwrap_or(self.text.len())
    }

    /// The selection end as a byte offset.
    #[must_use]
    pub fn extent_byte(&self) -> usize {
        grapheme::byte_offset(&self.text, self.extent).unwrap_or(self.text.len())
    }

    /// Whether there is nothing to render.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Length in grapheme clusters.
    #[must_use]
    pub fn len(&self) -> usize {
        grapheme::count(&self.text)
    }

    /// The commit this preedit would produce if the IME ended right now.
    ///
    /// Some toolkits need the final text without waiting for a `Commit`
    /// event, for example on focus loss.
    #[must_use]
    pub fn to_commit(&self) -> String {
        self.text.clone()
    }
}

/// Byte offset of UTF-16 code unit `unit` in `text`, or `text.len()` if past.
///
/// When `unit` lands strictly inside a character's UTF-16 range (between the
/// high and low surrogate of an astral character) this returns that
/// character's own byte offset, not the next character's. Rounding *down* to
/// the enclosing character is the point: rounding up would place the caret
/// after a character the platform has not finished sending.
fn utf16_byte_offset(text: &str, unit: usize) -> usize {
    let mut seen = 0;
    for (byte, ch) in text.char_indices() {
        let width = ch.len_utf16();
        if unit < seen + width {
            return byte;
        }
        seen += width;
    }
    text.len()
}

/// Total UTF-16 code units in `text`.
fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_carets_land_on_graphemes() {
        // "にほん" is three BMP chars: UTF-16 index 2 == grapheme 2.
        let p = Preedit::from_utf16("にほん", 2);
        assert_eq!(p.caret(), 2);
        assert_eq!(p.caret_byte(), 6);
    }

    /// The astral-plane case: an emoji is one grapheme but two UTF-16 units.
    /// A web `InputEvent` reporting index 2 must not place the caret after
    /// half of it.
    #[test]
    fn an_astral_emoji_is_one_grapheme_two_utf16_units() {
        let s = "a\u{1F600}b";
        assert_eq!(utf16_len(s), 4);
        assert_eq!(grapheme::count(s), 3);
        for unit in 0..=4 {
            let p = Preedit::from_utf16(s, unit as u16);
            assert!(
                p.caret() <= 3,
                "utf16 {unit} produced grapheme {} of 3",
                p.caret()
            );
        }
        // Unit 1 is the high surrogate and unit 2 the low: both are inside the
        // emoji's cluster, so both round down to its start. Unit 3 is the
        // next character's boundary.
        assert_eq!(Preedit::from_utf16(s, 0).caret(), 0);
        assert_eq!(Preedit::from_utf16(s, 1).caret(), 1);
        assert_eq!(Preedit::from_utf16(s, 2).caret(), 1);
        assert_eq!(Preedit::from_utf16(s, 3).caret(), 2);
        assert_eq!(Preedit::from_utf16(s, 4).caret(), 3);
    }

    #[test]
    fn a_stale_cursor_cannot_produce_an_out_of_bounds_caret() {
        let p = Preedit::from_utf16("ab", 9999);
        assert_eq!(p.caret(), 2);
        let p = Preedit::new("ab", 50, 50);
        assert_eq!(p.caret(), 2);
        assert_eq!(p.extent(), 2);
    }

    #[test]
    fn byte_and_utf16_paths_agree_when_they_can() {
        for s in ["", "a", "cafe\u{0301}", "にほん", "a\u{1F600}b"] {
            for unit in 0..=utf16_len(s) {
                let via_utf16 = Preedit::from_utf16(s, unit as u16).caret();
                let byte = utf16_byte_offset(s, unit);
                let via_byte = Preedit::from_bytes(s, byte).caret();
                assert_eq!(
                    via_utf16, via_byte,
                    "{s:?} utf16 {unit}: byte {byte} disagreed"
                );
            }
        }
    }

    #[test]
    fn caret_byte_always_slices_cleanly() {
        for s in ["", "a", "cafe\u{0301}", "\u{1F1FA}\u{1F1F8}", "a\u{1F600}b"] {
            for i in 0..=grapheme::count(s) {
                let p = Preedit::new(s, i, i);
                assert!(s.is_char_boundary(p.caret_byte()), "{s:?} at {i}");
                assert!(s.is_char_boundary(p.extent_byte()), "{s:?} at {i}");
            }
        }
    }

    #[test]
    fn selection_is_reported() {
        let p = Preedit::new("にほん", 1, 3);
        assert!(p.has_selection());
        assert_eq!((p.caret(), p.extent()), (1, 3));
        assert!(!Preedit::at_end("ab").has_selection());
    }
}
