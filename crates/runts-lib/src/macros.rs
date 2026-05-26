//! Macros for runts
//!
//! This module provides useful macros for building runts applications.

/// Create a VNode element
///
/// # Example
/// ```rust,ignore
/// let node = html! {
///     <div>
///         <h1>"Hello"</h1>
///         <p>{ message }</p>
///     </div>
/// };
/// ```
#[macro_export]
macro_rules! html {
    // Match: <tag>children</tag>
    (< $tag:ident > $($inner:tt)* </ $closing:ident >) => {{
        $crate::runtime::vdom::VNode::element(stringify!($tag))
    }};
    
    // Match: <tag />
    (< $tag:ident />) => {{
        $crate::runtime::vdom::VNode::element(stringify!($tag))
    }};
}
