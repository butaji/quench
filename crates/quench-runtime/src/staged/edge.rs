//! The control edges an instruction's step takes, a leaf the stencil build includes as well.

/// Which of an instruction's two control edges it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Edge {
    /// The jump target.
    Target,
    /// The instruction after this one.
    Following,
}
