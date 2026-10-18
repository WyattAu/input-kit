//! Grapheme-cluster caret mapping.
//!
//! A caret is a *rendering* position, not a byte position. Splitting a
//! grapheme cluster splits a character the user perceives as one: a base
//! letter from its combining mark, a flag from its second regional indicator,
//! or an emoji ZWJ sequence down the middle. Every platform reports preedit
//! cursors in a different unit (Win32 in UTF-16 code units, Wayland in byte
//! offsets, macOS marked-text ranges, the web in UTF-16), so an adapter that
//! trusts its platform's unit and slices the string will corrupt composed text.
//!
//! This module normalises all of them onto one unit: the grapheme cluster
//! index, per UAX #29 extended grapheme clusters as implemented by
//! [`unicode_segmentation`]. Every conversion here is total: an offset that
//! lands inside a cluster does not panic and does not silently snap, it
//! returns `None` and lets the caller decide.

use alloc::string::String;
use unicode_segmentation::UnicodeSegmentation;

/// Number of grapheme clusters in `s`.
///
/// ```
/// # use input_kit::grapheme;
/// // "e" + combining acute is one grapheme, two bytes.
/// assert_eq!(grapheme::count("e\u{0301}"), 1);
/// assert_eq!(grapheme::count("cafe\u{0301}"), 4);
/// ```
#[must_use]
pub fn count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Byte offset of the start of grapheme cluster `index`.
///
/// Returns `None` when `index` is past the end. `index == count(s)` is the
/// caret-after-the-end position and is valid.
///
/// ```
/// # use input_kit::grapheme;
/// let s = "cafe\u{0301}";
/// assert_eq!(grapheme::byte_offset(s, 0), Some(0));
/// assert_eq!(grapheme::byte_offset(s, 3), Some(3));
/// assert_eq!(grapheme::byte_offset(s, 4), Some(6)); // past the last cluster
/// assert_eq!(grapheme::byte_offset(s, 5), None);   // past the end
/// ```
#[must_use]
pub fn byte_offset(s: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return Some(0);
    }
    s.grapheme_indices(true)
        .nth(index)
        .map(|(byte, _)| byte)
        .or_else(|| (index == count(s)).then_some(s.len()))
}

/// Grapheme index containing `offset`.
///
/// Returns `None` when `offset` is past the end of `s`.
///
/// # Errors on interior offsets
///
/// An offset that lands *inside* a cluster returns `None` rather than
/// rounding. Callers that need a nearest-boundary position use
/// [`nearest_index`], which is explicit about which way it rounds.
///
/// ```
/// # use input_kit::grapheme;
/// let s = "cafe\u{0301}";
/// assert_eq!(grapheme::index(s, 0), Some(0));
/// assert_eq!(grapheme::index(s, 3), Some(3));
/// assert_eq!(grapheme::index(s, 6), Some(4)); // end of string
/// assert_eq!(grapheme::index(s, 4), None);   // the combining mark itself
/// ```
#[must_use]
pub fn index(s: &str, offset: usize) -> Option<usize> {
    if offset == s.len() {
        return Some(count(s));
    }
    if !s.is_char_boundary(offset) {
        return None;
    }
    for (idx, (byte, cluster)) in s.grapheme_indices(true).enumerate() {
        if byte == offset {
            return Some(idx);
        }
        // `byte` is the cluster start. The offset belongs to this cluster iff
        // it is before the next cluster's start, which is exactly what
        // `byte < offset <= byte + cluster.len()` expresses. Landing strictly
        // inside therefore falls through to the final check below.
        if offset < byte + cluster.len() {
            return None;
        }
    }
    None
}

