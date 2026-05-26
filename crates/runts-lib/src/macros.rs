//! Macros for runts
//!
//! This module provides useful macros for building runts applications.

/// Create a VNode element
///
/// # Example
/// ```rust,ignore
/// let node = html! {
///     <div class="container">
///         <h1>"Hello"</h1>
///         <p>{ message }</p>
///     </div>
/// };
/// ```
#[macro_export]
macro_rules! html {
    // Match: <tag />
    (< $tag:ident />) => {{
        $crate::runtime::VNode::element(stringify!($tag))
    }};
}
