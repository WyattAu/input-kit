//! The composition state machine.
//!
//! An IME is a protocol, and every GUI toolkit reimplements it slightly
//! differently. [`Composer`] is the one implementation: feed it the toolkit's
//! events in whatever dialect, get back an unambiguous instruction to render
//! a preedit or insert committed text.
//!
//! # The invariant
//!
//! **No character is invented and none is lost.** Across any sequence of
//! events, the concatenation of every [`Output::Commit`] the composer emits,
//! followed by the preedit still active, equals the concatenation of the text
//! the caller fed in. A composition that drops a character, duplicates one, or
//! emits one the user never typed is a bug in this crate, and the
//! `round_trip_*` tests assert it over hostile sequences.
//!
//! ```
//! use input_kit::{Composer, InputEvent, Output, Preedit};
//!
//! let mut c = Composer::new();
//! // The IME opens a preedit: "に"
//! assert!(matches!(c.handle(InputEvent::Preedit(Preedit::at_end("に"))), Output::Preedit(_)));
//! // ...grows it to "にほ"
//! assert!(matches!(c.handle(InputEvent::Preedit(Preedit::at_end("にほ"))), Output::Preedit(_)));
//! // ...and commits on Enter.
//! assert_eq!(
//!     c.handle(InputEvent::Commit("日本".into())),
//!     Output::Commit("日本".into()),
//! );
//! assert!(!c.is_composing());
//! ```

use crate::preedit::Preedit;
use alloc::string::String;

/// An event from the toolkit, in any dialect the platform speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// Text that bypasses composition: ordinary typing with no IME active, or
    /// a paste. Emitted straight through as a commit.
    Insert(String),
    /// The IME reports its in-progress text.
    Preedit(Preedit),
    /// The IME finished and produced final text.
    Commit(String),
    /// The IME abandoned composition (Escape, focus loss, language switch).
    Reset,
}

impl InputEvent {
    /// Build a preedit event from a platform caret in **UTF-16 code units**
    /// (Win32, macOS, web).
    #[must_use]
    pub fn preedit_utf16(text: impl Into<String>, caret: u16) -> Self {
        Self::Preedit(Preedit::from_utf16(text, caret))
    }

    /// Build a preedit event from a **byte offset** caret (Wayland, X11).
    #[must_use]
    pub fn preedit_bytes(text: impl Into<String>, caret: usize) -> Self {
        Self::Preedit(Preedit::from_bytes(text, caret))
    }
}

/// What the toolkit should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Nothing changed. A toolkit may skip the frame.
    Idle,
    /// Render this preedit inline at the document caret.
    Preedit(Preedit),
    /// Insert this text at the document caret, replacing nothing. Any active
    /// preedit is over.
    Commit(String),
    /// Remove any rendered preedit and insert nothing.
    Cancelled,
}

/// The composition state machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composer {
    preedit: Option<Preedit>,
}

