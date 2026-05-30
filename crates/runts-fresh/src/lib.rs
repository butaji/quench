//! Fresh/Preact web framework plugin for runts.

mod codegen;
mod plugin;

pub use codegen::{jsx_element, jsx_expr, jsx_fragment, jsx_text, page_component};
pub use plugin::FreshPlugin;
