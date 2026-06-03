//! `runts-ink` — Ink-style components for `runts` + Ratatui.
//!
//! This crate is the **framework** layer of `runts` for TUI
//! apps. The `runts` core is about the language (TSX →
//! Rust); `runts-ink` is about the components and the
//! runtime that hosts them.
//!
//! # Architecture
//!
//! ```text
//! Ink TSX (React reconciler)
//!     │
//!     ▼
//! rquickjs (QuickJS runtime)
//!     │
//!     ▼
//! Reconciler Bridge (Rust)
//!     ├─► Taffy tree (CSS flexbox layout)
//!     ├─► Event loop (crossterm)
//!     └─► Ratatui render (immediate-mode widgets)
//! ```
//!
//! The user's `.tsx` file is compiled by `runts` into a
//! Rust binary that:
//!
//! 1. Hosts an `rquickjs` context running the React
//!    reconciler + Ink's component code.
//! 2. Maintains a `Taffy` flexbox tree. The JS reconciler
//!    pushes "tree ops" (CreateNode / SetStyle / AddChild
//!    / SetText) over an mpsc channel; the Rust render
//!    loop applies them.
//! 3. Polls `crossterm::event` for key / resize / paste
//!    events and routes them to the JS `useInput` /
//!    `useFocus` / `useApp` handlers.
//! 4. Renders the Taffy-computed layout to Ratatui each
//!    frame.
//!
//! # Public API
//!
//! * **Components** — [`Box`], [`Text`], [`Newline`],
//!   [`Spacer`], [`Static`], [`Transform`]. The user
//!   writes these in `.tsx` and the runts-ratatui plugin
//!   emits Rust that constructs them.
//! * **Hook types** — [`InputEvent`], [`FocusId`], etc.
//!   These are the JSON shape the JS reconciler and the
//!   Rust event loop exchange.
//! * **Entry point** — [`render`] / [`render_to_string`] /
//!   [`Instance`]. The render entry point boots the
//!   runtime, mounts the root, and runs the event loop.
//!
//! See the `examples/ink-counter/` for a working end-to-end
//! example.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod components;
mod events;
pub mod js_bridge;
mod props;
mod render;
mod style;
mod taffy_bridge;
mod vnode;

pub use components::{
    Box, Color, FlexDirection, Newline, Spacer, Static, Text, Transform,
};
pub use events::{
    FocusId, InputEvent, Key, MouseEvent, PasteEvent, ResizeEvent, WindowSize,
};
pub use props::Props;
pub use render::{render, render_to_string, Instance, RenderOptions, RootFn};
pub use style::{BorderStyle, Borders, Display, Overflow, Position, Wrap};
pub use vnode::{VNode, VNodeContent};