impl Composer {
    /// A composer with no active composition.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an IME composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
    }

    /// The active preedit, if any.
    #[must_use]
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// Feed one event.
    pub fn handle(&mut self, event: InputEvent) -> Output {
        match event {
            InputEvent::Insert(text) => {
                // A platform that switches to direct-insert mode while an IME
                // is composing has aborted the composition. Drop the preedit
                // so the two do not interleave into one corrupt string.
                self.preedit = None;
                Output::Commit(text)
            }
            InputEvent::Preedit(next) => {
                if next.is_empty() {
                    // An empty preedit means "still composing, nothing to
                    // show yet". Preserve composition state rather than
                    // ending it: several platforms emit this while waiting.
                    return match self.preedit {
                        Some(ref cur) if !cur.is_empty() => Output::Idle,
                        Some(_) | None => {
                            self.preedit = None;
                            Output::Idle
                        }
                    };
                }
                self.preedit = Some(next.clone());
                Output::Preedit(next)
            }
            InputEvent::Commit(text) => {
                self.preedit = None;
                Output::Commit(text)
            }
            InputEvent::Reset => {
                if self.preedit.take().is_some() {
                    Output::Cancelled
                } else {
                    Output::Idle
                }
            }
        }
    }

    /// Feed a run of events, returning every non-idle output in order.
    ///
    /// The convenience form for a platform that delivers a batch.
    pub fn feed<I: IntoIterator<Item = InputEvent>>(&mut self, events: I) -> alloc::vec::Vec<Output> {
        events
            .into_iter()
            .map(|e| self.handle(e))
            .filter(|o| !matches!(o, Output::Idle))
            .collect()
    }

    /// End composition because the widget lost focus.
    ///
    /// Returns `Some(text)` when a preedit was active and should be inserted
    /// as ordinary text, or `None` when there was nothing pending. A toolkit
    /// that drops the preedit on focus loss loses the user's half-typed
    /// syllable; a toolkit that commits it may insert text the user cancelled.
    /// The right call is *commit*, because the user typed it, and it is this
    /// method that makes that the default.
    pub fn flush(&mut self) -> Option<String> {
        self.preedit.take().map(|p| p.to_commit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::borrow::ToOwned;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Drive a composer over `events`, returning every non-idle output.
    fn run(events: Vec<InputEvent>) -> Vec<Output> {
        Composer::new().feed(events)
    }

    #[test]
    fn the_happy_path() {
        let out = run(vec![
            InputEvent::Preedit(Preedit::at_end("に")),
            InputEvent::Preedit(Preedit::at_end("にほ")),
            InputEvent::Commit("日本".into()),
        ]);
        assert_eq!(out.len(), 3);
        assert!(matches!(out.first(), Some(Output::Preedit(_))));
        assert!(matches!(out.get(1), Some(Output::Preedit(_))));
        assert_eq!(
            out.get(2),
            Some(&Output::Commit("日本".into())),
            "third output"
        );
    }

    #[test]
    fn reset_ends_composition_once() {
        let mut c = Composer::new();
        assert_eq!(c.handle(InputEvent::Reset), Output::Idle);
        c.handle(InputEvent::Preedit(Preedit::at_end("か")));
        assert_eq!(c.handle(InputEvent::Reset), Output::Cancelled);
        assert!(!c.is_composing());
        // A second reset with nothing active is idle, not another cancel.
        assert_eq!(c.handle(InputEvent::Reset), Output::Idle);
    }

    /// An empty preedit must not end composition: several platforms emit one
    /// while they are still deciding what the syllable is.
    #[test]
    fn an_empty_preedit_does_not_end_composition() {
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("か")));
        assert_eq!(c.handle(InputEvent::Preedit(Preedit::empty())), Output::Idle);
        assert!(c.is_composing(), "composition survived the empty preedit");
    }

    /// Direct insert while composing must not interleave with the preedit.
    #[test]
    fn direct_insert_aborts_an_active_preedit() {
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("か")));
        assert_eq!(c.handle(InputEvent::Insert("x".into())), Output::Commit("x".into()));
        assert!(!c.is_composing(), "preedit cleared by direct insert");
    }

    #[test]
    fn flush_recovers_the_typed_text_on_focus_loss() {
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("にほ")));
        assert_eq!(c.flush().as_deref(), Some("にほ"));
        assert!(!c.is_composing());
        assert_eq!(c.flush(), None);
    }

    /// THE invariant, in its two checkable halves.
    ///
    /// 1. The committed stream is exactly the `Insert`/`Commit` input text, in
    ///    order, each appearing once. A preedit that never commits is
    ///    legitimately discarded by a `Reset`; that is a cancel, not a loss.
    /// 2. Whatever preedit survives is the last one seen, and it is *not*
    ///    also in the commit stream, so nothing is both pending and inserted.
    #[test]
    fn the_round_trip_neither_invents_nor_loses_text() {
        let sequences: Vec<Vec<InputEvent>> = vec![
            vec![InputEvent::Insert("hello".into())],
            vec![
                InputEvent::Preedit(Preedit::at_end("に")),
                InputEvent::Preedit(Preedit::at_end("にほ")),
                InputEvent::Commit("日本".into()),
            ],
            vec![
                InputEvent::Preedit(Preedit::at_end("ñ")),
                InputEvent::Insert("!".into()),
            ],
            vec![
                InputEvent::Preedit(Preedit::at_end("a")),
                InputEvent::Reset,
                InputEvent::Insert("b".into()),
            ],
            // A commit with no preedit (platform goes straight to commit).
            vec![InputEvent::Commit("即".into())],
            // Empty preedits interleaved, including after a commit.
            vec![
                InputEvent::Preedit(Preedit::empty()),
                InputEvent::Preedit(Preedit::at_end("か")),
                InputEvent::Preedit(Preedit::empty()),
                InputEvent::Commit("漢".into()),
                InputEvent::Preedit(Preedit::empty()),
            ],
            // A chatty platform interleaving preedits and commits.
            vec![
                InputEvent::Preedit(Preedit::at_end("あ")),
                InputEvent::Commit("a".into()),
                InputEvent::Preedit(Preedit::at_end("い")),
                InputEvent::Commit("b".into()),
                InputEvent::Insert("c".into()),
            ],
            // A preedit abandoned by reset contributes nothing.
            vec![
                InputEvent::Preedit(Preedit::at_end("語")),
                InputEvent::Reset,
            ],
            // Repeated identical preedits (some platforms re-send unchanged).
            vec![
                InputEvent::Preedit(Preedit::at_end("か")),
                InputEvent::Preedit(Preedit::at_end("か")),
                InputEvent::Commit("か".into()),
            ],
            // Pastel: combining marks, astral chars, ZWJ emoji.
            vec![
                InputEvent::Preedit(Preedit::at_end("e\u{0301}")),
                InputEvent::Commit("e\u{0301}".into()),
                InputEvent::Insert("\u{1F469}\u{200D}\u{1F4BB}".into()),
            ],
        ];

        for seq in sequences {
            let mut c = Composer::new();
            let out = c.feed(seq.clone());

            // Half 1: the committed stream is exactly the committed input.
            let want_committed: String = seq
                .iter()
                .filter_map(|e| match e {
                    InputEvent::Insert(t) | InputEvent::Commit(t) => Some(t.as_str()),
                    InputEvent::Preedit(_) | InputEvent::Reset => None,
                })
                .collect();
            let got_committed: String = out
                .iter()
                .filter_map(|o| match o {
                    Output::Commit(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                got_committed, want_committed,
                "commit stream drifted for {seq:?}"
            );

            // Half 2: a surviving preedit is the last non-empty one seen, and
            // is not double-counted as committed.
            let want_pending: Option<String> = seq.iter().fold(None, |acc, e| match e {
                InputEvent::Preedit(p) if !p.is_empty() => Some(p.text().to_owned()),
                // An empty preedit is a "still composing" ping; it neither
                // creates nor clears pending text.
                InputEvent::Preedit(_) => acc,
                // A commit, a direct insert, or a reset all end composition.
                InputEvent::Insert(_)
                | InputEvent::Commit(_)
                | InputEvent::Reset => None,
            });
            let got_pending = c.preedit().map(|p| p.text().to_owned());
            assert_eq!(got_pending, want_pending, "pending state for {seq:?}");
        }
    }

    /// Every structural rule a buggy composer would break, one assertion each.
    #[test]
    fn the_state_machine_holds_its_structural_rules() {
        // A commit always ends composition, even with no preedit active.
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("か")));
        c.handle(InputEvent::Commit("x".into()));
        assert!(!c.is_composing(), "commit must clear the preedit");

        // Reset is idempotent.
        let mut c = Composer::new();
        c.handle(InputEvent::Preedit(Preedit::at_end("か")));
        assert_eq!(c.handle(InputEvent::Reset), Output::Cancelled);
        assert_eq!(c.handle(InputEvent::Reset), Output::Idle);

        // Re-sending an identical preedit still reports a preedit (the
        // platform asked for a redraw); it is not idempotent-Idle.
        let mut c = Composer::new();
        let p = Preedit::at_end("か");
        assert!(matches!(c.handle(InputEvent::Preedit(p.clone())), Output::Preedit(_)));
        assert!(matches!(c.handle(InputEvent::Preedit(p)), Output::Preedit(_)));
    }
}