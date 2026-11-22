//! The control edges an instruction's step takes, a leaf the stencil build includes as well.

/// Which of an instruction's two control edges it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// The jump target.
    Target,
    /// The record leaves the lane run: its activation suspended into its job (`Frame::suspend_await`).
    Leave,
    /// The record switched the running register file: its frame made a suspension that transfers
    /// control to another activation (`Frame::suspend_yield`), which the frame holds for the lane to resume.
    Transfer,
    /// The instruction after this one.
    Following,
    /// The entry a branch table's index picks (`k`, clamped to the default entry): control goes to
    /// the jump record `k` places after the table record.
    Entry(u32),
}
