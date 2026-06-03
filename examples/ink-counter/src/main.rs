//! Real, minimal Ink-style counter for `runts-ink`.
//!
//! This is a Rust binary that hosts a pure-Rust Ink
//! component tree (no JS) and runs a Ratatui event loop
//! driven by crossterm. The shape mirrors what a
//! `runts build --plugin ink` compile would produce for
//! the equivalent `.tsx` source: the JSX is hand-rolled
//! in Rust via the `runts_ink::*` builder API.
//!
//! The point of this example is twofold:
//!
//! 1. **Reference implementation** — anyone writing
//!    their own TUI in pure Rust (without going through
//!    `.tsx` + the runts compiler) can copy this file
//!    and adapt it.
//! 2. **End-to-end test** — `cargo run --bin
//!    ink-counter` boots a TUI on the user's terminal.
//!    It's a smoke test for the runtime path.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use runts_ink::{
    render, Box as InkBox, Color, InputEvent, Key, Props, RenderOptions, Text, VNode,
};

fn main() -> Result<()> {
    // State: a single counter. The Ink pattern is to
    // keep state in a closure that re-builds the
    // component tree on each render. We use `Rc<Cell>`
    // so the closure (which is `FnMut` from the
    // renderer's perspective) and the input loop can
    // both mutate the same value.
    let count: Rc<Cell<u32>> = Rc::new(Cell::new(0));

    // The root component: a column with a heading, the
    // count, and a hint. The `props` arg is ignored for
    // this example; real apps would thread props from
    // `render` into the component. The closure clones
    // the `Rc<Cell>` so the input loop retains its
    // own handle on the same counter.
    let count_for_root = Rc::clone(&count);
    let mut root: runts_ink::RootFn = Box::new(move |_props: Props| -> VNode {
        let n = count_for_root.get();
        InkBox::column()
            .padding(1)
            .child(Text::new("Ink Counter").bold().color(Color::Cyan))
            .child(Text::new(""))
            .child(Text::new(format!("Count: {n}")).bold())
            .child(Text::new(""))
            .child(Text::new("Press up/down to change count.").italic())
            .child(Text::new("Press q to quit.").italic())
            .into()
    });

    // Render options. We use `exit_on_q` so the test
    // (and the user's Ctrl-C handler) can drain the
    // event loop cleanly.
    let mut options = RenderOptions::new();
    options.exit_on_q = true;
    options.tick_ms = 100;

    // Mount the root. `render` returns an `Instance`
    // that owns the terminal. We pass `&mut root` so
    // we can call `instance.redraw(&mut root)` later
    // to re-render after mutating state.
    let mut instance = render(&mut root, Props::new(), options)?;

    // Drive the input loop. The render loop is on a
    // background thread; this thread polls for key
    // events, mutates state, and re-renders.
    while instance.is_running() {
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(input) = classify_key(&key) {
                        match input {
                            InputAction::Up => {
                                count.set(count.get().saturating_add(1));
                            }
                            InputAction::Down => {
                                count.set(count.get().saturating_sub(1));
                            }
                            InputAction::Quit => {
                                instance.unmount()?;
                                return Ok(());
                            }
                        }
                        // Re-render with the new count.
                        instance.redraw(&mut root)?;
                    }
                }
                _ => {}
            }
        }
    }
    instance.unmount()
}

/// Translate a crossterm key into an Ink-shaped
/// `InputEvent` and decide which app action to take.
enum InputAction {
    /// Up arrow — increment the counter.
    Up,
    /// Down arrow — decrement the counter.
    Down,
    /// q or Ctrl-C — exit.
    Quit,
}

fn classify_key(key: &crossterm::event::KeyEvent) -> Option<InputAction> {
    // Build an Ink-shaped `InputEvent` from the raw
    // crossterm event. This is the same conversion the
    // runts-ratatui plugin would do for a `useInput`
    // handler. We can then match on the `key.up_arrow`
    // / `key.down_arrow` flags.
    let ink = Key::from_crossterm(key.clone());
    let _: InputEvent = InputEvent::from_crossterm(key.clone());
    if ink.up_arrow {
        Some(InputAction::Up)
    } else if ink.down_arrow {
        Some(InputAction::Down)
    } else if matches!(key.code, KeyCode::Char('q')) || key.code == KeyCode::Esc {
        Some(InputAction::Quit)
    } else {
        None
    }
}
