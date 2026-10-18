# input-kit

Toolkit-neutral text-composition semantics for Rust: IME preedit state
machine, grapheme-cluster caret mapping, and dead-key composition.

`no_std` + `alloc`. No windowing, no widget toolkit, no terminal. L1
substrate: no estate dependencies.

```toml
[dependencies]
input-kit = "0.1"
```

## The problem

Typing Japanese, Vietnamese, or Spanish with a dead-key layout is the one
input problem every GUI toolkit in the ecosystem solves separately and
incompletely.

An IME sends **preedit** text that the widget must render inline while the
user is still forming a syllable, then either commit it or abandon it. Each
platform reports that caret in a different unit:

| Platform | Unit |
| --- | --- |
| Win32 IMM/TSF | UTF-16 code unit index |
| macOS marked text | UTF-16 code unit range |
| Web `InputEvent` | UTF-16 code unit index |
| Wayland / X11 | byte offset |

All four can land **inside** a grapheme cluster. A toolkit that trusts its
platform's unit and slices the string will render a preedit as mojibake,
orphan a combining mark, or drop a half-typed syllable when the widget loses
focus.

`input-kit` is the one implementation, and it normalises every unit onto the
grapheme cluster, which is the only unit a renderer can safely slice at.

## What is in the box

- **`Composer`** — the preedit state machine. Feed it any platform's event
  dialect, get back an unambiguous *render this* or *insert that*.
- **`Preedit`** — in-progress text with the caret normalised from any
  platform's unit onto a grapheme-cluster index.
- **`grapheme`** — caret-to-byte mapping that refuses to split a cluster, plus
  cluster-aware `splice` and `delete`.
- **`DeadKey`** — the accented-Latin composition table, shared instead of
  reimplemented per platform.

## Example

```rust
use input_kit::{Composer, InputEvent, Output, Preedit};

let mut composer = Composer::new();

// A Japanese IME opens a preedit ("に") and extends it ("にほ").
composer.handle(InputEvent::Preedit(Preedit::at_end("に")));
composer.handle(InputEvent::Preedit(Preedit::at_end("にほ")));

// Render it inline under the caret.
let preedit = composer.preedit().expect("composing");
assert_eq!(preedit.text(), "にほ");

// The user picks a candidate and Enter commits it.
match composer.handle(InputEvent::Commit("日本".into())) {
    Output::Commit(text) => { /* insert `text` at the document caret */ }
    other => unreachable!("a commit yields a commit: {other:?}"),
}
```

Adapting a platform is one call:

```rust
# use input_kit::Preedit;
// Win32, macOS, web: a UTF-16 code unit index.
let p = Preedit::from_utf16("にほん", 2);
assert_eq!(p.caret(), 2);

// Wayland, X11: a byte offset.
let q = Preedit::from_bytes("cafe\u{301}", 5);
assert_eq!(q.caret(), 3);   // rounds down to the e's cluster
```

## The invariant

**No character is invented and none is lost.** Across any sequence of events,
the concatenation of every `Output::Commit` equals the `Insert`/`Commit` text
fed in, in order, once each; and whatever preedit survives is the last one
seen. A composition that drops a character, duplicates one, or emits one the
user never typed is a bug in this crate.

## What this is not

This is not a keymap or action layer.
[`tui-actions`](https://crates.io/crates/tui-actions) owns chord parsing,
action binding, and modal stacks. `input-kit` owns what happens *below* that,
when the platform is composing text rather than reporting a keystroke.

## Licence

MIT OR Apache-2.0.