#![allow(dead_code)]

//! runts-lib - Shared runtime library for runts
//!
//! This crate provides the runtime support for runts applications,
//! including:
//!
//! - Virtual DOM (VNode)
//! - Preact-compatible hooks
//! - Fine-grained signals
//! - Islands architecture
//! - Server utilities (Fresh compatibility)
//!
//! # Example
//!
//! ```rust,ignore
//! use runts_lib::runtime::prelude::*;
//!
//! #[component]
//! pub fn Counter(props: CounterProps) -> VNode {
//!     let (count, set_count) = use_state(|| 0);
//!     
//!     html! {
//!         <div class="counter">
//!             <p>Count: { count }</p>
//!             <button on_click={ move |_| set_count(count + 1) }>+</button>
//!         </div>
//!     }
//! }
//! ```

pub mod macros;
pub mod runtime;

pub use runtime::prelude::*;
