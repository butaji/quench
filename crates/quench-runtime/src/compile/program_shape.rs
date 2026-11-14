//! What a compilation expects its parsed program to be, and the eval context it compiles in.
use oxc_span::Span;

use crate::bytecode::Atom;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct EvalContext {
    pub in_function: bool,
    pub home_atom: Option<Atom>,
    pub super_calls: bool,
    pub class_initializer: bool,
}

/// What a compilation expects its parsed program to be.
#[derive(Clone, Copy)]
pub(super) enum ParsedProgramShape<'a> {
    Any,
    /// A script whose functions keep their records: a host script whose program keeps only
    /// part of its compiled text (a static body), from which they cannot compile again, or an
    /// application's entry, whose own functions mostly run.
    KeptRecords,
    DynamicFunction { parameter_list_end: usize },
    Eval(EvalContext, Option<Span>),
    /// The chunk of a first-call stub: a Script of one function of a program's
    /// text, compiled as it is called, whole.
    FirstCallChunk(super::first_call::ChunkWindow<'a>),
}

impl ParsedProgramShape<'_> {
    /// Whether each function compiles again from its own text alone: a script's or module's,
    /// or an eval's that its caller gives no context (indirect or global: unwrapped, a script).
    pub(super) fn replayable(self, private_names: &[(String, String)], annex_b_forbidden: &[String]) -> bool {
        matches!(self, Self::Any) || matches!(self, Self::Eval(context, None) if context == EvalContext::default()
            && private_names.is_empty() && annex_b_forbidden.is_empty())
    }

    /// Which functions of a source of this shape of `source_bytes` compile on first call
    /// (`first_call`): an eval's that compile again from their text alone, and a direct eval's
    /// of a source of megabytes, which its caller's scopes complete when its chunk compiles. A chunk
    /// costs more than the function's compile at load (its text is parsed and checked again),
    /// so only a source that mostly goes uncalled gains: a large bundle, as engines' lazy
    /// parsing assumes of a script.
    pub(super) fn stubbing(self, replayable: bool, source_bytes: usize) -> Stubbing {
        let eval = matches!(self, Self::Eval(..));
        let direct = eval && !replayable && (source_bytes >= DIRECT_EVAL_STUB_BYTES || crate::trace::Dev::DirectEvalStubs.on());
        Stubbing { functions: eval && replayable || direct, arrows: direct }
    }

    /// Whether function bodies of a source of this shape parse lazily (`lazy_bodies`): each
    /// compiles again from its own text (`replayable`), or compiles on first call.
    pub(super) fn lazy_functions(self, replayable: bool, source_bytes: usize) -> bool {
        replayable || self.stubbing(replayable, source_bytes).functions
    }
}

/// Source bytes from which a direct eval's functions compile on first call (`stubbing`): below
/// it most of a bundle's code runs and the chunks cost more than they save.
const DIRECT_EVAL_STUB_BYTES: usize = 4 << 20;

/// Which functions of a source compile on first call.
#[derive(Clone, Copy)]
pub(super) struct Stubbing {
    pub(super) functions: bool,
    /// Arrow functions too, and the parser leaves the bodies it does not skim whole.
    pub(super) arrows: bool,
}