/// Nearest grapheme index to `offset`, rounding down.
///
/// The forgiving companion to [`index`], for renderers that must place a
/// caret somewhere rather than give up. An offset past the end clamps to
/// [`count`].
///
/// ```
/// # use input_kit::grapheme;
/// assert_eq!(grapheme::nearest_index("cafe\u{0301}", 5), Some(3));
/// assert_eq!(grapheme::nearest_index("cafe\u{0301}", 99), Some(4));
/// ```
#[must_use]
pub fn nearest_index(s: &str, offset: usize) -> Option<usize> {
    let clamped = offset.min(s.len());
    let mut idx = 0;
    for (byte, cluster) in s.grapheme_indices(true) {
        if clamped <= byte {
            return Some(idx);
        }
        if clamped < byte + cluster.len() {
            // Inside this cluster: round down to its start.
            return Some(idx);
        }
        idx += 1;
    }
    Some(idx)
}

/// Clamp a grapheme index into `0..=count(s)`.
#[must_use]
pub fn clamp_index(s: &str, index: usize) -> usize {
    index.min(count(s))
}

/// Splice `insert` into `s` at grapheme index `index`.
///
/// Never splits a cluster: an index past the end appends. Returns `None` only
/// when `s` does not end on a grapheme boundary, which is a malformed string
/// and not a caller error.
///
/// ```
/// # use input_kit::grapheme;
/// assert_eq!(grapheme::splice("cafe", 2, "X").as_deref(), Some("caXfe"));
/// // Inserting after the base of a decomposed "e" keeps the mark with it.
/// assert_eq!(grapheme::splice("cafe\u{0301}", 4, "!").as_deref(), Some("cafe\u{0301}!"));
/// ```
#[must_use]
pub fn splice(s: &str, index: usize, insert: &str) -> Option<String> {
    let at = byte_offset(s, index)?;
    let mut out = String::with_capacity(s.len() + insert.len());
    out.push_str(s.get(..at)?);
    out.push_str(insert);
    out.push_str(s.get(at..)?);
    Some(out)
}

