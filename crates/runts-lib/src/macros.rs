//! Re-exports of runts procedural macros.
//!
//! The `#[component]` macro is defined in the `runts-macros` crate and
//! re-exported here so users only need to depend on `runts-lib`.
//!
//! The `html!` proc-macro from earlier prototypes has been removed.
//! Component authoring in Rust is not a supported workflow in the
//! current design — components are written as `.tsx` files and the
//! runts-fresh plugin transpiles them to Rust. If you need a `VNode`
//! at runtime, use the public `VNode` builder API in
//! `runts_lib::runtime::vdom` directly.

pub use runts_macros::component;
