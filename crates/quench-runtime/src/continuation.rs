#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuspensionPoint {
    Yield { pc: usize, src: u16 },
    YieldStar { pc: usize, dst: u16, iterator: u16 },
}
