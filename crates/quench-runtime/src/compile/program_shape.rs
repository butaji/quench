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
    /// A classic host script run from its kept form, which records the room its first-call
    /// chunks append in (`ResidualProgram::kept_chunk_room`): as `Any`, and a large one's
    /// functions compile on first call (`stubbing`).
    FirstCallScript,
    /// A script whose functions keep their records: a host script whose program keeps only
    /// part of its compiled text (a static body), from which they cannot compile again, or an
    /// application's entry, whose own functions mostly run.
    KeptRecords,
    /// A Function-constructor program: its root function and the functions nested in its body.
    /// Each nested function compiles as a stub when the source is large, as a script's do, but
    /// its captures are the body's bindings (`nested` stubs), not the global environment.
    /// `eager` compiles every body at load, as a native image must: a stub needs a compiler the
    /// image does not carry.
    DynamicFunction { parameter_list_end: usize, eager: bool },
    Eval(EvalContext, Option<Span>),
    /// The chunk of a first-call stub: a Script of one function of a program's
    /// text, compiled as it is called, whole.
    FirstCallChunk(super::first_call::ChunkWindow<'a>),
}

impl ParsedProgramShape<'_> {
    /// Whether the load checked the function a first-call chunk compiles (`ChunkWindow::checked`):
    /// its early errors were found then, so the chunk's semantic analysis need not check them again.
    pub(super) fn checked_at_load(self) -> bool {
        matches!(self, Self::FirstCallChunk(window) if window.checked())
    }

    /// Whether each function compiles again from its own text alone: a script's or module's,
    /// or an eval's that its caller gives no context (indirect or global: unwrapped, a script).
    pub(super) fn replayable(self, private_names: &[(String, String)], annex_b_forbidden: &[String]) -> bool {
        matches!(self, Self::Any | Self::FirstCallScript) || matches!(self, Self::Eval(context, None) if context == EvalContext::default()
            && private_names.is_empty() && annex_b_forbidden.is_empty())
    }

    /// Which functions of a source of this shape of `source_bytes` compile on first call
    /// (`first_call`): an eval's that compile again from their text alone, and a direct eval's
    /// of a source of megabytes, which its caller's scopes complete when its chunk compiles. A chunk
    /// costs more than the function's compile at load (its text is parsed and checked again),
    /// so only a source that mostly goes uncalled gains: a large bundle, as engines' lazy
    /// parsing assumes of a script.
    ///
    /// A first-call chunk's functions and arrows compile on their own first calls too, against
    /// the chunk's scopes: a chunk that compiled them eagerly compiled a stubbed root's whole
    /// subtree at its first call, so a bundle's module wrapper, which runs once, compiled every
    /// function of its module (babylonjs-startup: about 9,000 that never run).
    ///
    /// A host script (`FirstCallScript`) of `FIRST_CALL_SCRIPT_BYTES` or more stubs its
    /// functions as an eval's do: its chunks append to the program it runs
    /// (`ResidualProgram::restore_chunk_room`), so calls into them stay within one program. A
    /// module's never stub.
    ///
    /// A large Function-constructor body (`DynamicFunction`) stubs its nested functions as a
    /// chunk's do: not replayable, their captures are the body's bindings (`FirstCallScopes`),
    /// never the global environment, so its root functions stay empty (`first_call_roots`).
    /// Its arrows stay eager, as an indirect eval's do.
    ///
    /// A script's arrows stay eager: stubbed too, babel-wtb's load fell from 421 to 361 Ir per
    /// source byte, but its module arrows mostly run, and their chunks' second parse made the
    /// whole run cost more (first 5.66 -> 5.90 G Ir, steady 1.79 -> 1.91 G, Nov 18).
    ///
    /// Their bodies are skimmed where the skim takes them (`Stubbing::skim`): a direct eval's and a
    /// script's, whose bodies the skim declines parse whole and are checked (a script) or left
    /// unchecked (a direct eval, whose chunks check their own text).
    pub(super) fn stubbing(self, replayable: bool, source_bytes: usize, module: bool) -> Stubbing {
        if matches!(self, Self::FirstCallChunk(_)) {
            return Stubbing { functions: true, arrows: true, skim: true };
        }
        let eval = matches!(self, Self::Eval(..));
        let direct = eval && !replayable && (source_bytes >= DIRECT_EVAL_STUB_BYTES);
        let script = matches!(self, Self::FirstCallScript) && !module && source_bytes >= FIRST_CALL_SCRIPT_BYTES;
        let dynamic = matches!(self, Self::DynamicFunction { eager: false, .. }) && source_bytes >= FIRST_CALL_SCRIPT_BYTES;
        Stubbing { functions: (eval || script) && replayable || direct || dynamic, arrows: direct, skim: direct || script }
    }

    /// Where in the source lazily parsed bodies begin (`lazy_bodies::parse`): past a chunk's
    /// own function, past a Function-constructor program's root function, inside the method an
    /// eval in a method's context is wrapped in (whose body is the eval's code), else from the start.
    pub(super) fn lazy_from(self) -> u32 {
        match self {
            Self::FirstCallChunk(window) => window.lazy_from(),
            Self::DynamicFunction { .. } => DYNAMIC_FUNCTION_ROOT_START + 1,
            Self::Eval(_, Some(body)) => body.start,
            _ => 0,
        }
    }

    /// Whether function bodies of a source of this shape parse lazily (`lazy_bodies`): each
    /// compiles again from its own text (`replayable`), or compiles on first call.
    pub(super) fn lazy_functions(self, replayable: bool, source_bytes: usize) -> bool {
        replayable || self.stubbing(replayable, source_bytes, false).functions
    }
}

