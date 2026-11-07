//! What a compilation expects its parsed program to be, and the eval context it compiles in.
use oxc_span::Span;

use crate::bytecode::Atom;

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct EvalContext {
    pub in_function: bool,
    pub home_atom: Option<Atom>,
    pub super_calls: bool,
    pub class_initializer: bool,
}

/// What a compilation expects its parsed program to be.
#[derive(Clone, Copy)]
pub(super) enum ParsedProgramShape {
    Any,
    /// A script whose functions keep their records: a host script whose program keeps only
    /// part of its compiled text (a static body), from which they cannot compile again, or an
    /// application's entry, whose own functions mostly run.
    KeptRecords,
    DynamicFunction { parameter_list_end: usize },
    Eval(EvalContext, Option<Span>),
}

impl ParsedProgramShape {
    /// Whether each function compiles again from its own text alone: a script's or module's,
    /// or an eval's that its caller gives no context (indirect or global: unwrapped, a script).
    pub(super) fn replayable(self, private_names: &[(String, String)], annex_b_forbidden: &[String]) -> bool {
        matches!(self, Self::Any) || matches!(self, Self::Eval(context, None) if context == EvalContext::default()
            && private_names.is_empty() && annex_b_forbidden.is_empty())
    }
}
