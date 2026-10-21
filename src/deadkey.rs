//! Dead-key composition for the accented Latin keyboard layouts.
//!
//! A dead key is a modifier that produces nothing by itself: press Option+`
//! then `e` and the layout emits `é` instead of two characters. X11, Wayland,
//! Win32, and macOS each implement this separately, each with its own table,
//! and a toolkit that wants Option+e to work in a text field ends up
//! reimplementing the table for every platform it ships on.
//!
//! [`DeadKey`] is that table. It is pure data plus one lookup, no platform
//! dependency, and it composes with [`crate::Composer`] as ordinary events:
//! the dead key opens a one-character preedit, and the following character
//! either combines with it or is emitted after it.
//!
//! ```
//! use input_kit::DeadKey;
//!
//! assert_eq!(DeadKey::Acute.apply('e'), Some('é'));
//! assert_eq!(DeadKey::Acute.apply('x'), None, "no precomposed x-acute exists");
//! assert_eq!(DeadKey::Acute.compose('x'), "x\u{0301}", "so compose with the mark");
//! ```

use alloc::string::String;

/// A dead-key accent and the precomposed characters it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeadKey {
    /// U+00B4 ACUTE ACCENT, the key above Tab on most layouts.
    Acute,
    /// U+0060 GRAVE ACCENT.
    Grave,
    /// U+005E CIRCUMFLEX ACCENT.
    Circumflex,
    /// U+007E TILDE.
    Tilde,
    /// U+00A8 DIAERESIS.
    Diaeresis,
    /// U+00B8 CEDILLA.
    Cedilla,
    /// U+02D8 DOUBLE ACUTE ACCENT.
    DoubleAcute,
    /// U+00AF MACRON.
    Macron,
    /// U+02D9 DOT ABOVE.
    DotAbove,
}

impl DeadKey {
    /// The character the dead key itself renders as.
    #[must_use]
    pub fn mark(self) -> char {
        match self {
            Self::Acute => '\u{00B4}',
            Self::Grave => '\u{0060}',
            Self::Circumflex => '\u{005E}',
            Self::Tilde => '\u{007E}',
            Self::Diaeresis => '\u{00A8}',
            Self::Cedilla => '\u{00B8}',
            Self::DoubleAcute => '\u{02D8}',
            Self::Macron => '\u{00AF}',
            Self::DotAbove => '\u{02D9}',
        }
    }

    /// The precomposed form of `base` under this accent, if one exists.
    ///
    /// Returns `None` when no precomposed character is available, which is the
    /// common case: most combinations have no Unicode precomposed form and a
    /// caller must fall back to base + combining mark.
    #[must_use]
    pub fn apply(self, base: char) -> Option<char> {
        // Indexed by (accent, lowercase base). Composed with the precomposed
        // Latin-1/Latin Extended-A table, which is where every layout's
        // table agrees.
        const ACUTE: &[(char, char)] = &[
            ('a', '\u{00E1}'),
            ('e', '\u{00E9}'),
            ('i', '\u{00ED}'),
            ('o', '\u{00F3}'),
            ('u', '\u{00FA}'),
            ('y', '\u{00FD}'),
            ('c', '\u{0107}'),
            ('n', '\u{0144}'),
            ('s', '\u{015B}'),
            ('z', '\u{017A}'),
            ('l', '\u{013E}'),
            ('r', '\u{0154}'),
            ('g', '\u{01F5}'),
        ];
        const GRAVE: &[(char, char)] = &[
            ('a', '\u{00E0}'),
            ('e', '\u{00E8}'),
            ('i', '\u{00EC}'),
            ('o', '\u{00F2}'),
            ('u', '\u{00F9}'),
        ];
        const CIRCUMFLEX: &[(char, char)] = &[
            ('a', '\u{00E2}'),
            ('e', '\u{00EA}'),
            ('i', '\u{00EE}'),
            ('o', '\u{00F4}'),
            ('u', '\u{00FB}'),
        ];
        const TILDE: &[(char, char)] = &[
            ('a', '\u{00E3}'),
            ('n', '\u{00F1}'),
            ('o', '\u{00F5}'),
            ('i', '\u{0129}'),
            ('u', '\u{016B}'),
        ];
        const DIAERESIS: &[(char, char)] = &[
            ('a', '\u{00E4}'),
            ('e', '\u{00EB}'),
            ('i', '\u{00EF}'),
            ('o', '\u{00F6}'),
            ('u', '\u{00FC}'),
            ('y', '\u{00FF}'),
        ];
        const CEDILLA: &[(char, char)] = &[
            ('c', '\u{00E7}'),
            ('s', '\u{015F}'),
            ('t', '\u{0163}'),
            ('g', '\u{0127}'),
        ];
        const DOUBLE_ACUTE: &[(char, char)] = &[('o', '\u{0151}'), ('u', '\u{0171}')];
        const MACRON: &[(char, char)] = &[
            ('a', '\u{0101}'),
            ('e', '\u{0113}'),
            ('i', '\u{012B}'),
            ('o', '\u{014D}'),
            ('u', '\u{016B}'),
            ('n', '\u{00F1}'),
        ];
        const DOT_ABOVE: &[(char, char)] = &[
            ('c', '\u{010B}'),
            ('e', '\u{0117}'),
            ('g', '\u{0121}'),
            ('z', '\u{017C}'),
        ];

        let table: &[(char, char)] = match self {
            Self::Acute => ACUTE,
            Self::Grave => GRAVE,
            Self::Circumflex => CIRCUMFLEX,
            Self::Tilde => TILDE,
            Self::Diaeresis => DIAERESIS,
            Self::Cedilla => CEDILLA,
            Self::DoubleAcute => DOUBLE_ACUTE,
            Self::Macron => MACRON,
            Self::DotAbove => DOT_ABOVE,
        };
        let lower = base.to_lowercase().next()?;
        let found = table.iter().find(|(k, _)| *k == lower)?;
        // Preserve the caller's case. `char::to_uppercase` gets the historical
        // mappings right for free: U+00FD (ý) uppercases to U+0178 (Ÿ), which
        // a hand-written table gets wrong.
        if base.is_uppercase() {
            return found.1.to_uppercase().next();
        }
        Some(found.1)
    }

