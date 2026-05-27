//! Runtime prelude - commonly used items
//!
//! Import with: `use runts_lib::runtime::prelude::*;`

// Re-export common types
pub use super::vdom::VNode;
pub use super::islands::{Island, IslandProps, HydrationMode, IslandRenderer, IslandRegistry};
pub use super::hooks::{
    use_state, use_effect, use_ref, use_memo, use_callback,
    use_reducer, use_context, create_context, use_id,
    Signal, Computed, Ref,
};
pub use super::server::{PageProps, HandlerContext, Handler, Request, Response};

// Macros re-exported from runts-macros
pub use crate::macros::{html, component};
