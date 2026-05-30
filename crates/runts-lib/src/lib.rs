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

#[cfg(test)]
mod tests {
    #[test]
    fn test_is_browser_constant() {
        // IS_BROWSER is false in this crate (server-side/runtime)
        assert!(!crate::runtime::IS_BROWSER);
    }

    #[test]
    fn test_prelude_access() {
        use crate::runtime::prelude::*;
        // Just verify prelude items are accessible
        let _node = VNode::element("div");
        let _sig = Signal::new(42i32);
        let _computed: Computed<i32> = crate::runtime::signals::computed(|| 0i32);
    }
}