    /// Compose `base` with this accent, falling back to base followed by the
    /// combining mark when no precomposed form exists.
    ///
    /// The fallback is what makes this complete: `x` + acute has no precomposed
    /// character, so the correct output is `"x\u{0301}"`, two code points that
    /// render as one glyph.
    ///
    /// ```
    /// # use input_kit::DeadKey;
    /// assert_eq!(DeadKey::Acute.compose('e'), "é");   // precomposed
    /// assert_eq!(DeadKey::Acute.compose('x'), "x\u{0301}"); // base + mark
    /// ```
    #[must_use]
    pub fn compose(self, base: char) -> String {
        if let Some(precomposed) = self.apply(base) {
            let mut s = String::with_capacity(4);
            s.push(precomposed);
            return s;
        }
        let mut s = String::with_capacity(4);
        s.push(base);
        s.push(self.combining());
        s
    }

    /// The Unicode combining mark for this accent.
    #[must_use]
    pub fn combining(self) -> char {
        match self {
            Self::Acute => '\u{0301}',
            Self::Grave => '\u{0300}',
            Self::Circumflex => '\u{0302}',
            Self::Tilde => '\u{0303}',
            Self::Diaeresis => '\u{0308}',
            Self::Cedilla => '\u{0327}',
            Self::DoubleAcute => '\u{030B}',
            Self::Macron => '\u{0304}',
            Self::DotAbove => '\u{0307}',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grapheme;

    #[test]
    fn precomposed_forms_exist_for_the_latin1_set() {
        assert_eq!(DeadKey::Acute.apply('a'), Some('\u{00E1}'));
        assert_eq!(DeadKey::Grave.apply('a'), Some('\u{00E0}'));
        assert_eq!(DeadKey::Circumflex.apply('a'), Some('\u{00E2}'));
        assert_eq!(DeadKey::Tilde.apply('n'), Some('\u{00F1}'));
        assert_eq!(DeadKey::Diaeresis.apply('u'), Some('\u{00FC}'));
        assert_eq!(DeadKey::Cedilla.apply('c'), Some('\u{00E7}'));
    }

    #[test]
    fn uppercase_input_gets_the_uppercase_form() {
        assert_eq!(DeadKey::Acute.apply('E'), Some('\u{00C9}'));
        assert_eq!(DeadKey::Diaeresis.apply('U'), Some('\u{00DC}'));
        assert_eq!(DeadKey::Cedilla.apply('C'), Some('\u{00C7}'));
        assert_eq!(DeadKey::Tilde.apply('N'), Some('\u{00D1}'));
    }

    #[test]
    fn a_missing_precomposed_form_falls_back_to_a_combining_mark() {
        assert_eq!(DeadKey::Acute.apply('x'), None);
        assert_eq!(DeadKey::Acute.compose('x'), "x\u{0301}");
        assert_eq!(DeadKey::Cedilla.compose('x'), "x\u{0327}");
    }

    /// The point of the fallback: base + mark must render as ONE grapheme, so
    /// the caret math in `grapheme` still works on it.
    #[test]
    fn the_composing_fallback_is_a_single_grapheme() {
        for key in [
            DeadKey::Acute,
            DeadKey::Grave,
            DeadKey::Circumflex,
            DeadKey::Tilde,
            DeadKey::Diaeresis,
            DeadKey::Cedilla,
            DeadKey::DoubleAcute,
            DeadKey::Macron,
            DeadKey::DotAbove,
        ] {
            for base in ['x', 'q', 'v'] {
                let composed = key.compose(base);
                assert_eq!(
                    grapheme::count(&composed),
                    1,
                    "{key:?} + {base} = {composed:?} is not one cluster"
                );
            }
        }
    }

    #[test]
    fn precomposed_and_fallback_agree_on_rendering_width() {
        // The precomposed and decomposed spellings of "é" are different
        // strings that must look identical.
        assert_eq!(grapheme::count("é"), 1);
        assert_eq!(grapheme::count("e\u{0301}"), 1);
    }

    #[test]
    fn the_mark_is_a_nonspacing_combining_character() {
        for key in [
            DeadKey::Acute,
            DeadKey::Grave,
            DeadKey::Circumflex,
            DeadKey::Tilde,
            DeadKey::Diaeresis,
            DeadKey::Cedilla,
            DeadKey::DoubleAcute,
            DeadKey::Macron,
            DeadKey::DotAbove,
        ] {
            let m = key.combining();
            let c = m as u32;
            assert!(
                (0x0300..=0x036F).contains(&c),
                "{key:?} mark U+{c:04X} is not a combining diacritic"
            );
        }
    }

    #[test]
    fn every_dead_key_composes_with_every_letter() {
        // No panics, no empty output, whatever the letter.
        for key in [
            DeadKey::Acute,
            DeadKey::Grave,
            DeadKey::Circumflex,
            DeadKey::Tilde,
            DeadKey::Diaeresis,
            DeadKey::Cedilla,
            DeadKey::DoubleAcute,
            DeadKey::Macron,
            DeadKey::DotAbove,
        ] {
            for base in ('a'..='z').chain('A'..='Z') {
                let s = key.compose(base);
                assert_eq!(grapheme::count(&s), 1, "{key:?}+{base}");
            }
        }
    }
}
