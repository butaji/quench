#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopPhase {
    Init,
    Test,
    Body,
    Update,
}

#[derive(Debug, Clone, PartialEq)]
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
    Try {
        phase: crate::machine::TryPhase,
        body: crate::machine::CodeRange,
        handler: Option<crate::machine::CodeRange>,
        finalizer: Option<crate::machine::CodeRange>,
        body_resume: crate::machine::CodeRange,
        yield_dst: u16,
        catch_slot: Option<u16>,
    },
    Iterator {
        iterator: crate::value::Value,
        binding: u16,
        body: crate::machine::CodeRange,
        body_resume: crate::machine::CodeRange,
        yield_dst: u16,
        close_normal: bool,
        repeat: bool,
        slot: u16,
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

impl SuspensionPoint {
    pub(crate) fn destination(&self) -> u16 {
        match self {
            Self::Yield { src, .. } => *src,
            Self::YieldStar { dst, .. } => *dst,
            Self::Branch { yield_dst, .. }
            | Self::Try { yield_dst, .. }
            | Self::Iterator { yield_dst, .. }
            | Self::Loop { yield_dst, .. } => *yield_dst,
            Self::Nested { inner, .. } => inner.destination(),
        }
    }

    pub(crate) fn nest(self, outer: Self) -> Self {
        Self::Nested {
            inner: Box::new(self),
            outer: Box::new(outer),
        }
    }
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

pub(crate) fn attach_executed_suspension(
    code: crate::machine::CodeView<'_>,
    mut step: crate::vm::CompletionStep,
) -> Result<crate::vm::CompletionStep, crate::execute::VmError> {
    use crate::completion::Completion;

    if !matches!(step.completion, Completion::Yield(_) | Completion::Suspend(_)) {
        return Ok(step);
    }
    let pc = step
        .suspended_pc
        .or_else(|| step.next.checked_sub(1))
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let op = code
        .cold_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let point = executed_point(op, code.range(), step.next)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    step.completion = match step.completion {
        Completion::Yield(value) => Completion::YieldAt(value, point),
        Completion::Suspend(promise) => Completion::SuspendAt(promise, point),
        completion => completion,
    };
    Ok(step)
}