/// Source bytes from which a direct eval's functions compile on first call (`stubbing`): below
/// it most of a bundle's code runs and the chunks cost more than they save.
const DIRECT_EVAL_STUB_BYTES: usize = 4 << 20;

/// Where a Function-constructor program's root function starts: its source is the expression
/// `(function anonymous(…`, so past the `(` (`DynamicFunctionSource::new`). A lazy body's function
/// at or past `lazy_from` parses lazily, so the root's own body stays eager.
const DYNAMIC_FUNCTION_ROOT_START: u32 = 1;

/// Source bytes from which a classic script's functions compile on first call (`stubbing`).
/// Callgrind Ir per JetStream 3 iteration against compiling at load (no-jit, Nov 18), with the
/// compile cache unused / warm: babel-wtb (5.6 MB) first -16.7% / +4.3% (a warm load
/// compiled nothing), steady -10.1% / -9.8%. Scripts under a megabyte mostly run their code:
/// stubbed, chai-wtb (530 KB) went +0.3% / +8.4% first, +0.7% / +3.6% steady; postcss-wtb
/// (1.2 MB) +0.8% / +1.4% steady; espree and source-map within 2%. typescript (4.6 MB, a
/// compiler that runs most of its bundle) went +1.8% first, +4.8% steady at a 2 MiB threshold,
/// so the threshold stays above it until stubbing weighs how much of a script runs.
const FIRST_CALL_SCRIPT_BYTES: usize = 5 << 20;

/// Which functions of a source compile on first call.
#[derive(Clone, Copy)]
pub(super) struct Stubbing {
    pub(super) functions: bool,
    /// Arrow functions may compile on first call too (a direct eval's, and its chunks'), so the
    /// skim may take their bodies (`BodyFacts::skim`); otherwise they parse whole.
    pub(super) arrows: bool,
    /// The lazily parsed bodies are skimmed where the skim takes them (`BodyFacts::skim`), a
    /// body it declines parsing whole. Independent of `arrows`: a script skims its functions
    /// but keeps its arrows eager, and checks what the skim declines.
    pub(super) skim: bool,
}
