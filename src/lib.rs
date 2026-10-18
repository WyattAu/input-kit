//! Toolkit-neutral text-composition semantics for Rust.
//!
//! Typing Japanese, Vietnamese, or Spanish with a dead-key layout is the one
//! input problem every GUI toolkit in the ecosystem solves separately and
//! incompletely. An IME sends *preedit* text that the widget must render
//! inline while the user is still forming a syllable, then either commit it or
//! abandon it. Each platform reports that caret in a different unit (UTF-16 on
//! Win32, macOS, and the web; bytes on Wayland and X11), and every one of those
//! units can land in the middle of a grapheme cluster. Get it wrong and the
//! preedit renders as mojibake, or a combining mark is orphaned, or a
//! half-typed syllable vanishes when the widget loses focus.
//!
//! `input-kit` is the one implementation, with no windowing or widget
//! dependency:
//!
//! - [`Composer`] — the preedit state machine. Feed it any platform's event
//!   dialect, get back an unambiguous *render this* or *insert that*.
//! - [`Preedit`] — in-progress text with the caret normalised from any
//!   platform's unit onto a grapheme-cluster index.
//! - [`grapheme`] — caret-to-byte mapping that refuses to split a cluster.
//! - [`DeadKey`] — the accented-Latin composition table, shared instead of
//!   reimplemented per platform.
//!
//! # What this is not
//!
//! This is not a keymap or action layer. [`tui-actions`](https://crates.io/crates/tui-actions)
//! owns chord parsing, action binding, and modal stacks; `input-kit` owns what
//! happens *below* that, when the platform is composing text rather than
//! reporting a keystroke.
//!
//! # Example
//!
//! ```
//! use input_kit::{Composer, InputEvent, Output, Preedit};
//!
//! let mut composer = Composer::new();
//!
//! // A Japanese IME opens a preedit ("に") and extends it ("にほ").
//! composer.handle(InputEvent::Preedit(Preedit::at_end("に")));
//! composer.handle(InputEvent::Preedit(Preedit::at_end("にほ")));
//!
//! // Render it inline under the caret.
//! let preedit = composer.preedit().expect("composing");
//! assert_eq!(preedit.text(), "にほ");
//!
//! // The user picks a candidate and Enter commits it.
//! match composer.handle(InputEvent::Commit("日本".into())) {
//!     Output::Commit(text) => { /* insert `text` at the document caret */ }
//!     other => unreachable!("commit yields a commit: {other:?}"),
//! }
//! assert!(!composer.is_composing());
//! ```
//!
//! # Scope and layer
//!
//! L1 substrate in the estate's layer model: no estate dependencies, no
//! windowing, no widget toolkit, no terminal. `no_std` plus `alloc`.

#![no_std]
#![deny(missing_docs)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod composer;
pub mod deadkey;
pub mod grapheme;
pub mod preedit;

pub use composer::{Composer, InputEvent, Output};
pub use deadkey::DeadKey;
pub use preedit::Preedit;

/// The estate layer this crate occupies.
pub const LAYER: &str = "L1";

#[cfg(test)]
mod integration {
    //! Cross-module tests. The per-module tests prove each piece alone; these
    //! prove the pieces compose, which is the property a toolkit consumer
    //! actually depends on.

    use super::*;
    use alloc::string::String;

    /// A dead key flowing through the composer: press Option+e on a layout
    /// with no IME, get "é" as ordinary committed text.
    #[test]
    fn a_dead_key_composes_through_the_composer() {
        let mut c = Composer::new();

        // The dead key itself shows as a one-character preedit.
        let mark = DeadKey::Acute.mark();
        c.handle(InputEvent::Preedit(Preedit::at_end(
            String::from(mark),
        )));
        assert!(c.is_composing());

        // The base letter commits the pair.
        let composed = DeadKey::Acute.compose('e');
        let out = c.handle(InputEvent::Commit(composed.clone()));
        assert_eq!(out, Output::Commit(composed.clone()));
        assert!(!c.is_composing());
    }

    /// Focus loss mid-syllable must not lose the user's half-typed character.
    /// This is the bug every hand-rolled IME path has.
    #[test]
    fn focus_loss_mid_syllable_recovers_the_text() {
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("にほ")));
        let recovered = c.flush().unwrap_or_default();
        assert_eq!(recovered, "にほ", "flush dropped a pending preedit");
        assert!(!c.is_composing());
    }

    /// A cancelled composition must lose the text, which is the opposite
    /// requirement, and the two must not be confused.
    #[test]
    fn a_cancelled_composition_loses_the_text() {
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("にほ")));
        assert_eq!(c.handle(InputEvent::Reset), Output::Cancelled);
        assert_eq!(c.flush(), None);
    }

    /// Combining text: insert decomposed, and the caret math that a renderer
    /// depends on must hold on the result.
    #[test]
    fn decomposed_text_stays_addressable() {
        let mut c = Composer::new();
        c.handle(InputEvent::Insert("cafe\u{0301}".into()));
        assert!(c.preedit().is_none(), "direct insert does not compose");

        let s = "cafe\u{0301}";
        assert_eq!(grapheme::count(s), 4);
        // Deleting the cluster takes the mark with it.
        assert_eq!(grapheme::delete(s, 3..4).as_deref(), Some("caf"));
    }
}