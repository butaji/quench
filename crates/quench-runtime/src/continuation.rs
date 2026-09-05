#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SuspensionPoint {
    Yield {
        pc: usize,
        src: u16,
    },
    YieldStar {
        pc: usize,
        dst: u16,
        iterator: u16,
    },
    Loop {
        pc: usize,
        label: Option<String>,
        body: crate::machine::CodeRange,
        test: crate::machine::CodeRange,
        update: crate::machine::CodeRange,
        body_resume: crate::machine::CodeRange,
        dst: u16,
        yield_dst: u16,
        post_test: bool,
    },
    /// Structured suspensions compose from the innermost operation outward;
    /// each layer owns its own resume range and is resumed in stack order.
    Nested {
        inner: Box<SuspensionPoint>,
        outer: Box<SuspensionPoint>,
    },
}
