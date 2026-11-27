//! The source frontend's routes: every way the runtime compiles or reads source text after
//! load (eval, `Function`, a module or script it loads, a function compiled at first call or
//! again from its text, a call site's callee read from its text). `Vm::new` and the compiler's
//! own entries install them (`enable`); a binary that only resumes a runtime saved at rest
//! never names them unless its program can make code (`enable_dynamic_source`), so it links no
//! parser or compiler. Without them, source compilation is refused as Node refuses it under
//! `--disallow-code-generation-from-strings` (`Vm::require_source_compilation`).
use super::{
    AtomPrefix, Constant, Diagnostic, EvalRegExpLiteral, ParameterSite, ParsedProgramShape, ReleasedRecords,
    SpecializationMode, StaticModuleAnalysis, StaticModuleOutcome,
};
#[cfg(feature = "source")]
use super::Engine;
use crate::bytecode::{CallSiteDescription, ChunkDelta, ProgramSource, ResidualProgram, SourceSpan, SourceText};
use oxc_span::SourceType;

type Specialized = Result<ResidualProgram, Vec<Diagnostic>>;

/// The routes `enable` installs.
pub(crate) struct SourceRoutes {
    pub(crate) specialize: fn(&str, &str, SpecializationMode, &dyn AtomPrefix, SourceType, bool, bool, bool,
        &[(String, String)], &[String], ParsedProgramShape<'_>, ReleasedRecords, Option<SourceText>) -> Specialized,
    pub(crate) first_call_chunk: fn(&ResidualProgram, u32, SourceSpan, &dyn AtomPrefix, Option<&super::statement_stream::StreamRun>) -> Specialized,
    pub(crate) append_first_call_chunk: fn(&ResidualProgram, u32, &dyn AtomPrefix) -> Option<ChunkDelta>,
    pub(crate) recompile: fn(&ResidualProgram, u32) -> Result<super::replay::Recompiled, String>,
    pub(crate) eval_has_tagged_template: fn(&str) -> bool,
    pub(crate) eval_single_regexp_literal: fn(&str) -> Option<EvalRegExpLiteral>,
    pub(crate) class_eval_references_arguments: fn(&str) -> bool,
    pub(crate) analyze_static_module: fn(&str, &str, bool) -> StaticModuleAnalysis,
    pub(crate) static_module_outcome: fn(&str, &str) -> Option<StaticModuleOutcome>,
    pub(crate) static_module_exports: fn(&str, &str) -> Option<Vec<(String, Constant)>>,
    pub(crate) typescript_has_module_syntax: fn(&str) -> Option<bool>,
    pub(crate) typescript_parameter_sites: fn(&str) -> Option<Vec<ParameterSite>>,
    pub(crate) standalone_source_demands: fn(&str, crate::SourceKind) -> Option<super::SourceDemands>,
    /// The call sites of `program`, frozen once (`call_printer::freeze_call_sites`).
    pub(crate) freeze_call_sites: fn(&ResidualProgram),
    /// The call sites a throwing function's text gives before its source froze any.
    pub(crate) unfrozen_call_sites: fn(&ProgramSource, Option<SourceSpan>, u32) -> Option<Vec<CallSiteDescription>>,
}

static ROUTES: std::sync::OnceLock<SourceRoutes> = std::sync::OnceLock::new();

/// The installed routes, or `None` in a binary whose program makes no code.
#[inline]
pub(crate) fn routes() -> Option<&'static SourceRoutes> {
    ROUTES.get()
}

/// Installs the routes: the runtime may compile and read source from here on. An
/// execution-only build (no `source`) has no front end to route to: nothing installs.
#[cfg(not(feature = "source"))]
pub fn enable() {}

#[cfg(feature = "source")]
pub fn enable() {
    ROUTES.get_or_init(|| SourceRoutes {
        specialize: Engine::specialize_source,
        first_call_chunk: Engine::specialize_first_call_chunk_source,
        append_first_call_chunk: Engine::append_first_call_chunk_source,
        recompile: super::replay::recompile_source,
        eval_has_tagged_template: Engine::eval_has_tagged_template_source,
        eval_single_regexp_literal: Engine::eval_single_regexp_literal_source,
        class_eval_references_arguments: Engine::class_eval_references_arguments_source,
        analyze_static_module: Engine::analyze_static_module_source,
        static_module_outcome: Engine::static_module_outcome_source,
        static_module_exports: Engine::static_module_exports_source,
        typescript_has_module_syntax: Engine::typescript_has_module_syntax_source,
        typescript_parameter_sites: Engine::typescript_parameter_sites_source,
        standalone_source_demands: Engine::standalone_source_demands_parsed,
        freeze_call_sites: crate::vm::call_printer::freeze_call_sites,
        unfrozen_call_sites: super::call_sites::unfrozen_call_sites,
    });
}