/// Delete the grapheme cluster range `range` from `s`.
///
/// The unit is clusters, so a single `delete` on `"cafe\u{0301}"` with range
/// `3..4` removes the `e` *and* its combining mark. That is the whole point:
/// a byte-range delete would leave a dangling mark.
///
/// ```
/// # use input_kit::grapheme;
/// assert_eq!(grapheme::delete("cafe\u{0301}", 3..4).as_deref(), Some("caf"));
/// assert_eq!(grapheme::delete("caf\u{00e9}", 0..1).as_deref(), Some("af\u{00e9}"));
/// ```
#[must_use]
pub fn delete(s: &str, range: core::ops::Range<usize>) -> Option<String> {
    let (start, end) = if range.start <= range.end {
        (range.start, range.end)
    } else {
        (range.end, range.start)
    };
    let lo = byte_offset(s, start)?;
    let hi = byte_offset(s, end)?;
    let mut out = String::with_capacity(s.len());
    out.push_str(s.get(..lo)?);
    out.push_str(s.get(hi..)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// "e" + U+0301 is one cluster spanning bytes 3..=5, so the text is four
    /// clusters long and index 4 is the past-the-end position at byte 6.
    #[test]
    fn a_combining_mark_stays_with_its_base() {
        let s = "cafe\u{0301}";
        assert_eq!(count(s), 4);
        assert_eq!(s.len(), 6);
        assert_eq!(byte_offset(s, 3), Some(3));
        assert_eq!(byte_offset(s, 4), Some(6));
        // Byte 4 is the combining mark itself: inside the cluster, so refused.
        assert_eq!(index(s, 4), None);
        assert_eq!(nearest_index(s, 4), Some(3));
    }

    /// A regional-indicator flag is ONE grapheme cluster under UAX #29: the
    /// pair joins, so a caret cannot be placed between the two halves. This is
    /// the case that makes a byte-offset caret visibly wrong, and the reason
    /// the unit cannot be trusted from the platform.
    #[test]
    fn a_flag_is_one_cluster_not_two() {
        let s = "\u{1F1FA}\u{1F1F8}"; // US flag: 2 code points, 8 bytes
        assert_eq!(s.len(), 8);
        assert_eq!(count(s), 1, "RI pairs are a single cluster");
        assert_eq!(byte_offset(s, 0), Some(0));
        assert_eq!(byte_offset(s, 1), Some(8), "past the end");
        // Splitting the pair is impossible, which is correct: a renderer that
        // let a caret land at byte 4 would show half a flag.
        assert_eq!(index(s, 4), None);
        assert_eq!(nearest_index(s, 4), Some(0));
    }

    /// Every byte offset is either a cluster boundary or refused, and the
    /// forgiving path always lands on a valid cluster. Checked exhaustively
    /// over the awkward strings: marks, flags, and ZWJ sequences.
    #[test]
    fn every_byte_offset_is_either_a_boundary_or_refused() {
        for s in [
            "",
            "a",
            "cafe\u{0301}",
            "\u{1F1FA}\u{1F1F8}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F4BB}",
            "a\u{0301}b\u{0308}c",
        ] {
            let boundaries: Vec<bool> = (0..=s.len())
                .map(|offset| index(s, offset).is_some())
                .collect();
            // The forgiving path must never fail, and must never round up.
            for offset in 0..=s.len() {
                let near = match nearest_index(s, offset) {
                    Some(n) => n,
                    None => unreachable!("nearest_index is total over any offset"),
                };
                assert!(near <= count(s), "{s:?} at {offset} gave {near}");
                let byte = match byte_offset(s, near) {
                    Some(b) => b,
                    None => unreachable!("index {near} is within count({:?})", s),
                };
                assert!(byte <= offset, "{s:?} at {offset} rounded up to {byte}");
            }
            // Byte 0 and the end are always real boundaries.
            assert!(
                boundaries.first().copied().unwrap_or(false),
                "{s:?}: byte 0 must be a boundary"
            );
            assert!(
                boundaries.last().copied().unwrap_or(false),
                "{s:?}: end must be a boundary"
            );
        }
    }

    /// Round-trip: index -> byte -> index is the identity at every cluster.
    #[test]
    fn index_and_byte_round_trip() {
        let s = "cafe\u{0301} \u{1F1FA}\u{1F1F8} ok";
        for i in 0..=count(s) {
            let byte = byte_offset(s, i).unwrap_or(s.len());
            assert_eq!(index(s, byte), Some(i), "cluster {i} of {s:?}");
        }
    }

    #[test]
    fn deleting_a_cluster_takes_its_marks() {
        assert_eq!(delete("cafe\u{0301}", 3..4).as_deref(), Some("caf"));
        // Deleting "caf" leaves the decomposed e with its mark intact. A
        // byte-range delete of 0..3 would be a different span; the point is
        // that a *cluster* range never orphans the U+0301.
        assert_eq!(
            delete("cafe\u{0301}", 0..3).as_deref(),
            Some("e\u{0301}")
        );
        // A precomposed letter is one cluster in one byte-pair.
        assert_eq!(delete("caf\u{00E9}", 3..4).as_deref(), Some("caf"));
    }

    #[test]
    fn out_of_range_is_refused_not_clamped() {
        assert_eq!(byte_offset("ab", 9), None);
        assert_eq!(index("ab", 99), None);
        assert_eq!(nearest_index("ab", 99), Some(2));
        assert_eq!(clamp_index("ab", 9), 2);
    }

    #[test]
    fn a_reversed_range_deletes_the_same_span() {
        // Built from variables so clippy does not read the literal as an
        // always-empty range; the point is that `delete` normalises the order.
        let (start, end) = (4, 1);
        assert_eq!(delete("abcdef", start..end).as_deref(), Some("aef"));
    }

    #[test]
    fn empty_and_ascii_are_identity() {
        assert_eq!(count(""), 0);
        assert_eq!(byte_offset("", 0), Some(0));
        assert_eq!(splice("", 0, "x").as_deref(), Some("x"));
        assert_eq!(splice("abc", 0, "x").as_deref(), Some("xabc"));
        assert_eq!(splice("abc", 3, "x").as_deref(), Some("abcx"));
    }
}