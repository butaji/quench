#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopPhase {
    Init,
    Test,
    Body,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SuspensionPoint {
    Yield {
        resume: Option<crate::machine::CodeRange>,
        src: u16,
    },
    YieldStar {
        resume: Option<crate::machine::CodeRange>,
        dst: u16,
        iterator: u16,
    },
    Branch {
        body_resume: crate::machine::CodeRange,
        yield_dst: u16,
    },
    Loop {
        pc: usize,
        label: Option<String>,
        body: crate::machine::CodeRange,
        test: crate::machine::CodeRange,
        update: crate::machine::CodeRange,
        phase: LoopPhase,
        phase_resume: crate::machine::CodeRange,
        dst: u16,
        yield_dst: u16,
        post_test: bool,
        per_iteration: std::rc::Rc<[u16]>,
    },
    /// Structured suspensions compose from the innermost operation outward;
    /// each layer owns its own resume range and is resumed in stack order.
    Nested {
        inner: Box<SuspensionPoint>,
        outer: Box<SuspensionPoint>,
    },
}

pub(crate) fn executed_point(
    op: &crate::ops::Op,
    range: crate::machine::CodeRange,
    next: usize,
) -> Option<SuspensionPoint> {
    let next = u32::try_from(next).ok()?;
    let start = range.start.checked_add(next)?;
    if start > range.end {
        return None;
    }
    let resume = Some(crate::machine::CodeRange {
        code: range.code,
        start,
        end: range.end,
    });
    match op {
        crate::ops::Op::Yield { src } => Some(SuspensionPoint::Yield { resume, src: *src }),
        crate::ops::Op::Await { dst, .. } => Some(SuspensionPoint::Yield { resume, src: *dst }),
        crate::ops::Op::YieldStar { dst, iterator, .. } => Some(SuspensionPoint::YieldStar {
            resume,
            dst: *dst,
            iterator: *iterator,
        }),
        _ => None,
    }
}
