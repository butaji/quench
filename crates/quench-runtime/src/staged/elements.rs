//! A dense array backing's element read as a staged operation: the interpreter reads it on
//! `Concrete`, a specializer emits it.

use super::{Concrete, Stage};
use super::region::{Access, Region};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
pub use super::double_view::{double_view, reloaded_double_view};

#[path = "elements/dense.rs"]
mod dense;
#[path = "elements/dense_view.rs"]
mod dense_view;
#[path = "elements/backing.rs"]
mod backing;
#[path = "elements/record.rs"]
mod record;
#[path = "elements/writable.rs"]
mod writable;
#[path = "elements/write.rs"]
mod write;
#[path = "elements/concrete.rs"]
mod concrete;
pub use dense::*;
pub use dense_view::*;
pub use backing::*;
pub use record::*;
pub use writable::*;
pub use write::*;
pub use concrete::*;

#[cfg(test)]
mod tests;
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
