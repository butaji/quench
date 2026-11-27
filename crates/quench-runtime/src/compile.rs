use crate::bytecode::{
    Atom, Constant, DispatchClass, FieldBase, FieldSite, Function as BcFunction, FunctionSlot, Instr,
    LexicalBindingKind, MethodSite, ModuleImportBinding, ModuleImportName, ModuleLinkPlan,
    ModuleRequest, ModuleRequestPhase, ObjectSite, Op, Operand, REGISTER_MOVE, RETURN_OP, Register,
    ResidualProgram, SET_THIS_REGISTER, Superinstruction, Traits, WideInstruction, the_op,
};
use oxc_allocator::Allocator;
use replay::SiteTable;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::{fmt, ops::Range};
/// Source-shaping inputs shared by host Script preparation and native compilation.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostScriptOptions {
    pub origin: Option<(u32, u32)>,
    pub keep_records: bool,
    pub typescript: bool,
}

impl HostScriptOptions {
    /// Middle component of the host compile-cache/native-image key; source text
    /// and the specialization mode remain separate components.
    pub fn cache_goal(self) -> String {
        let (line, column) = self.origin.unwrap_or_default();
        format!("specialized-script:{line}:{column}:{}{}", self.keep_records,
            if self.typescript { ":typescript" } else { "" })
    }
}

/// The most registers a function's promoted locals may take: half of the register domain,
/// leaving the other half for the temporaries of any one statement.
const PROMOTED_LOCAL_REGISTER_BUDGET: Register = crate::bytecode::SET_THIS_REGISTER / 2;
/// The frame-local binding ops and their environment and plain forms, by role.
const LOCAL_LOAD_OP: Op = the_op(Traits::LOCAL_BINDING_LOAD);
const LOCAL_STORE_OP: Op = the_op(Traits::LOCAL_BINDING_STORE);
const ENV_LOAD_OP: Op = the_op(Traits::ENV_LOCAL_LOAD);
const ENV_STORE_OP: Op = the_op(Traits::ENV_LOCAL_STORE);
const PLAIN_LOAD_OP: Op = the_op(Traits::PLAIN_LOCAL_LOAD);
const PLAIN_STORE_OP: Op = the_op(Traits::PLAIN_LOCAL_STORE);

/// The plain form of a frame local's binding load or store, which is the op a plain slot's accesses take.
fn plain_local_form(op: Op) -> Option<Op> {
    if op == LOCAL_LOAD_OP {
        Some(PLAIN_LOAD_OP)
    } else if op == LOCAL_STORE_OP {
        Some(PLAIN_STORE_OP)
    } else {
        None
    }
}
pub mod aot_facts;
pub mod signatures;
mod annex_b_targets;
mod arrow;
mod ast;
mod atoms;
mod native_sources;
pub use native_sources::{NativeJsCompiler, NativeSourceKey, NativeSourceCompilation, SourceDemands};
/// The parse goal shared by module compile-cache and linked native source keys.
pub const STATIC_MODULE_GOAL: &str = "static-module";
pub const STATIC_TYPESCRIPT_MODULE_GOAL: &str = "static-module:typescript";
/// Global Script identity is independent of its invocation's diagnostic name.
pub const GLOBAL_SCRIPT_GOAL: &str = "global-script";
pub const GLOBAL_SCRIPT_SOURCE_NAME: &str = "<global-script>";
mod binding_time;
pub(crate) mod data_literal;
mod capture_layout;
#[cfg(feature = "profile-memory")]
mod capture_profile;
mod call_only;
mod capture_scope;
mod class; mod class_private;
mod early;
mod frame_registers;
pub(crate) mod lazy_bodies;
mod literal_singletons;
mod semantic_stats;
pub(crate) use lazy_bodies::{expire_warm_arena, release_warm_arena};
#[cfg(feature = "verify-replay")]
mod free_names;
mod syntax_message;
pub(crate) mod source_syntax;
pub mod liveness;
mod locals;
mod numeric;
pub(crate) mod first_call;
mod program_shape;
pub(crate) use program_shape::EvalContext;
use program_shape::ParsedProgramShape;
mod object_registers;
mod rest_reads;
pub(crate) mod regexp; mod regexp_memo; mod source_text;
pub(crate) mod program_profile;
#[cfg(feature = "profile-memory")]
mod register_profile;
pub(crate) mod replay;
pub mod source_routes; pub(crate) mod call_sites;
mod settled;
mod deferred_rewrite; mod function_table; pub(crate) use deferred_rewrite::{FIELD_SITE_WINDOW, SUPERINSTRUCTION_WINDOW, settle_deferred_rewrite};
mod rewrite; mod rest_alias;
mod scratch;
pub(crate) use scratch::{CompileScratch, code_capacity_hint, trim_slack};
mod sequence;
pub(crate) mod stack;
mod checks;
pub(crate) mod statement_stream;
mod string;
mod template;
mod typescript;
pub use typescript::{CheckedParameters, ParameterSite};
use ast::{FunctionCompiler, StatementCompletion};
use atoms::CompilerAtoms;
pub(crate) use atoms::{AtomPrefix, NoAtomPrefix};

/// The synthetic method supplies parsing context only; execution uses its body.
fn eval_method_body<'a, 'b>(
    program: &'b mut Program<'a>,
    ordinary_function: bool,
) -> Option<&'b mut oxc_ast::ast::FunctionBody<'a>> {
    let [Statement::ExpressionStatement(statement)] = program.body.as_mut_slice() else {
        return None;
    };
    let function = match statement.expression.without_parentheses_mut() {
        Expression::ClassExpression(class) => {
            let ClassElement::MethodDefinition(method) = class.body.body.last_mut()? else {
                return None;
            };
            &mut method.value
        }
        Expression::ObjectExpression(object) => {
            let [ObjectPropertyKind::ObjectProperty(property)] = object.properties.as_mut_slice()
            else {
                return None;
            };
            let Expression::FunctionExpression(function) = &mut property.value else {
                return None;
            };
            function
        }
        _ => return None,
    };
    let body = function.body.as_deref_mut()?;
    if !ordinary_function { return Some(body); }
    let [Statement::ExpressionStatement(statement)] = body.statements.as_mut_slice() else {
        return None;
    };
    let Expression::FunctionExpression(function) = statement.expression.without_parentheses_mut() else {
        return None;
    };
    function.body.as_deref_mut()
}

pub(crate) fn eval_context_source(
    source: &str,
    private_names: &[(String, String)],
    super_calls: bool,
) -> String {
    let normalized = early::normalize_hashbang(source);
    let source = normalized.as_ref();
    let declarations = private_names
        .iter()
        .map(|(label, _)| format!("#{label};"))
        .collect::<Vec<_>>()
        .join("\n");
    let heritage = if super_calls { " extends null" } else { "" };
    let method = if super_calls { "constructor" } else { "__eval" };
    format!("(class{heritage} {{\n{declarations}\n{method}() {{\n{source}\n}}\n}})")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagnosticKind {
    Syntax,
    Compilation,
    StackExhausted,
}
#[derive(Clone)]
pub struct Diagnostic {
    kind: DiagnosticKind,
    source: String,
    message: String,
    span: Span,
}
impl Diagnostic {
    fn message(&self) -> &str {
        match self.kind {
            DiagnosticKind::Syntax | DiagnosticKind::Compilation => &self.message,
            DiagnosticKind::StackExhausted => crate::stack::STACK_EXHAUSTED_MESSAGE,
        }
    }

    /// Whether parsing or ECMAScript early-error validation rejected the source.
    /// Backend limits and unsupported syntax are separate compilation failures.
    pub fn is_syntax_error(&self) -> bool {
        self.kind == DiagnosticKind::Syntax
    }

    /// The UTF-8 offset in the source the diagnostic points at.
    pub fn offset(&self) -> u32 {
        self.span.start
    }

    /// The diagnostic message without a leading SyntaxError class name.
    pub fn syntax_message(&self) -> &str {
        let message = self.message();
        message.strip_prefix(SYNTAX_ERROR_PREFIX).unwrap_or(message)
    }

    /// A parse failure, reported with V8's message at OXC's position.
    fn parse_failure(source_name: &str, source: &str, error: &oxc_diagnostics::OxcDiagnostic) -> Self {
        let offset = error.labels.first().map_or(0, |label| label.offset());
        Self {
            kind: DiagnosticKind::Syntax,
            source: source_name.into(),
            message: syntax_message::parse_failure(source, error),
            span: Span::new(offset, offset),
        }
    }

    pub(crate) fn is_stack_exhausted(&self) -> bool {
        self.kind == DiagnosticKind::StackExhausted
    }

    fn stack_exhausted(source: &str) -> Self {
        Self {
            kind: DiagnosticKind::StackExhausted,
            source: source.into(),
            message: String::new(),
            span: Span::default(),
        }
    }

    pub(crate) fn unsupported(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticKind::Compilation,
            source: source.into(),
            message: message.into(),
            span: Span::default(),
        }
    }
}
impl fmt::Debug for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Diagnostic")
            .field("source", &self.source)
            .field("message", &self.message())
            .field("span", &self.span)
            .finish()
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}..{}: {}",
            self.source,
            self.span.start,
            self.span.end,
            self.message()
        )
    }
}
pub struct Engine;

/// The class name some compiler messages lead with.
const SYNTAX_ERROR_PREFIX: &str = "SyntaxError: ";

/// V8's report for a `return` at the top level of an eval.
const RETURN_OUTSIDE_FUNCTION_IN_EVAL: &str = "SyntaxError: return outside function in eval";

#[cfg(test)]
thread_local! {
    /// The lazily parsed bodies compiled from their text parsed again (`compile_parsed_again`), counted
    /// by the tests that check a program was compiled lazily.
    pub(crate) static REPARSED_BODIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// The rejection for a lazily parsed function body that fails when compiled alone.
const LAZY_FUNCTION_BODY_NOT_ALONE: &str = "SyntaxError: a lazily parsed function body does not compile alone";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicFunctionKind {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

#[cfg(feature = "native-execution")]
pub(crate) const DYNAMIC_FUNCTION_GOAL: &str = "dynamic-function";

pub(crate) const DYNAMIC_FUNCTION_PUBLIC_NAME: &str = "anonymous";

/// The name of every Function-constructor program. Its frames report their origin, not this
/// name (`CompilationOrigin::FunctionConstructor`), so one name serves all of them and keys
/// their compiled form by text alone.
pub(crate) const DYNAMIC_FUNCTION_SOURCE_NAME: &str = "<Function>";

/// A Function-constructor program's source: the coerced parameter and body strings,
/// normalized, in the expression the compiler parses (`(function anonymous(…\n) {\n…\n})`),
/// and where its parameter list ends.
pub(crate) struct DynamicFunctionSource {
    pub(crate) text: String,
    parameter_list_end: usize,
}

impl DynamicFunctionSource {
    pub(crate) fn new(parameters: &str, body: &str, kind: DynamicFunctionKind) -> Self {
        let body = early::normalize_dynamic_function_body(body);
        let parameters = early::normalize_dynamic_function_parameters(parameters);
        const EXPRESSION_OPEN: &str = "(";
        const EXPRESSION_CLOSE: &str = ")";
        const PARAMETER_LIST_END: &str = "\n)";
        let function_prefix = format!("{} {DYNAMIC_FUNCTION_PUBLIC_NAME}(", kind.source_prefix());
        Self {
            text: format!("{EXPRESSION_OPEN}{}{EXPRESSION_CLOSE}", kind.source(&parameters, &body)),
            parameter_list_end: EXPRESSION_OPEN.len()
                + function_prefix.len()
                + parameters.len()
                + PARAMETER_LIST_END.len(),
        }
    }
}

impl DynamicFunctionKind {
    const ALL: [Self; 4] = [Self::Ordinary, Self::Async, Self::Generator, Self::AsyncGenerator];

    pub(crate) fn from_source_prefix(prefix: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.source_prefix() == prefix)
    }

    #[cfg(feature = "native-execution")]
    pub(crate) fn native_parts<'a>(self, mode: &'a str, parameters: &'a str, body: &'a str) -> [&'a str; 5] {
        [mode, DYNAMIC_FUNCTION_GOAL, self.source_prefix(), parameters, body]
    }

    fn source_prefix(self) -> &'static str {
        match self {
            Self::Ordinary => "function",
            Self::Async => "async function",
            Self::Generator => "function*",
            Self::AsyncGenerator => "async function*",
        }
    }

    pub(crate) fn source(self, parameters: &str, body: &str) -> String {
        format!("{} {DYNAMIC_FUNCTION_PUBLIC_NAME}({parameters}\n) {{\n{body}\n}}", self.source_prefix())
    }
}

pub(crate) struct EvalRegExpLiteral {
    pub(crate) span: Range<usize>,
    pub(crate) flags: String,
}

/// The first characters, past white space, `/*…*/` and `//` comments and opening
/// parentheses, after which a Script of one parenthesized regular expression literal may
/// still begin: the literal's own `/`, and the starts of the comments this scan leaves to
/// the parser (Annex B's `<!--` and `-->`, and a hashbang `#!`).
const SINGLE_REGEXP_LITERAL_STARTS: [u8; 4] = *b"/<-#";

/// Whether `source` may be a Script of one (possibly parenthesized) regular expression
/// literal, which `Engine::eval_single_regexp_literal` then parses to decide: false only when
/// the first token past white space, ordinary comments and `(` is something else. Skipping
/// Rust's white space, which is ECMAScript's plus U+0085 and less U+FEFF, as well as U+FEFF,
/// keeps the test an over-approximation: it rejects no source the parse would accept.
fn may_begin_single_regexp_literal(source: &str) -> bool {
    let mut rest = source;
    loop {
        rest = rest.trim_start_matches(|character: char| character.is_whitespace() || character == '\u{FEFF}');
        if let Some(after) = rest.strip_prefix("/*") {
            let Some(end) = after.find("*/") else { return true };
            rest = &after[end + "*/".len()..];
        } else if let Some(after) = rest.strip_prefix("//") {
            let Some(end) = after.find(['\n', '\r', '\u{2028}', '\u{2029}']) else { return true };
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('(') {
            rest = after;
        } else {
            return rest.bytes().next().is_some_and(|first| SINGLE_REGEXP_LITERAL_STARTS.contains(&first));
        }
    }
}
pub(crate) enum StaticModuleThrow {
    Value(Constant),
    Error {
        name: String,
        message: Option<Constant>,
    },
}

/// What a static module's text evaluates to without running it (`Engine::static_module_outcome`).
pub(crate) enum StaticModuleOutcome {
    Exports(Vec<(String, Constant)>),
    Throw(StaticModuleThrow),
}
#[derive(Clone, Debug, Default)]
pub(crate) struct StaticModulePlan {
    pub(crate) link_plan: Option<ModuleLinkPlan>,
    pub(crate) requests: Vec<ModuleRequest>,
    pub(crate) has_top_level_await: bool,
}
pub(crate) use crate::bytecode::ModuleReexport as StaticModuleReexport;

/// `Engine::static_module_analysis`: a static module text's early error, or else its plan
/// (none when the stack does not bound its syntax).
pub(crate) struct StaticModuleAnalysis {
    pub(crate) early_error: Option<String>,
    pub(crate) plan: Option<StaticModulePlan>,
}

/// The key `static_module_analysis` memoizes a text under: the module's name, the text's
/// length and its hash. Texts are the process's own module files, not adversarial input, and
/// a module is looked up about fifteen times, so one fast hash of its text serves.
#[derive(PartialEq, Eq, Hash)]
struct StaticModuleTextKey {
    length: usize,
    hash: u64,
    name: Box<str>,
    typescript: bool,
}

impl StaticModuleTextKey {
    fn of(source: &str, module_name: &str, typescript: bool) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        source.hash(&mut hasher);
        Self { length: source.len(), hash: hasher.finish(), name: module_name.into(), typescript }
    }
}
impl Engine {
    /// V8 excludes tagged templates from eval's compilation cache because each
    /// evaluation owns distinct template objects, including nested function sites.
    pub(crate) fn eval_has_tagged_template(source: &str) -> bool {
        source_routes::routes().map_or(false, |routes| (routes.eval_has_tagged_template)(source))
    }

    pub(crate) fn eval_has_tagged_template_source(source: &str) -> bool {
        use oxc_ast_visit::Visit;
        struct TaggedTemplate(bool);
        impl<'a> Visit<'a> for TaggedTemplate {
            fn visit_tagged_template_expression(&mut self, _: &TaggedTemplateExpression<'a>) {
                self.0 = true;
            }
        }
        let allocator = Allocator::with_capacity(source.len());
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() { return true; }
        let mut visitor = TaggedTemplate(false);
        visitor.visit_program(&parsed.program);
        visitor.0
    }

    pub(crate) fn eval_single_regexp_literal(source: &str) -> Option<EvalRegExpLiteral> {
        source_routes::routes().map_or(None, |routes| (routes.eval_single_regexp_literal)(source))
    }

    pub(crate) fn eval_single_regexp_literal_source(source: &str) -> Option<EvalRegExpLiteral> {
        if !may_begin_single_regexp_literal(source) {
            return None;
        }
        let allocator = Allocator::with_capacity(source.len());
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if stack::validate_parsed(&parsed).is_err() {
            return None;
        }
        if !parsed.diagnostics.is_empty() || parsed.program.body.len() != 1 {
            return None;
        }
        let Statement::ExpressionStatement(statement) = &parsed.program.body[0] else {
            return None;
        };
        let Expression::RegExpLiteral(literal) = statement.expression.without_parentheses() else {
            return None;
        };
        let flags = [
            ('d', RegExpFlags::D),
            ('g', RegExpFlags::G),
            ('i', RegExpFlags::I),
            ('m', RegExpFlags::M),
            ('s', RegExpFlags::S),
            ('u', RegExpFlags::U),
            ('v', RegExpFlags::V),
            ('y', RegExpFlags::Y),
        ]
        .into_iter()
        .filter_map(|(flag, bit)| literal.regex.flags.contains(bit).then_some(flag))
        .collect();
        Some(EvalRegExpLiteral {
            span: literal.span.start as usize..literal.span.end as usize,
            flags,
        })
    }

    pub(crate) fn class_eval_references_arguments(source: &str) -> bool {
        source_routes::routes().map_or(false, |routes| (routes.class_eval_references_arguments)(source))
    }

    pub(crate) fn class_eval_references_arguments_source(source: &str) -> bool {
        let context = eval_context_source(source, &[], true);
        let allocator = Allocator::with_capacity(context.len());
        let mut parsed = annex_b_targets::parse_program(&allocator, &context, SourceType::script());
        if stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() {
            return false; // The normal eval compiler owns syntax and exhaustion errors.
        }
        eval_method_body(&mut parsed.program, false)
            .is_some_and(|body| early::class_eval_references_arguments(&body.statements))
    }

    /// The SyntaxError message of a module source's first early error, if it has one.
    /// A static module's link plan, when its text has no early error and the stack bounds
    /// its syntax (`static_module_analysis`).
    pub(crate) fn static_module_plan(source: &str, module_name: &str) -> Option<StaticModulePlan> {
        Self::static_module_analysis(source, module_name).plan.clone()
    }

    /// What a static module's text says before it links, from one parse and semantic check,
    /// memoized for the thread by the text and name: linking and evaluating a module asks for
    /// it several times (a module graph parsed each module three times, 12% of a Babel run).
    pub(crate) fn static_module_analysis(source: &str, module_name: &str) -> Rc<StaticModuleAnalysis> {
        Self::static_module_analysis_with(source, module_name, false, |analyze| analyze())
    }

    /// Whether `text`, a compiled module's kept text, is `source` as the compiler reads it: as
    /// given, or with its hashbang blanked (`early::normalize_hashbang`).
    pub(crate) fn compiled_module_text_is(text: &str, source: &str) -> bool {
        text == source || text == &*early::normalize_hashbang(source)
    }

    /// `static_module_analysis`, whose memo misses `obtain`, which analyzes the text with the
    /// function it is given or finds the analysis elsewhere (a host's compile cache).
    pub(crate) fn static_module_analysis_with(
        source: &str,
        module_name: &str,
        typescript: bool,
        obtain: impl FnOnce(&dyn Fn() -> StaticModuleAnalysis) -> StaticModuleAnalysis,
    ) -> Rc<StaticModuleAnalysis> {
        thread_local! {
            static ANALYSES: std::cell::RefCell<FxHashMap<StaticModuleTextKey, Rc<StaticModuleAnalysis>>> =
                std::cell::RefCell::default();
        }
        let key = StaticModuleTextKey::of(source, module_name, typescript);
        if let Some(analysis) = ANALYSES.with(|analyses| analyses.borrow().get(&key).cloned()) {
            return analysis;
        }
        let analysis = Rc::new(obtain(&|| Self::analyze_static_module(source, module_name, typescript)));
        ANALYSES.with(|analyses| analyses.borrow_mut().insert(key, analysis.clone()));
        analysis
    }

    fn analyze_static_module(source: &str, module_name: &str, typescript: bool) -> StaticModuleAnalysis {
        source_routes::routes().map_or(StaticModuleAnalysis { early_error: None, plan: None }, |routes| (routes.analyze_static_module)(source, module_name, typescript))
    }

    pub(crate) fn analyze_static_module_source(source: &str, module_name: &str, typescript: bool) -> StaticModuleAnalysis {
        let unbounded = StaticModuleAnalysis { early_error: None, plan: None };
        // No source-derived proof is available in execution-only support.
        // Archived analysis may still satisfy static_module_analysis_with;
        // otherwise execution must use linked code or fail its admission guard.
        if !crate::DYNAMIC_EXECUTION_ENABLED { return unbounded; }
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let mut parsed = Parser::new(&allocator, &normalized, SourceType::mjs().with_typescript(typescript)).parse();
        if stack::validate_parsed(&parsed).is_err() {
            return unbounded;
        }
        if let Some(error) = parsed.diagnostics.first() {
            return StaticModuleAnalysis {
                early_error: Some(syntax_message::parse_failure(&normalized, error)),
                plan: None,
            };
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if let Some(error) = semantic.diagnostics.first() {
            return StaticModuleAnalysis { early_error: Some(error.message.to_string()), plan: None };
        }
        let type_only_imports = typescript::type_only_imports(&parsed.program, semantic.semantic.scoping());
        drop(semantic);
        Self::admit_typescript(&allocator, &mut parsed.program);
        typescript::elide_type_only_imports(&mut parsed.program, &type_only_imports, &mut crate::bytecode::DeclaredTypes::default());
        StaticModuleAnalysis {
            early_error: None,
            plan: Some(StaticModulePlan {
                link_plan: Self::module_link_plan(&parsed.program.body, module_name),
                requests: module_requests(&parsed.program.body),
                has_top_level_await: has_top_level_await(&parsed.program.body),
            }),
        }
    }

    /// Admit a typed tree (`typescript::admit`): its declared environment kept apart from
    /// code generation, its run-time forms (enums, namespaces, parameter properties) in
    /// place. Every compile parse admits through `lazy_bodies::parse`; a host that reads
    /// module structure from its own parse admits here.
    pub fn admit_typescript<'a>(allocator: &'a Allocator, program: &mut Program<'a>) {
        if program.source_type.is_typescript() { typescript::admit(allocator, program, &mut crate::bytecode::DeclaredTypes::default()); }
    }

    /// Whether typed text keeps module syntax once admitted (type-only imports and exports
    /// are declarations, not module syntax): a `.ts` file's goal when no package scope
    /// declares one. None when it does not parse.
    pub fn typescript_has_module_syntax(source: &str) -> Option<bool> {
        source_routes::routes().map_or(None, |routes| (routes.typescript_has_module_syntax)(source))
    }

    pub(crate) fn typescript_has_module_syntax_source(source: &str) -> Option<bool> {
        if !crate::DYNAMIC_EXECUTION_ENABLED { return None; }
        let allocator = Allocator::with_capacity(source.len().saturating_mul(4));
        let mut parsed = Parser::new(&allocator, source, SourceType::unambiguous().with_typescript(true))
            .with_options(oxc_parser::ParseOptions { allow_return_outside_function: true, ..Default::default() }).parse();
        if !parsed.diagnostics.is_empty() || stack::validate_parsed(&parsed).is_err() { return None; }
        Self::admit_typescript(&allocator, &mut parsed.program);
        Some(parsed.program.body.iter().any(Statement::is_module_declaration))
    }

    /// The functions of TypeScript `source` a checker can give parameter types for, as UTF-8
    /// offsets into `source`; `None` when it does not parse.
    pub fn typescript_parameter_sites(source: &str) -> Option<Vec<typescript::ParameterSite>> {
        source_routes::routes().map_or(None, |routes| (routes.typescript_parameter_sites)(source))
    }

    pub(crate) fn typescript_parameter_sites_source(source: &str) -> Option<Vec<typescript::ParameterSite>> {
        // Without the front end no source is parsed (a static image's support).
        if !cfg!(feature = "source") {
            return None;
        }
        let allocator = Allocator::with_capacity(source.len().saturating_mul(4));
        let parsed = Parser::new(&allocator, source, SourceType::unambiguous().with_typescript(true))
            .with_options(oxc_parser::ParseOptions { allow_return_outside_function: true, ..Default::default() }).parse();
        parsed.diagnostics.is_empty().then(|| typescript::parameter_sites(&parsed.program))
    }

    /// Install a checker's parameter types for `text`, the exact source a later compile
    /// receives; they narrow its declared parameter facts (`typescript::checked`).
    pub fn register_checked_typescript_parameters(text: &str, checked: typescript::CheckedParameters) {
        typescript::register_checked_parameters(text, checked);
    }

    /// Runtime requests of an admitted tree (type-only module syntax is declarations).
    pub fn module_requests(program: &Program<'_>) -> Vec<ModuleRequest> {
        module_requests(&program.body)
    }

    fn module_link_plan(statements: &[Statement<'_>], module_name: &str) -> Option<ModuleLinkPlan> {
        let mut plan = ModuleLinkPlan {
            locals: Vec::new(),
            reexports: Vec::new(),
            hoisted_functions: module_hoisted_functions(statements, module_name),
        };
        for statement in statements {
            append_module_link_statement(statement, module_name, &mut plan)?;
        }
        let imports = module_import_bindings(statements)
            .into_iter()
            .map(|binding| (binding.local.clone(), binding))
            .collect::<FxHashMap<_, _>>();
        let mut locals = Vec::with_capacity(plan.locals.len());
        for (local, exported) in plan.locals {
            let Some(import) = imports.get(&local) else {
                locals.push((local, exported));
                continue;
            };
            if matches!(
                import.phase,
                ModuleRequestPhase::Source | ModuleRequestPhase::Defer
            ) || import.module_type.is_some() {
                locals.push((local, exported));
                continue;
            }
            if import.phase != ModuleRequestPhase::Evaluation {
                return None;
            }
            plan.reexports.push(match &import.imported {
                ModuleImportName::Namespace => StaticModuleReexport::Namespace {
                    source: import.source.clone(),
                    exported,
                },
                ModuleImportName::Named(imported) => StaticModuleReexport::Named {
                    source: import.source.clone(),
                    imported: imported.clone(),
                    exported,
                },
            });
        }
        plan.locals = locals;
        Some(plan)
    }

    /// `read` of the program of a static module's text, parsed and checked as a module, or None
    /// where it does not parse or check. One parse serves every reading of the text
    /// (`static_module_outcome`).
    fn with_static_module<R>(source: &str, read: impl FnOnce(&Program<'_>) -> Option<R>) -> Option<R> {
        if !crate::DYNAMIC_EXECUTION_ENABLED { return None; }
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, SourceType::mjs()).parse();
        if stack::validate_parsed(&parsed).is_err() {
            return None;
        }
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if !semantic.diagnostics.is_empty() {
            return None;
        }
        read(&parsed.program)
    }

    /// What a static module's text evaluates to without running it: its exports, else the value
    /// its program throws.
    pub(crate) fn static_module_outcome(source: &str, module_name: &str) -> Option<StaticModuleOutcome> {
        source_routes::routes().map_or(None, |routes| (routes.static_module_outcome)(source, module_name))
    }

    pub(crate) fn static_module_outcome_source(source: &str, module_name: &str) -> Option<StaticModuleOutcome> {
        Self::with_static_module(source, |program| {
            Self::static_module_exports_of(program, module_name)
                .map(StaticModuleOutcome::Exports)
                .or_else(|| Self::static_module_throw_of(program).map(StaticModuleOutcome::Throw))
        })
    }

    /// The value a static module's program throws, when its only statement besides empty ones is
    /// a throw (`static_module_outcome`).
    fn static_module_throw_of(program: &Program<'_>) -> Option<StaticModuleThrow> {
        let mut thrown = None;
        for statement in &program.body {
            match statement {
                Statement::EmptyStatement(_) => {}
                Statement::ThrowStatement(statement) if thrown.is_none() => {
                    let value = match &statement.argument {
                        Expression::NewExpression(expression) => {
                            let Expression::Identifier(identifier) = &expression.callee else {
                                return None;
                            };
                            let name = match identifier.name.as_str() {
                                "Error" | "EvalError" | "RangeError" | "ReferenceError"
                                | "SyntaxError" | "TypeError" | "URIError" => {
                                    identifier.name.to_string()
                                }
                                _ => return None,
                            };
                            let message = match expression.arguments.as_slice() {
                                [] => None,
                                [oxc_ast::ast::Argument::SpreadElement(_)] => return None,
                                [argument] => Some(binding_time::expression(argument.as_expression()?)?),
                                _ => return None,
                            };
                            StaticModuleThrow::Error { name, message }
                        }
                        expression => StaticModuleThrow::Value(binding_time::expression(expression)?),
                    };
                    thrown = Some(value);
                }
                _ => return None,
            }
        }
        thrown
    }

    pub(crate) fn static_module_exports(
        source: &str,
        module_name: &str,
    ) -> Option<Vec<(String, Constant)>> {
        source_routes::routes().and_then(|routes| (routes.static_module_exports)(source, module_name))
    }

    pub(crate) fn static_module_exports_source(
        source: &str,
        module_name: &str,
    ) -> Option<Vec<(String, Constant)>> {
        Self::with_static_module(source, |program| Self::static_module_exports_of(program, module_name))
    }

    /// The exports of a static module's program, when it only declares them
    /// (`static_module_outcome`).
    fn static_module_exports_of(program: &Program<'_>, module_name: &str) -> Option<Vec<(String, Constant)>> {
        let mut bindings = FxHashMap::default();
        let mut exports = Vec::new();
        for statement in &program.body {
            match statement {
                Statement::EmptyStatement(_) => {}
                Statement::VariableDeclaration(declaration) => {
                    static_module_bindings(declaration, &mut bindings)?;
                }
                Statement::ExportDeclaration(export) => {
                    let Declaration::VariableDeclaration(declaration) = &export.declaration else {
                        return None;
                    };
                    let names = static_module_bindings(declaration, &mut bindings)?;
                    exports.extend(names.into_iter().map(|name| (name.clone(), name)));
                }
                Statement::ExportNamedDeclaration(export) => {
                    for specifier in &export.specifiers {
                        exports.push((
                            module_export_name(&specifier.local),
                            module_export_name(&specifier.exported),
                        ));
                    }
                }
                Statement::ExportFromDeclaration(export)
                    if same_module_path(module_name, export.source.value.as_str()) =>
                {
                    for specifier in &export.specifiers {
                        exports.push((
                            module_export_name(&specifier.local),
                            module_export_name(&specifier.exported),
                        ));
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let expression = export.declaration.as_expression()?;
                    let value = match expression {
                        Expression::Identifier(identifier) => {
                            bindings.get(identifier.name.as_str())?.clone()
                        }
                        expression => binding_time::expression(expression)?,
                    };
                    exports.push(("\0quench:module-default".into(), "default".into()));
                    bindings.insert("\0quench:module-default".into(), value);
                }
                _ => return None,
            }
        }
        let mut resolved = Vec::with_capacity(exports.len());
        for (local, exported) in exports {
            resolved.push((exported, bindings.get(&local)?.clone()));
        }
        resolved.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
        Some(resolved)
    }

    pub(crate) fn module_export_names(
        source: &str,
        module_name: &str,
    ) -> Option<Vec<(String, String)>> {
        let plan = Self::static_module_plan(source, module_name)?;
        let link_plan = plan.link_plan?;
        link_plan.reexports.is_empty().then_some(link_plan.locals)
    }

    /// Whether `source` parses as `source_type` with no syntax error OXC reports, holding
    /// one function's tree at a time (`lazy_bodies::syntax_valid`).
    pub fn syntax_valid(source: &str, source_type: SourceType) -> bool {
        lazy_bodies::syntax_valid(source, source_type)
    }

    /// Parse TypeScript directly: annotations remain available to analysis and
    /// replay. They do not establish runtime representation invariants.
    pub fn specialize_typescript(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_with_mode(
            source, name, SpecializationMode::Enabled, &NoAtomPrefix,
            SourceType::script().with_typescript(true), false, false, false,
        )
    }

    /// A host script's program. Where `first_call`, the host runs it from its kept form,
    /// which records the room its first-call chunks append in, so a classic script's
    /// functions may compile on first call (`ParsedProgramShape::FirstCallScript`).
    pub(crate) fn specialize_host_script(
        source: &str, name: &str, prefix: &dyn AtomPrefix,
        options: HostScriptOptions, released: ReleasedRecords, first_call: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_host_script_provided(source, name, prefix, options, released, first_call, None)
    }

    /// `specialize_host_script` for a host text the caller holds in shared storage: `provided`
    /// is that storage, which the program keeps in place of a copy of `source` (`kept_or_provided`).
    pub(crate) fn specialize_host_script_provided(
        source: &str, name: &str, prefix: &dyn AtomPrefix,
        options: HostScriptOptions, released: ReleasedRecords, first_call: bool,
        provided: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        // The routes of the Script, typescript and first-call forms; each one's shape and
        // released records are those its own helper gave.
        let (shape, released, source_type) = if options.typescript {
            let shape = if options.keep_records { ParsedProgramShape::KeptRecords } else { ParsedProgramShape::Any };
            (shape, released, SourceType::script().with_typescript(true))
        } else if options.keep_records {
            (ParsedProgramShape::KeptRecords, ReleasedRecords::Dropped, SourceType::script())
        } else if first_call {
            (ParsedProgramShape::FirstCallScript, released, SourceType::script())
        } else {
            (ParsedProgramShape::Any, released, SourceType::script())
        };
        Self::specialize_script_shaped(source, name, prefix, shape, released, source_type, provided)
    }

    pub fn specialize(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Enabled,
            &NoAtomPrefix,
            SourceType::script(),
            false,
            false,
            false,
        )
    }
    /// Compile a host Script against the VM's atom prefix.
    pub(crate) fn specialize_script_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_script_shaped(source, name, atom_prefix, ParsedProgramShape::Any, released, SourceType::script(), None)
    }

    fn specialize_script_shaped(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        shape: ParsedProgramShape<'_>,
        released: ReleasedRecords,
        source_type: SourceType,
        provided_text: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            source_type,
            false,
            true,
            false,
            &[],
            &[],
            shape,
            released,
            provided_text,
        )
    }
    pub fn specialize_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Disabled,
            &NoAtomPrefix,
            SourceType::script(),
            false,
            false,
            false,
        )
    }
    pub(crate) fn specialize_eval_with_context(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        inherited_strict: bool,
        context: EvalContext,
        private_names: &[(String, String)],
        annex_b_forbidden_names: &[String],
        provided_text: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let normalized = early::normalize_hashbang(source);
        let source = normalized.as_ref();
        let method_source;
        let raw_source_len = source.len();
        let ordinary_private_context = context.home_atom.is_none()
            && context.in_function && !private_names.is_empty();
        let (source, source_type) = if context.home_atom.is_some() || ordinary_private_context {
            method_source = if private_names.is_empty() && !context.super_calls {
                let directive = if inherited_strict {
                    "'use strict';\n"
                } else {
                    ""
                };
                format!("{directive}({{__eval() {{\n{source}\n}}}})")
            } else {
                let ordinary_source;
                let source = if ordinary_private_context {
                    ordinary_source = format!("(function() {{\n{source}\n}});");
                    ordinary_source.as_str()
                } else { source };
                eval_context_source(source, private_names, context.super_calls)
            };
            (method_source.as_str(), SourceType::script())
        } else {
            (
                source,
                if context.in_function {
                    SourceType::cjs()
                } else {
                    SourceType::script()
                },
            )
        };
        let body_span = if context.home_atom.is_some() || ordinary_private_context {
            let suffix = if ordinary_private_context { "\n});\n}\n})" }
                else if private_names.is_empty() && !context.super_calls { "\n}})" }
                else { "\n}\n})" };
            let body_start = source.len() - suffix.len() - raw_source_len - "{\n".len();
            Some(Span::new(body_start as u32, (body_start + "{\n".len() + raw_source_len + "\n}".len()) as u32))
        } else { None };
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            source_type,
            false,
            true,
            inherited_strict,
            private_names,
            annex_b_forbidden_names,
            ParsedProgramShape::Eval(context, body_span),
            ReleasedRecords::Dropped,
            provided_text,
        )
    }

    pub fn specialize_module_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_module_with_mode(source, name, SpecializationMode::Disabled)
    }
    /// Compile a module against an existing program's canonical atom prefix, specializing
    /// exactly when the VM it extends does, as a run-time script does.
    pub(crate) fn specialize_module_with_atom_prefix(
        source: &str,
        name: &str,
        typescript: bool,
        atom_prefix: &dyn AtomPrefix,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            SourceType::mjs().with_typescript(typescript),
            true,
            false,
            false,
            &[],
            &[],
            ParsedProgramShape::Any,
            released,
            None,
        )
    }
    /// Compile a TypeScript module: the typed tree is the program, as for `.ts`/`.mts` files.
    pub fn specialize_typescript_module(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_module_with_atom_prefix(source, name, true, &NoAtomPrefix, ReleasedRecords::Dropped)
    }
    pub fn specialize_module(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        source_routes::enable();
        Self::specialize_module_with_mode(source, name, SpecializationMode::Enabled)
    }
    /// Compile a Function-constructor program's source through the same OXC script pipeline
    /// used by ordinary source.
    pub(crate) fn specialize_function_constructor(
        source: &DynamicFunctionSource,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        eager: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let expected_shape = ParsedProgramShape::DynamicFunction {
            parameter_list_end: source.parameter_list_end,
            eager,
        };
        Self::specialize_with_program_shape(
            &source.text,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            SourceType::script(),
            false,
            false,
            false,
            expected_shape,
        )
    }
    fn specialize_module_with_mode(
        source: &str,
        name: &str,
        mode: SpecializationMode,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(
            source,
            name,
            mode,
            &NoAtomPrefix,
            SourceType::mjs(),
            true,
            false,
            false,
        )
    }
    fn specialize_with_mode(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &dyn AtomPrefix,
        source_type: SourceType,
        module_goal: bool,
        capture_script_completion: bool,
        inherited_strict: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_program_shape(
            source,
            name,
            mode,
            atom_prefix,
            source_type,
            module_goal,
            capture_script_completion,
            inherited_strict,
            ParsedProgramShape::Any,
        )
    }
    fn specialize_with_program_shape(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &dyn AtomPrefix,
        source_type: SourceType,
        module_goal: bool,
        capture_script_completion: bool,
        inherited_strict: bool,
        expected_shape: ParsedProgramShape<'_>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            mode,
            atom_prefix,
            source_type,
            module_goal,
            capture_script_completion,
            inherited_strict,
            &[],
            &[],
            expected_shape,
            ReleasedRecords::Dropped,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    /// Without the source routes (`source_routes`) nothing compiles: the callers refused it first
    /// (`Vm::require_source_compilation`).
    fn specialize_with_mode_and_private_names(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &dyn AtomPrefix,
        source_type: SourceType,
        module_goal: bool,
        capture_script_completion: bool,
        inherited_strict: bool,
        private_name_overrides: &[(String, String)],
        annex_b_forbidden_names: &[String],
        expected_shape: ParsedProgramShape<'_>,
        released: ReleasedRecords,
        provided_text: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        match source_routes::routes() {
            Some(routes) => (routes.specialize)(source, name, mode, atom_prefix, source_type, module_goal, capture_script_completion,
                inherited_strict, private_name_overrides, annex_b_forbidden_names, expected_shape, released, provided_text),
            None => Err(vec![Diagnostic::unsupported(name, "native guest code required: source compilation is unavailable")]),
        }
    }

    pub(crate) fn specialize_source(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &dyn AtomPrefix,
        source_type: SourceType,
        module_goal: bool,
        capture_script_completion: bool,
        inherited_strict: bool,
        private_name_overrides: &[(String, String)],
        annex_b_forbidden_names: &[String],
        expected_shape: ParsedProgramShape<'_>,
        released: ReleasedRecords,
        provided_text: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        // Without the front end (a static image's support) no source compiles: nothing links the parser.
        #[cfg(not(feature = "source"))]
        {
            let _ = (source, mode, atom_prefix, source_type, module_goal, capture_script_completion, inherited_strict,
                private_name_overrides, annex_b_forbidden_names, expected_shape, released, provided_text);
            return Err(vec![Diagnostic::unsupported(name, "native guest code required: source compilation is unavailable")]);
        }
        #[cfg(feature = "source")]
        let compile = |lazy| {
            Self::specialize_parsing(
                source,
                name,
                mode,
                atom_prefix,
                source_type,
                module_goal,
                capture_script_completion,
                inherited_strict,
                private_name_overrides,
                annex_b_forbidden_names,
                expected_shape,
                lazy,
                released,
                DataLiterals::Admitted,
                provided_text.clone(),
            )
        };
        // Lazy function bodies apply to source a replay can parse again; a program that does
        // not compile with them compiles eagerly, which reports its own diagnostics.
        // A TypeScript body is checked alone as it is released, and admitted again where it
        // compiles (`lazy_bodies::parse`): a construct admission must reject is found by that
        // check, which fails the lazy compilation so the eager one reports it at load
        // (`BodyFacts::check`). Its declarations are collected where it compiles, against the
        // program's type names (`compile_parsed_again`).
        // A streamed body's first run parses the body lazily, to compile one run of it.
        #[cfg(feature = "source")]
        {
            let first_run = matches!(expected_shape, ParsedProgramShape::FirstCallChunk(window) if window.first_run());
            if (expected_shape.lazy_functions(expected_shape.replayable(private_name_overrides, annex_b_forbidden_names), source.len()) || first_run)
                && let Ok(program) = compile(true)
            {
                return Ok(program);
            }
            compile(false)
        }
    }

    /// `specialize_with_mode_and_private_names`, with the function bodies parsed lazily when
    /// `lazy` (`lazy_bodies`).
    #[allow(clippy::too_many_arguments)]
    fn specialize_parsing(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &dyn AtomPrefix,
        source_type: SourceType,
        module_goal: bool,
        capture_script_completion: bool,
        inherited_strict: bool,
        private_name_overrides: &[(String, String)],
        annex_b_forbidden_names: &[String],
        expected_shape: ParsedProgramShape<'_>,
        lazy: bool,
        released: ReleasedRecords,
        data_literals: DataLiterals,
        provided_text: Option<crate::bytecode::SourceText>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let normalized = early::normalize_hashbang(source);
        // A data script's literal is parsed as an empty one (`data_literal`). Replay
        // verification compares the tree with one parsed from the text, so it keeps it.
        let located = (data_literals == DataLiterals::Admitted
            && !source_type.is_typescript()
            && !cfg!(feature = "verify-replay"))
        .then(|| data_literal::locate(&normalized))
        .flatten();
        let parsed_text = located.as_ref().map_or(&*normalized, |literal| literal.substituted.as_str());
        let allocator = lazy_bodies::sized_arena(parsed_text);
        let stubbing = expected_shape.stubbing(expected_shape.replayable(private_name_overrides, annex_b_forbidden_names), source.len(), source_type.is_module());
        // A recognized body's check needs its call-only facts deferred, or none to find
        // (`BodyFacts::check`): only bodies with no function inside are recognized then.
        // A module checks every body alone (`BodyFacts::checks_every_body`), so a body is never
        // recognized without its tree: a check that declined one would parse it whole again.
        let recognize = if module_goal {
            annex_b_targets::Recognize::No
        } else if stubbing.functions {
            annex_b_targets::Recognize::All
        } else {
            annex_b_targets::Recognize::Flat
        };
        let facts = std::cell::RefCell::new(lazy_bodies::BodyFacts::deferring(stubbing, module_goal));
        { facts.borrow_mut().native_source_requests = atom_prefix.native_source_requests().map(|_| (atom_prefix.specialized(), Vec::new())); }
        // A first-call chunk's annotations resolve against its program's top-level names.
        let saved_names = match expected_shape {
            ParsedProgramShape::FirstCallChunk(window) => window.saved_names(),
            _ => None,
        };
        let mut declared_types = crate::bytecode::DeclaredTypes { type_names: saved_names, ..Default::default() };
        let mut parsed = lazy_bodies::parse_collecting(&allocator, parsed_text, source_type, &mut declared_types, lazy.then(|| expected_shape.lazy_from()), &facts, inherited_strict, recognize, atom_prefix.native_global_script_calls());
        lazy_bodies::release_parsed_free_pages(&allocator);
        let mut facts = facts.into_inner();
        // A first-call chunk's declarations are its program's, for its function (`ChunkWindow::declared`),
        // with the chunk's own admitted ones, in the chunk's positions, moved to the program's
        // (`shift`). Its annotations resolve against the names its admission saved.
        let chunk = matches!(expected_shape, ParsedProgramShape::FirstCallChunk(_));
        let chunk_names = declared_types.type_names.clone();
        if let ParsedProgramShape::FirstCallChunk(window) = expected_shape {
            let mut admitted = std::mem::take(&mut declared_types);
            admitted.shift(window.start());
            let mut declared = window.declared();
            declared.append(admitted);
            declared_types = declared;
        }
        let data_literal = match &located {
            Some(literal) if parsed.diagnostics.is_empty()
                && data_literal::is_top_level_value(&parsed.program, literal.start) => Some(literal.start),
            Some(_) => {
                drop(parsed);
                return Self::specialize_parsing(
                    source,
                    name,
                    mode,
                    atom_prefix,
                    source_type,
                    module_goal,
                    capture_script_completion,
                    inherited_strict,
                    private_name_overrides,
                    annex_b_forbidden_names,
                    expected_shape,
                    lazy,
                    released,
                    DataLiterals::Parsed,
                    provided_text,
                );
            }
            None => None,
        };
        #[cfg(feature = "verify-replay")]
        if !lazy {
            lazy_bodies::verify(name, &normalized, &parsed.program);
        }
        if parsed.stack_exhausted { return Err(vec![Diagnostic::stack_exhausted(name)]); }
        if !parsed.diagnostics.is_empty() {
            return Err(parsed
                .diagnostics
                .iter()
                .map(|error| Diagnostic::parse_failure(name, &normalized, error))
                .collect());
        }
        // One walk serves the checks below; each is read in the order the compiler reports them.
        let findings = checks::walk(&parsed.program, checks::Passes {
            depth: true,
            decorators: source_type.is_typescript(),
            eval_return: capture_script_completion && source_type.is_commonjs(),
            targets: parsed.has_call_assignment_targets,
        });
        if findings.stack_exhausted { return Err(vec![Diagnostic::stack_exhausted(name)]); }
        if let Some((span, message)) = findings.decorator {
            let mut diagnostic = Diagnostic::unsupported(name, message);
            diagnostic.span = span;
            return Err(vec![diagnostic]);
        }
        if let Some(error) = early::regexp_early_error(&parsed) {
            return Err(vec![match error {
                early::RegExpEarlyError::StackExhausted => Diagnostic::stack_exhausted(name),
                early::RegExpEarlyError::Syntax(message) => Diagnostic {
                    kind: DiagnosticKind::Syntax,
                    source: name.into(),
                    message,
                    span: Span::default(),
                },
            }]);
        }
        if let Some(span) = findings.eval_return {
            return Err(vec![Diagnostic {
                kind: DiagnosticKind::Syntax,
                source: name.into(),
                message: RETURN_OUTSIDE_FUNCTION_IN_EVAL.into(),
                span,
            }]);
        }
        if capture_script_completion {
            // CommonJS provides OXC's inherited new.target parsing context;
            // eval still has Script semantics for declarations and scopes.
            parsed.program.source_type = SourceType::script().with_typescript(source_type.is_typescript());
        }
        if let Some(span) = findings.invalid_target {
            return Err(vec![Diagnostic {
                kind: DiagnosticKind::Syntax,
                source: name.into(),
                message: "SyntaxError: invalid assignment target".into(),
                span,
            }]);
        }
        let ordinary_private_context = matches!(expected_shape, ParsedProgramShape::Eval(context, _)
            if context.home_atom.is_none() && context.in_function && !private_name_overrides.is_empty());
        let wrapped_eval = matches!(expected_shape, ParsedProgramShape::Eval(context, _)
            if context.home_atom.is_some()) || ordinary_private_context;
        let dynamic_function_shape = match expected_shape {
            ParsedProgramShape::Any | ParsedProgramShape::FirstCallScript | ParsedProgramShape::KeptRecords | ParsedProgramShape::FirstCallChunk(_) => true,
            ParsedProgramShape::Eval(_, expected_span) => {
                !wrapped_eval || eval_method_body(&mut parsed.program, ordinary_private_context)
                    .is_some_and(|body| Some(body.span) == expected_span)
            }
            ParsedProgramShape::DynamicFunction { parameter_list_end, .. } => {
                match parsed.program.body.as_mut_slice() {
                    [Statement::ExpressionStatement(statement)] => match &mut statement.expression {
                        Expression::ParenthesizedExpression(expression) => {
                            match &mut expression.expression {
                                Expression::FunctionExpression(function) => {
                                    let valid = function.id.as_ref().is_some_and(|identifier| identifier.name == DYNAMIC_FUNCTION_PUBLIC_NAME)
                                        && function.body.is_some()
                                        && function.params.span.end as usize == parameter_list_end;
                                    // The generated public source name is not a dynamic
                                    // Function self binding. OXC still owns the grammar.
                                    if valid { function.id = None; }
                                    valid
                                }
                                _ => false,
                            }
                        }
                        _ => false,
                    },
                    _ => false,
                }
            }
        };
        if !dynamic_function_shape {
            return Err(vec![Diagnostic {
                kind: DiagnosticKind::Syntax,
                source: name.into(),
                message: "SyntaxError: invalid dynamic Function source shape".into(),
                span: Span::default(),
            }]);
        }
        // Parsing is done; the semantic analysis starts here.
        compile_phase_boundary(CompilePhase::Analysis);
        let inherited_directive = add_use_strict(&mut parsed.program, &allocator, inherited_strict);
        let semantic = semantic_stats::checked_semantic(&parsed.program, name, expected_shape.checked_at_load())?;
        let mut private_name_ids = private_name_ids(&semantic.semantic);
        let mut private_name_labels = private_name_labels(&semantic.semantic);
        let mut prototype_unobservable = call_only::prototype_unobservable_functions(&semantic.semantic, &parsed.program, &Default::default());
        #[cfg(feature = "verify-replay")]
        let free_names = free_names::free_names(&semantic.semantic, &parsed.program);
        let inherited_private_ids = if wrapped_eval
        {
            let mut ids = FxHashSet::default();
            if let [Statement::ExpressionStatement(statement)] = parsed.program.body.as_slice()
                && let Expression::ClassExpression(class) =
                    statement.expression.without_parentheses()
            {
                for element in &class.body.body {
                    if let ClassElement::PropertyDefinition(field) = element {
                        let span = field.key.span();
                        if let Some(id) = private_name_ids.get(&(span.start, span.end)) {
                            ids.insert(*id);
                        }
                    }
                }
            }
            Some(ids)
        } else {
            None
        };
        let private_aliases = private_name_ids
            .iter()
            .filter_map(|(span, id)| {
                if inherited_private_ids
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(id))
                {
                    return None;
                }
                let label = private_name_labels.get(span)?;
                let (_, identity) = private_name_overrides
                    .iter()
                    .find(|(name, _)| name == label)?;
                Some((*id, identity.clone()))
            })
            .collect();
        // Validate in method grammar, then remove the context wrapper. Original
        // spans still address `normalized`, retaining exact nested function source.
        let native_requests = atom_prefix.native_source_requests().map(|pending| {
            (pending, Self::native_source_requests(&parsed.program, semantic.semantic.scoping(), atom_prefix.specialized(), atom_prefix.native_global_script_calls()))
        });
        let type_only_imports = if module_goal { typescript::type_only_imports(&parsed.program, semantic.semantic.scoping()) } else { Vec::new() };
        drop(semantic);
        parsed.program.directives.drain(..usize::from(inherited_directive));
        typescript::elide_type_only_imports(&mut parsed.program, &type_only_imports, &mut declared_types);
        let mut native_requests = native_requests;
        if lazy {
            // The lazily parsed bodies, checked and analyzed one at a time as the parser
            // released them (`lazy_bodies::BodyFacts::check`).
            if facts.failed || parsed.lazy_methods_kept {
                return Err(vec![Diagnostic::unsupported(name, "SyntaxError: a lazily parsed function body does not check alone")]);
            }
            let first_private_id = private_name_ids.values().max().map_or(0, |id| id + 1);
            if let Some((_, requests)) = facts.native_source_requests
                && let Some((_, pending)) = &mut native_requests { pending.extend(requests); }
            private_name_ids.extend(facts.private_name_ids.into_iter().map(|(span, id)| (span, first_private_id + id)));
            private_name_labels.extend(facts.private_name_labels);
            prototype_unobservable.extend(facts.prototype_unobservable);
        }
        // Parsing and analysis are done; the code generator's work starts here.
        compile_phase_boundary(CompilePhase::CodeGeneration);
        let eval_context = if let ParsedProgramShape::Eval(context, _) = expected_shape {
            if wrapped_eval {
                let body = eval_method_body(&mut parsed.program, ordinary_private_context)
                    .expect("validated eval context shape");
                let statements = std::mem::replace(
                    &mut body.statements,
                    oxc_allocator::Vec::new_in(&&*allocator),
                );
                let directives = std::mem::replace(
                    &mut body.directives,
                    oxc_allocator::Vec::new_in(&&*allocator),
                );
                parsed.program.body = statements;
                parsed.program.directives = directives;
                if let Some(span) = checks::walk(&parsed.program, checks::Passes { eval_return: true, ..Default::default() }).eval_return {
                    return Err(vec![Diagnostic {
                        kind: DiagnosticKind::Syntax,
                        source: name.into(),
                        message: RETURN_OUTSIDE_FUNCTION_IN_EVAL.into(),
                        span,
                    }]);
                }
            }
            Some(context)
        } else {
            None
        };
        let mut compiler = match expected_shape {
            ParsedProgramShape::FirstCallChunk(window) => window.compiler(name, mode, atom_prefix, &mut parsed.program, private_name_ids, &mut private_name_labels, &mut prototype_unobservable),
            _ => Compiler::new(name, &normalized, mode, atom_prefix, private_name_ids),
        };
        compiler.data_literal = data_literal;
        compiler.provided_text = provided_text;
        compiler.declared_types = declared_types;
        compiler.type_names = if chunk { chunk_names } else { None };
        compiler.prototype_unobservable = prototype_unobservable;
        #[cfg(feature = "verify-replay")]
        {
            compiler.free_names = free_names;
        }
        compiler.source_type = source_type;
        // Eval code in its caller's context and dynamic functions compile from a wrapper, or a
        // tree changed after parsing, that their text alone does not give back. A released
        // function compiles again from its replay on first call (`ResidualProgram::function`).
        compiler.replayable_source = expected_shape.replayable(private_name_overrides, annex_b_forbidden_names);
        compiler.first_call.plan(expected_shape, compiler.replayable_source, &parsed.program, inherited_strict, (std::mem::take(&mut facts.free_names), std::mem::take(&mut facts.call_only_found)));
        compiler.streamed = std::mem::take(&mut facts.streamed);
        compiler.stream_candidates = std::mem::take(&mut facts.stream_candidates);
        compiler.checked_bodies = std::mem::take(&mut facts.checked);
        if let ParsedProgramShape::FirstCallChunk(window) = expected_shape {
            // The chunk's text parsed alone; its functions now lie at their place.
            let delta = window.start();
            compiler.streamed = compiler.streamed.iter().map(|start| start + delta).collect();
            compiler.stream_candidates = compiler.stream_candidates.iter().map(|start| start + delta).collect();
            compiler.first_call.free_names = std::mem::take(&mut compiler.first_call.free_names).shifted(delta);
            compiler.first_call.call_only_found.iter_mut().for_each(|(start, end)| (*start, *end) = (*start + delta, *end + delta));
        }
        compiler.load_encoding =
            (compiler.replayable_source && released == ReleasedRecords::Encoded).then(Default::default);
        compiler.private_name_labels = private_name_labels;
        compiler.private_name_overrides = private_aliases;
        compiler.capture_script_completion = capture_script_completion;
        compiler.eval_context = eval_context;
        // Only an eval's caller forbids names (its lexical bindings); without one no function
        // collides, and the walk would find nothing to keep.
        if !annex_b_forbidden_names.is_empty() {
            compiler.eval_annex_b_collisions = early::annex_b_function_names(&parsed.program.body)
                .into_iter()
                .filter_map(|(span, name)| annex_b_forbidden_names.contains(&name).then_some(span))
                .collect();
        }
        let emitted = compiler.program(&parsed.program, module_goal, inherited_strict);
        #[cfg(feature = "profile-memory")]
        program_profile::report_parse_arena(&allocator);
        // The tree is the largest structure of a compilation; the passes that finish the
        // program do not read it, nor the text it was parsed from when that was a copy.
        drop(parsed);
        drop(allocator);
        drop(located);
        let (compiler, module) = emitted?;
        let program = compiler.finish(module)?;
        // Lazy parsing releases function-body ASTs. A native-only build freezes complete
        // diagnostic facts once from source without forcing their bytecode replay; a
        // dynamic one defers that parse to the first diagnostic or write that reads them.
        if !crate::DYNAMIC_EXECUTION_ENABLED {
            crate::vm::freeze_call_sites(&program);
        }
        if let Some((pending, requests)) = native_requests { pending.borrow_mut().extend(requests); }
        #[cfg(feature = "verify-replay")]
        replay::verify(&program);
        Ok(program)
    }
}

/// Gives `program`, when it `inherits` strictness (eval code in strict code), a leading
/// `"use strict"` directive unless it has one, and says whether it did: inherited strictness is
/// no directive of its text, and OXC's checks see it only as one. The caller removes it before
/// code generation, where a directive would be the program's completion value.
fn add_use_strict<'a>(program: &mut Program<'a>, allocator: &'a Allocator, inherits: bool) -> bool {
    let added = inherits && !program.has_use_strict_directive();
    if added {
        program.directives.insert(0, oxc_ast::ast::Directive::new_use_strict(&oxc_ast::builder::AstBuilder::new(allocator)));
    }
    added
}

/// A point in compilation where the code about to run differs from the code that ran, which
/// an embedding may act on (`set_compile_phase_boundary`), as a host returns read-only pages
/// the next phase does not touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilePhase {
    /// A compilation has parsed its source; its semantic analysis starts.
    Analysis,
    /// A compilation has parsed and analysed its source; its code generation starts.
    CodeGeneration,
    /// A compiled program is complete and its encoding for the host's compile cache starts:
    /// the parser's and code generator's work is over, and the encoder's begins.
    Encoding,
    /// The code generator's thread finished a compile: the lowering's and code generator's code
    /// pages it touched are no run's working set.
    NativeCodeDone,
}

/// What a compilation does at each `CompilePhase`, set once by the embedding.
static COMPILE_PHASE_BOUNDARY: std::sync::OnceLock<fn(CompilePhase)> = std::sync::OnceLock::new();

/// Sets the action a compilation takes at each `CompilePhase`; the first call wins.
pub fn set_compile_phase_boundary(action: fn(CompilePhase)) {
    let _ = COMPILE_PHASE_BOUNDARY.set(action);
}

pub(crate) fn compile_phase_boundary(phase: CompilePhase) {
    if let Some(action) = COMPILE_PHASE_BOUNDARY.get() {
        action(phase);
    }
}

pub(crate) fn module_default_binding(module_name: &str) -> String {
    format!("\0quench:module-default:{module_name}")
}

fn static_declaration_exports(
    declaration: &Declaration<'_>,
    output: &mut Vec<(String, String)>,
) -> Option<()> {
    let mut names = Vec::new();
    match declaration {
        Declaration::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                early::collect_pattern_names(&declarator.id, &mut names);
            }
        }
        Declaration::FunctionDeclaration(function) => {
            names.push(function.id.as_ref()?.name.to_string());
        }
        Declaration::ClassDeclaration(class) => {
            names.push(class.id.as_ref()?.name.to_string());
        }
        _ => return None,
    }
    output.extend(names.into_iter().map(|name| (name.clone(), name)));
    Some(())
}

fn module_requests(statements: &[Statement<'_>]) -> Vec<ModuleRequest> {
    let mut requests = Vec::new();
    for statement in statements {
        match statement {
            Statement::ImportDeclaration(import) => {
                let phase = module_request_phase(import.phase);
                push_module_request(
                    &mut requests,
                    import.source.value.to_string(),
                    phase,
                    import_module_type(import.with_clause.as_deref()),
                );
            }
            Statement::ExportFromDeclaration(export) => push_module_request(
                &mut requests,
                export.source.value.to_string(),
                ModuleRequestPhase::Evaluation,
                import_module_type(export.with_clause.as_deref()),
            ),
            Statement::ExportAllDeclaration(export) => push_module_request(
                &mut requests,
                export.source.value.to_string(),
                ModuleRequestPhase::Evaluation,
                import_module_type(export.with_clause.as_deref()),
            ),
            _ => {}
        }
    }
    requests
}

fn import_module_type(clause: Option<&WithClause<'_>>) -> Option<String> {
    clause?.with_entries.iter().find_map(|attribute| {
        let key = match &attribute.key {
            ImportAttributeKey::Identifier(key) => key.name.as_str(),
            ImportAttributeKey::StringLiteral(key) => key.value.as_str(),
        };
        (key == "type").then(|| attribute.value.value.to_string())
    })
}

fn module_request_phase(phase: Option<ImportPhase>) -> ModuleRequestPhase {
    match phase {
        Some(ImportPhase::Defer) => ModuleRequestPhase::Defer,
        Some(ImportPhase::Source) => ModuleRequestPhase::Source,
        None => ModuleRequestPhase::Evaluation,
    }
}

fn module_import_bindings(statements: &[Statement<'_>]) -> Vec<ModuleImportBinding> {
    statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(import) => Some((
                import.source.value.to_string(),
                module_request_phase(import.phase),
                import_module_type(import.with_clause.as_deref()),
                import.specifiers.as_ref(),
            )),
            _ => None,
        })
        .flat_map(|(source, phase, module_type, specifiers)| {
            specifiers.into_iter().flatten().map(move |specifier| {
                let (imported, local) = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier) => (
                        ModuleImportName::Named(module_export_name(&specifier.imported)),
                        specifier.local.name.to_string(),
                    ),
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => (
                        ModuleImportName::Named("default".into()),
                        specifier.local.name.to_string(),
                    ),
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => (
                        ModuleImportName::Namespace,
                        specifier.local.name.to_string(),
                    ),
                };
                ModuleImportBinding {
                    source: source.clone(),
                    phase,
                    module_type: module_type.clone(),
                    imported,
                    local,
                }
            })
        })
        .collect()
}

fn push_module_request(
    requests: &mut Vec<ModuleRequest>,
    source: String,
    phase: ModuleRequestPhase,
    module_type: Option<String>,
) {
    if !requests.iter().any(|request| {
        request.source == source && request.phase == phase && request.module_type == module_type
    }) {
        requests.push(ModuleRequest {
            source,
            phase,
            module_type,
        });
    }
}

/// The functions and arrows written in `program`, each of which the compiler emits.
/// The module records `Compiler::program` reads from the tree for `Compiler::finish`.
struct ModuleParts {
    requests: Vec<crate::bytecode::ModuleRequest>,
    imports: Vec<crate::bytecode::ModuleImportBinding>,
    link_plan: Option<crate::bytecode::ModuleLinkPlan>,
}

fn has_top_level_await(statements: &[Statement<'_>]) -> bool {
    struct Finder(bool);
    impl<'a> oxc_ast_visit::Visit<'a> for Finder {
        fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
            self.0 = true;
        }

        fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
            // `for await` suspends just like an explicit top-level await.
            // Include it in the async-module decision so the root compiler
            // context lowers its loop body with await support enabled.
            self.0 |= statement.r#await;
            oxc_ast_visit::walk::walk_for_of_statement(self, statement);
        }

        fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
            self.0 |= declaration.kind == VariableDeclarationKind::AwaitUsing;
            oxc_ast_visit::walk::walk_variable_declaration(self, declaration);
        }

        fn visit_function(
            &mut self,
            _: &oxc_ast::ast::Function<'a>,
            _: oxc_syntax::scope::ScopeFlags,
        ) {
        }

        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    }
    let mut finder = Finder(false);
    for statement in statements {
        oxc_ast_visit::walk::walk_statement(&mut finder, statement);
    }
    finder.0
}

fn static_module_bindings(
    declaration: &VariableDeclaration<'_>,
    bindings: &mut FxHashMap<String, Constant>,
) -> Option<Vec<String>> {
    let mut names = Vec::with_capacity(declaration.declarations.len());
    for declarator in &declaration.declarations {
        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
            return None;
        };
        let value = match &declarator.init {
            Some(expression) => binding_time::expression(expression)?,
            None => Constant::Undefined,
        };
        let name = identifier.name.to_string();
        bindings.insert(name.clone(), value);
        names.push(name);
    }
    Some(names)
}

fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(identifier) => identifier.name.to_string(),
        ModuleExportName::IdentifierReference(identifier) => identifier.name.to_string(),
        ModuleExportName::StringLiteral(literal) => literal.value.to_string(),
    }
}

fn module_hoisted_functions(
    statements: &[Statement<'_>],
    module_name: &str,
) -> Vec<(String, String)> {
    let mut functions = Vec::new();
    for statement in statements {
        let declaration = match statement {
            Statement::FunctionDeclaration(function) => function
                .id
                .as_ref()
                .map(|name| (name.name.to_string(), name.name.to_string())),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => function
                    .id
                    .as_ref()
                    .map(|name| (name.name.to_string(), name.name.to_string())),
                _ => None,
            },
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &export.declaration
                {
                    default_export_binding(&export.declaration, module_name).map(|binding| {
                        let name = function
                            .id
                            .as_ref()
                            .map(|identifier| identifier.name.to_string())
                            .unwrap_or_else(|| "default".into());
                        (binding, name)
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(declaration) = declaration {
            functions.push(declaration);
        }
    }
    functions
}

fn append_module_link_statement(
    statement: &Statement<'_>,
    module_name: &str,
    plan: &mut ModuleLinkPlan,
) -> Option<()> {
    match statement {
        Statement::ExportDeclaration(export) => {
            static_declaration_exports(&export.declaration, &mut plan.locals)?;
        }
        Statement::ExportDefaultDeclaration(export) => {
            plan.locals.push((
                default_export_binding(&export.declaration, module_name)?,
                "default".into(),
            ));
        }
        Statement::ExportNamedDeclaration(export) => {
            plan.locals
                .extend(export.specifiers.iter().map(|specifier| {
                    (
                        module_export_name(&specifier.local),
                        module_export_name(&specifier.exported),
                    )
                }));
        }
        Statement::ExportFromDeclaration(export) => {
            let source = export.source.value.to_string();
            plan.reexports
                .extend(
                    export
                        .specifiers
                        .iter()
                        .map(|specifier| StaticModuleReexport::Named {
                            source: source.clone(),
                            imported: module_export_name(&specifier.local),
                            exported: module_export_name(&specifier.exported),
                        }),
                );
        }
        Statement::ExportAllDeclaration(export) => {
            let source = export.source.value.to_string();
            plan.reexports.push(match &export.exported {
                Some(name) => StaticModuleReexport::Namespace {
                    source,
                    exported: module_export_name(name),
                },
                None => StaticModuleReexport::Star { source },
            });
        }
        _ => {}
    }
    Some(())
}

fn default_export_binding(
    declaration: &ExportDefaultDeclarationKind<'_>,
    module_name: &str,
) -> Option<String> {
    match declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => Some(
            function
                .id
                .as_ref()
                .map(|identifier| identifier.name.to_string())
                .unwrap_or_else(|| module_default_binding(module_name)),
        ),
        ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(
            class
                .id
                .as_ref()
                .map(|identifier| identifier.name.to_string())
                .unwrap_or_else(|| module_default_binding(module_name)),
        ),
        declaration if declaration.as_expression().is_some() => {
            Some(module_default_binding(module_name))
        }
        _ => None,
    }
}

fn same_module_path(module_name: &str, specifier: &str) -> bool {
    crate::module_identity::resolves_to(module_name, specifier)
}

pub(super) fn is_lexical_binding_declaration(kind: VariableDeclarationKind) -> bool {
    matches!(
        kind,
        VariableDeclarationKind::Let
            | VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing
    )
}

fn is_immutable_binding_declaration(kind: VariableDeclarationKind) -> bool {
    matches!(
        kind,
        VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecializationMode {
    Enabled,
    Disabled,
}

impl SpecializationMode {
    /// A program compiled at run time specializes exactly when the VM it extends does, so
    /// eval, Function and host-script code keep the inline caches of the code that made it.
    fn of(host: &dyn AtomPrefix) -> Self {
        if host.specialized() {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

/// A named property access, by which past its unique sites a program shares cache sites
/// (`Compiler::name_shared_cache_site`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NamedAccess {
    Load,
    Store,
}

/// The name-shared cache sites (`NAME_SHARED_CACHE_SITES`) by access and atom: given out
/// at load, with the next one to give, and read in a replay, which keeps the load's.
enum NameSharedSites<'a> {
    Load { sites: NameSharedSiteMap, next: u16 },
    Replay(&'a NameSharedSiteMap),
    /// A first-call chunk's: the load's sites, then those its program's chunks gave out.
    Chunk(&'a NameSharedSiteMap, &'a replay::ChunkNameSites),
}

type NameSharedSiteMap = FxHashMap<(NamedAccess, u32), u16>;

struct Compiler<'a> {
    source: &'a str,
    text: &'a str,
    source_comments: Vec<Span>,
    /// The TypeScript declared environment its parse admitted (`typescript::admit`).
    declared_types: crate::bytecode::DeclaredTypes,
    /// The type names the annotations of the code being compiled resolve against, where that code is a
    /// lazily parsed body or a first-call chunk: its program's saved names, shadowed by its own
    /// declarations (`with_type_names`). `None` reads the names the way the program's own code does,
    /// with its declared parameter facts applied at the end (`apply_parameters`).
    type_names: Option<std::rc::Rc<[crate::bytecode::SavedTypeName]>>,
    /// A TypeScript checker's parameter types for this text (`typescript::checked`).
    checked_parameters: Option<std::sync::Arc<typescript::CheckedParameters>>,
    mode: SpecializationMode,
    root_strict: bool,
    module_goal: bool,
    capture_script_completion: bool,
    eval_context: Option<EvalContext>,
    /// The text the caller already holds in shared storage, which the program keeps in place
    /// of a copy of `text` (`source_text::kept_or_provided`).
    provided_text: Option<crate::bytecode::SourceText>,
    eval_annex_b_collisions: early::AnnexBCollisions,
    /// The script's own global lexical names, in declaration order: a nested function's read or write
    /// of one names it by that index (`LoadGlobalLexical`). Empty for a module or an eval.
    root_lexical_atoms: Vec<Atom>,
    atoms: CompilerAtoms<'a>,
    /// The capture markers made so far by binding kind (`None` for a catch parameter's) and
    /// source atom (`capture_marker`).
    capture_markers: FxHashMap<(Option<crate::bytecode::LexicalBindingKind>, Atom), Atom>,
    /// Private names resolved so far (`private_name_atom`): private member access resolves
    /// through name bindings, so functions resolving one keep theirs.
    private_name_references: u64,
    private_name_ids: FxHashMap<(u32, u32), u32>,
    /// The source starts of the function expressions whose `prototype` no program text
    /// observes (`call_only`); empty where the source was not analyzed.
    prototype_unobservable: FxHashSet<u32>,
    /// In a replay, the starts of the function bodies the load released
    /// (`ProgramReplay::lazy_bodies`), which the replay releases too (`Nesting::Recorded`).
    lazy_bodies: FxHashSet<u32>,
    /// The starts of the function bodies compiled from their source parsed again, which a
    /// replay stands in the same way (`ProgramReplay::lazy_bodies`).
    lazy_compiled: Vec<u32>,
    /// The starts of the function bodies the load's check found free of early errors
    /// (`BodyFacts::checked`), which its replay keeps (`ProgramReplay::checked_bodies`).
    checked_bodies: Vec<u32>,
    /// The arenas lazily parsed functions are parsed again in, one per nesting of such
    /// functions, each reset and reused: an arena per function interleaved its blocks with
    /// the program's long-lived records, and the heap kept the holes between them.
    lazy_arenas: Vec<Allocator>,
    /// The starts of the lazily parsed functions whose bodies compile as a stream of statement
    /// runs (`statement_stream`), and the body the next function compiled streams.
    streamed: FxHashSet<u32>,
    stream_candidates: FxHashSet<u32>,
    body_stream: Option<statement_stream::StreamedBody>,
    /// The run of a streamed function's body this chunk compiles (`StreamRun`).
    stream_run: Option<&'a statement_stream::StreamRun>,
    /// Each function's free names (`free_names`), which `verify_predicted_captures` checks
    /// against what the function's compilation captured.
    #[cfg(feature = "verify-replay")]
    free_names: free_names::FreeNames,
    private_name_labels: FxHashMap<(u32, u32), String>,
    private_name_overrides: FxHashMap<u32, String>,
    constants: SiteTable<'a, Constant>,
    constant_index: replay::ConstantIndex<'a>,
    functions: replay::FunctionTable<'a>,
    errors: Vec<Diagnostic>,
    cache_sites: u16,
    name_shared_sites: NameSharedSites<'a>,
    template_sites: u32,
    method_sites: SiteTable<'a, MethodSiteSpec>,
    field_sites: SiteTable<'a, FieldSite>,
    object_sites: SiteTable<'a, ObjectSite>,
    regexp_literal_sites: SiteTable<'a, crate::bytecode::RegExpLiteralSite>,
    literal_templates: SiteTable<'a, Box<[crate::bytecode::LiteralStep]>>,
    /// The source start of a data script's literal, which the tree holds as an empty one
    /// (`data_literal`).
    data_literal: Option<u32>,
    superinstructions: SiteTable<'a, Superinstruction>,
    /// Distinct `name_bindings` lists: sibling functions that see the same bindings,
    /// such as every function declared in one module scope, share one list.
    /// The captured scopes and declared names of each function's name bindings, resolved
    /// in `finish` for the functions dynamic name resolution can reach.
    deferred_name_bindings: FxHashMap<u32, DeferredNameBindings>,
    /// Resolved captured scopes keyed by (scope, enclosing chain) addresses; see `captured_names`.
    captured_name_scopes: FxHashMap<
        (usize, Option<usize>),
        (
            std::rc::Rc<capture_scope::CaptureScope>,
            std::rc::Rc<crate::bytecode::CapturedNames>,
        ),
    >,
    /// The tables' cursors once each function, its nested ones included, is compiled.
    function_exits: replay::FunctionRecords<replay::TableCursors>,
    /// What the program-wide layout analyses read of each compiled function.
    function_facts: replay::FunctionRecords<capture_layout::FunctionFacts>,
    /// Functions being compiled whose bodies may give nested functions binding sites
    /// (`early::DirectEvalFacts::binds_dynamically`). While any is open, a function finished inside
    /// it may still resolve names dynamically, so none is released early.
    dynamic_binding_scopes: u32,
    /// The replays of functions finished and released during the load.
    early_replays: FxHashMap<u32, replay::FunctionReplay>,
    /// The source starts of function expressions called where they are made or marked as
    /// about to run, which keep their records (`note_invoked_callee`).
    invoked_functions: FxHashSet<u32>,
    /// The buffers every function's lowering passes share (`CompileScratch`).
    scratch: CompileScratch,
    /// How each replayable function was compiled, with the cursors it started from.
    replay_calls: FxHashMap<u32, (replay::TableCursors, replay::ReplayCall)>,
    /// In a replay, the function it recompiles and where the load left each function.
    replay_target: Option<(u32, &'a [replay::TableCursors])>,
    source_type: oxc_span::SourceType,
    /// In a replay, the finished program whose function it compiles.
    replay_program: Option<&'a ResidualProgram>,
    /// Whether the program's functions compile again from their text alone.
    replayable_source: bool,
    /// What the compilation knows of first-call stubs (`first_call`).
    first_call: first_call::Stubs<'a>,
    /// The compile-cache records of the functions released during the load, when its
    /// program will be encoded (`ReleasedRecords::Encoded`).
    load_encoding: Option<crate::bytecode::LoadEncoding>,
    /// The instructions of the functions whose rewrites the load left to their first compile
    /// (`Compiler::defers_rewrites`), which bound the sites those add.
    deferred_rewrite_code: usize,
    /// The kept functions whose rewrites wait for their first read, in id order
    /// (`Compiler::defers_kept_rewrites`); the program's `RewriteRanges::deferred`.
    kept_rewrites: Vec<u32>,
}

/// Whether a compilation may parse a data script's literal as an empty one
/// (`data_literal`), or parses the text as it is, as after a located literal the parse did
/// not admit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataLiterals {
    Admitted,
    Parsed,
}

/// What a compilation keeps of each record it releases during its load
/// (`Compiler::finish_early`) beside the function's header and replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReleasedRecords {
    /// Nothing: the function compiles again from its source on its first read.
    Dropped,
    /// Its compile-cache encoding, for a program its host encodes once compiled: the
    /// encoding copies it instead of compiling every released function again (npm scenario,
    /// cold, empty cache, x86_64 callgrind: prettier 4.32 -> 2.98 G instructions, cold
    /// `import typescript` 19.7 -> 11.3 G).
    Encoded,
}

/// The compiler's tables as the derived facts of a function it finished read them
/// (`FactTables`): every function nested in it is finished too, so each it makes has its
/// record or its released header.
struct LoadTables<'c, 'a> {
    atoms: &'c CompilerAtoms<'a>,
    functions: &'c replay::FunctionTable<'a>,
    superinstructions: &'c [crate::bytecode::Superinstruction],
}

impl crate::bytecode::FactTables for LoadTables<'_, '_> {
    fn atom_name(&self, atom: usize) -> Option<&str> {
        self.atoms.get(atom)
    }

    fn function_is_arrow(&self, function: usize) -> Option<bool> {
        self.functions.is_arrow(u32::try_from(function).ok()?)
    }

    fn superinstructions(&self) -> &[crate::bytecode::Superinstruction] {
        self.superinstructions
    }
}

type MethodSiteSpec = (Atom, u16, Vec<Register>, Option<(Atom, u16)>);

/// The name prefix of a catch parameter's capture marker (`Compiler::capture_marker`).
const CATCH_CAPTURE_PREFIX: &str = "\0quench:catch-capture:";

/// A function's name bindings before its captured names are resolved: its direct bindings,
/// the scopes it closes over and the names it declares.
struct DeferredNameBindings {
    /// Whether the function or one it encloses names a private member, which resolves its
    /// home through these bindings.
    names_private_members: bool,
    direct: Vec<crate::bytecode::EvalBinding>,
    scopes: capture_scope::ScopeChain,
    function_scope: Vec<Atom>,
}

fn private_name_ids(semantic: &oxc_semantic::Semantic<'_>) -> FxHashMap<(u32, u32), u32> {
    let classes = semantic.classes();
    let mut definitions = FxHashMap::default();
    let mut next_id = 0;
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                definitions
                    .entry((class_id, element.name.to_string()))
                    .or_insert_with(|| {
                        let id = next_id;
                        next_id += 1;
                        id
                    });
            }
        }
    }
    let mut names = FxHashMap::default();
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                let id = definitions[&(class_id, element.name.to_string())];
                names.insert((element.span.start, element.span.end), id);
            }
        }
        for reference in classes.iter_private_identifiers(class_id) {
            let resolved = classes.ancestors(class_id).find_map(|ancestor| {
                definitions
                    .get(&(ancestor, reference.name.to_string()))
                    .copied()
            });
            if let Some(id) = resolved {
                names.insert((reference.span.start, reference.span.end), id);
            }
        }
    }
    names
}

fn private_name_labels(semantic: &oxc_semantic::Semantic<'_>) -> FxHashMap<(u32, u32), String> {
    let classes = semantic.classes();
    let mut labels = FxHashMap::default();
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                labels.insert(
                    (element.span.start, element.span.end),
                    element.name.to_string(),
                );
            }
        }
        for reference in classes.iter_private_identifiers(class_id) {
            labels.insert(
                (reference.span.start, reference.span.end),
                reference.name.to_string(),
            );
        }
    }
    labels
}

#[derive(Clone, Copy)]
enum FunctionBody<'a> {
    Statements(&'a [Statement<'a>]),
    Expression(&'a Expression<'a>),
}

#[derive(Default)]
struct FunctionOptions<'a> {
    defaults: Option<&'a FormalParameters<'a>>,
    directive_completion: Option<&'a StringLiteral<'a>>,
    source_span: Option<crate::bytecode::SourceSpan>,
    name_binding: Option<Atom>,
    async_function: bool,
    generator: bool,
    class_constructor: bool,
    derived_constructor: bool,
    non_constructible: bool,
    /// `Function::prototype_unobservable`.
    prototype_unobservable: bool,
    class_initializer: bool,
    instance_fields: Option<&'a [ClassField<'a>]>,
    instance_private_methods: Option<&'a [Atom]>,
    super_static: bool,
    super_home: bool,
    super_home_atom: Option<Atom>,
    rest_override: bool,
    implicit_body: Option<first_call::ImplicitBody>,
    strict: bool,
    with_depth: u16,
}

#[derive(Clone, Copy)]
enum ClassField<'a> {
    Property(&'a PropertyDefinition<'a>),
    AutoAccessor {
        accessor: &'a AccessorProperty<'a>,
        backing: Atom,
    },
}

/// A constant's identity for deduplication, borrowed from it: numbers by their bits, so
/// `-0` and each NaN payload stay distinct.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ConstantKey<'a> {
    Number(u64),
    WasmBits64(u64),
    WasmV128(&'a [u8; crate::wasm::V128_BYTES]),
    String(&'a str),
    StringUnits(&'a [u16]),
    BigInt(&'a num_bigint::BigInt),
    Boolean(bool),
    Null,
    Undefined,
}

impl<'a> From<&'a Constant> for ConstantKey<'a> {
    fn from(value: &'a Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(value.to_bits()),
            Constant::WasmBits64(bits) => Self::WasmBits64(*bits),
            Constant::WasmV128(bits) => Self::WasmV128(bits),
            Constant::String(value) => Self::String(value),
            Constant::StringUnits(value) => Self::StringUnits(value),
            Constant::BigInt(value) => Self::BigInt(value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}

/// Whether a load lowers every record it keeps instead of leaving it to the record's first
/// read: under `verify-replay`, and in tests that compare the two (`LOWER_AT_LOAD`).

fn lowers_at_load() -> bool {
    #[cfg(test)]
    if LOWER_AT_LOAD.with(std::cell::Cell::get) {
        return true;
    }
    cfg!(feature = "verify-replay")
}

#[cfg(test)]
thread_local! {
    pub(crate) static LOWER_AT_LOAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The program tables a function's lowering reads, final once every function's rewrites
/// have added their sites: at load the compiler's, after it the finished program's.
pub(crate) struct LoweringTables<'t> {
    pub(crate) object_sites: &'t [ObjectSite],
    pub(crate) methods: &'t dyn liveness::MethodArguments,
    pub(crate) field_sites: &'t [FieldSite],
    pub(crate) superinstructions: &'t [crate::bytecode::Superinstruction],
    pub(crate) specialized: bool,
    pub(crate) undefined_constant: u32,
}

/// The largest loop-free body whose copies are coalesced. This bound is unmeasured: the copy
/// scans visit at most `copies::SCAN_LIMIT` instructions per `Move`, and the liveness and copy
/// buffers are reused from `CompileScratch`, so a loop-free body's coalescing grows with its
/// size rather than with its block lengths. The bound was picked without a measurement and
/// must be checked on octane-code-load, where a coalescing pass once cost 4.7% of the run.
/// It covers a leaf small enough for a call entry to recognise by its shape
/// (`capture_append::recognizes`), which is 16 instructions or fewer.
const LOOP_FREE_COALESCE_INSTRUCTIONS: usize = 128;

/// Whether coalescing the copies frame-register promotion left repays its cost: a body that
/// loops runs its instructions many times and its loops' tiers read the coalesced shape, and a
/// loop-free body up to `LOOP_FREE_COALESCE_INSTRUCTIONS` is coalesced too, since its scans are
/// bounded (see that constant).
fn coalescing_repays(function: &BcFunction) -> bool {
    rewrite::live_count(&function.code) <= LOOP_FREE_COALESCE_INSTRUCTIONS || function.loops()
}

/// The lowering passes after a function's dispatch class is settled (`settle_dispatch`):
/// numeric and plain-local specialization and frame registers. They add no table entries,
/// so they run equally at load and on a function's first read from its finished program.
/// Returns the live sets of the function as it stands when the last pass solved them and
/// rewrote nothing after (`liveness::register_roots_from` reuses them), else `None`.
pub(crate) fn lower_settled<'a>(
    function: &mut BcFunction,
    tables: &LoweringTables<'_>,
    atom_name: impl Fn(usize) -> Option<&'a str>,
    scratch: &mut CompileScratch,
) -> Option<Vec<liveness::RegisterSet>> {
    let LoweringTables { methods, field_sites, superinstructions, specialized, undefined_constant, object_sites } = *tables;
    if function.dispatch == DispatchClass::Numeric {
        let live = liveness::buffers::analyze_with(function, methods, field_sites, superinstructions, scratch);
        numeric::apply(function, live.as_deref());
    }
    // Renaming a plain slot's operations keeps it plain (validation re-derives the same
    // proof from the specialized code), so frame registers start from the same slots.
    frame_registers::drop_unread_self_binding(function, superinstructions, &atom_name);
    // Specialization renames operations without moving control, so the TDZ analyses of both
    // passes read one set of blocks.
    let flow = crate::bytecode::initialization::FlowCell::new();
    let plain_slots = Compiler::specialize_plain_local_operations(function, superinstructions, &flow);
    frame_registers::promote(function, plain_slots.clone(), superinstructions, undefined_constant, &atom_name, &flow, scratch);
    if specialized {
        if coalescing_repays(function) {
            rewrite::coalesce_promoted_moves(function, methods, field_sites, superinstructions, scratch);
        }
        object_registers::promote(function, methods, field_sites, superinstructions, object_sites, scratch);
        rewrite::hoist_loop_constants(function, methods, field_sites, superinstructions, scratch);
    }
    rest_reads::rewrite(function, methods, field_sites, superinstructions, &|atom| atom_name(atom as usize) == Some("length"));
    let mut solved = None;
    if function
        .code
        .iter()
        .any(|instruction| instruction.op().traits().has(Traits::PLAIN_LOCAL_STORE))
    {
        // This solve stays even when the first one ran: the passes above rewrite the code it
        // reads (`drop_unread_self_binding`, the promotions, `hoist_loop_constants`, `rest_reads`),
        // and none of them reports whether it changed anything, so the first solve's sets are
        // not known to match it.
        let live = liveness::buffers::analyze_with(function, methods, field_sites, superinstructions, scratch);
        if !numeric::apply_plain_local_stores(function, live.as_deref()) {
            solved = live;
        }
    }
    // The one compaction of the lowering: the passes above leave the `Nop`s their in-place
    // rewrites made, and the live sets this returns are compacted with the same map.
    rewrite::remove_nops(function, &mut solved, scratch);
    // The proof above is the final code's `plain_local_slots`: the passes after `promote` move no
    // slot, and `numeric::apply_plain_local_stores` fuses a plain store into its producer, which
    // the dataflow reads as the same initializing write (`initialization::accesses`). So the
    // runtime's first read need not derive the set again (`Function::plain_locals`).
    let words: crate::heap::ThinBoxSlice<u64> = plain_slots.words().to_vec().into();
    function.plain_locals = std::cell::OnceCell::from(words);
    solved
}

impl<'a> Compiler<'a> {
    /// The captured scopes of `scopes`, innermost first. A scope's names are resolved once per
    /// enclosing chain and shared by every function closing over it.
    /// Completes the name bindings of every function a dynamic name lookup may consult
    /// (`capture_layout::dynamic_name_scopes`), of every function private member access
    /// resolves through, and of the root; the rest keep their direct bindings, as no lookup
    /// reaches their captured names.
    fn resolve_name_bindings(&mut self, functions: &mut [Option<BcFunction>], reachable: &[bool]) {
        for (id, function) in functions.iter_mut().enumerate() {
            let Some(function) = function else { continue };
            let Some(deferred) = self.deferred_name_bindings.remove(&(id as u32)) else {
                continue;
            };
            if id != crate::vm::ROOT_FUNCTION_ID as usize
                && !reachable[id]
                && !deferred.names_private_members
            {
                continue;
            }
            let captured = self.captured_names(deferred.scopes.levels());
            let mut hidden: Vec<Atom> = deferred
                .function_scope
                .into_iter()
                .filter(|atom| captured.as_ref().is_some_and(|names| names.contains(*atom)))
                .collect();
            hidden.sort_unstable();
            function.name_bindings = crate::bytecode::NameBindings::new(deferred.direct, hidden, captured);
        }
    }

    fn captured_names(
        &mut self,
        scopes: &[std::rc::Rc<capture_scope::CaptureScope>],
    ) -> Option<std::rc::Rc<crate::bytecode::CapturedNames>> {
        if scopes
            .len()
            .checked_sub(1)
            .is_some_and(|deepest| u16::try_from(deepest).is_err())
        {
            self.reject(
                Span::default(),
                "eval capture depth exceeds residual encoding",
            );
            return None;
        }
        let mut outer: Option<std::rc::Rc<crate::bytecode::CapturedNames>> = None;
        for scope in scopes.iter().rev() {
            let key = (
                std::rc::Rc::as_ptr(scope) as usize,
                outer
                    .as_ref()
                    .map(|outer| std::rc::Rc::as_ptr(outer) as usize),
            );
            let level = match self.captured_name_scopes.get(&key) {
                Some((_, level)) => level.clone(),
                None => {
                    let bindings = self.captured_scope_bindings(scope);
                    let level = std::rc::Rc::new(crate::bytecode::CapturedNames::new(
                        bindings,
                        outer.clone(),
                    ));
                    // The scope and the outer chain stay alive with the key, so neither
                    // address is reused while it names this entry.
                    self.captured_name_scopes
                        .insert(key, (scope.clone(), level.clone()));
                    level
                }
            };
            outer = Some(level);
        }
        outer
    }

    /// One captured scope's bindings, sorted by atom. Lexical markers own mutability and
    /// declaration conflicts, so ordinary captures are added only for names they leave.
    fn captured_scope_bindings(
        &mut self,
        scope: &capture_scope::CaptureScope,
    ) -> Vec<crate::bytecode::EvalBinding> {
        let capture = |slot: u16| crate::bytecode::EvalBindingLocation::Capture { depth: 0, slot };
        let mut visible = FxHashSet::default();
        let mut bindings = Vec::new();
        for (marker, slot) in scope.iter() {
            // Only a capture-prefixed name is copied: most scope entries are not captures.
            let Some((kind, name)) = LexicalBindingKind::ALL.into_iter().find_map(|kind| {
                self.atoms[marker as usize]
                    .strip_prefix(kind.capture_prefix())
                    .map(|name| (kind, name.to_owned()))
            }) else {
                continue;
            };
            let atom = self.atom(&name);
            if !visible.insert(atom) {
                continue;
            }
            let catch_marker = format!("{CATCH_CAPTURE_PREFIX}{name}");
            bindings.push(crate::bytecode::EvalBinding {
                atom,
                location: capture(slot),
                with_depth: 0,
                kind,
                declaration: if self
                    .atoms
                    .find(&catch_marker)
                    .is_some_and(|marker| scope.contains_key(&marker))
                {
                    crate::bytecode::EvalBindingDeclaration::CatchParameter
                } else {
                    crate::bytecode::EvalBindingDeclaration::Lexical
                },
            });
        }
        for (atom, slot) in scope.iter() {
            if self.atoms[atom as usize].contains('\0') || !visible.insert(atom) {
                continue;
            }
            bindings.push(crate::bytecode::EvalBinding {
                atom,
                location: capture(slot),
                with_depth: 0,
                kind: LexicalBindingKind::Mutable,
                declaration: crate::bytecode::EvalBindingDeclaration::Variable,
            });
        }
        crate::bytecode::EvalBinding::sort_by_atom(&mut bindings);
        bindings
    }

    fn annex_b_collisions(&self, body: &[Statement<'_>]) -> early::AnnexBCollisions {
        early::annex_b_lexical_collisions(body, &self.eval_annex_b_collisions)
    }

    fn new(
        source: &'a str,
        text: &'a str,
        mode: SpecializationMode,
        atom_prefix: &'a dyn AtomPrefix,
        private_name_ids: FxHashMap<(u32, u32), u32>,
    ) -> Self {
        let atoms = CompilerAtoms::new(atom_prefix, source.len() / 32);
        Self {
            source,
            text,
            source_comments: Vec::new(),
            declared_types: crate::bytecode::DeclaredTypes::default(),
            type_names: None,
            checked_parameters: typescript::checked_parameters(text),
            mode,
            root_strict: false,
            module_goal: false,
            capture_script_completion: false,
            eval_context: None,
            provided_text: None,
            eval_annex_b_collisions: early::AnnexBCollisions::default(),
            root_lexical_atoms: Vec::new(),
            atoms,
            capture_markers: FxHashMap::default(),
            private_name_references: 0,
            private_name_ids,
            prototype_unobservable: FxHashSet::default(),
            lazy_bodies: FxHashSet::default(),
            lazy_compiled: Vec::new(),
            checked_bodies: Vec::new(),
            lazy_arenas: Vec::new(),
            streamed: FxHashSet::default(),
            stream_candidates: FxHashSet::default(),
            body_stream: None,
            stream_run: None,
            #[cfg(feature = "verify-replay")]
            free_names: FxHashMap::default(),
            private_name_labels: FxHashMap::default(),
            private_name_overrides: FxHashMap::default(),
            constants: SiteTable::default(),
            constant_index: replay::ConstantIndex::default(),
            functions: replay::FunctionTable::default(), deferred_rewrite_code: 0, kept_rewrites: Vec::new(),
            errors: vec![],
            cache_sites: 0,
            name_shared_sites: NameSharedSites::default(),
            template_sites: 0,
            method_sites: SiteTable::default(),
            field_sites: SiteTable::default(),
            object_sites: SiteTable::default(),
            regexp_literal_sites: SiteTable::default(),
            literal_templates: SiteTable::default(),
            data_literal: None,
            superinstructions: SiteTable::default(),
            deferred_name_bindings: FxHashMap::default(),
            captured_name_scopes: FxHashMap::default(),
            function_exits: Default::default(),
            function_facts: Default::default(),
            dynamic_binding_scopes: 0,
            early_replays: FxHashMap::default(),
            invoked_functions: FxHashSet::default(),
            scratch: CompileScratch::default(),
            replay_calls: FxHashMap::default(),
            replay_target: None,
            source_type: oxc_span::SourceType::default(),
            replay_program: None,
            replayable_source: false,
            first_call: first_call::Stubs::default(),
            load_encoding: None,
        }
    }

    /// The next index of every table and counter.
    fn cursors(&self) -> replay::TableCursors {
        replay::TableCursors {
            functions: self.functions.len() as u32,
            constants: self.constants.len() as u32,
            cache_sites: self.cache_sites,
            template_sites: self.template_sites,
            method_sites: self.method_sites.len() as u32,
            field_sites: self.field_sites.len() as u32,
            object_sites: self.object_sites.len() as u32,
            regexp_literal_sites: self.regexp_literal_sites.len() as u32,
            literal_templates: self.literal_templates.len() as u32,
        }
    }

    /// Moves a replay to `cursors`, where the load stood at the same point.
    fn seek(&mut self, cursors: replay::TableCursors) {
        self.functions.seek(cursors.functions);
        self.constants.skip_to(cursors.constants as usize);
        self.cache_sites = cursors.cache_sites;
        self.template_sites = cursors.template_sites;
        self.method_sites.skip_to(cursors.method_sites as usize);
        self.field_sites.skip_to(cursors.field_sites as usize);
        self.object_sites.skip_to(cursors.object_sites as usize);
        self.regexp_literal_sites.skip_to(cursors.regexp_literal_sites as usize);
        self.literal_templates.skip_to(cursors.literal_templates as usize);
    }

    /// In a replay, the id of a function nested in the replayed one, past whose compilation
    /// the replay moves: the load compiled it, and it keeps that record.
    fn skip_replayed_child(&mut self) -> Option<u32> {
        let (target, exits) = self.replay_target?;
        let id = self.functions.len() as u32;
        if id == target {
            return None;
        }
        self.seek(exits[id as usize]);
        Some(id)
    }

    /// Notes where the tables stand once function `id` and those nested in it are compiled.
    fn record_function_exit(&mut self, id: u32) {
        if self.replay_target.is_some() {
            return;
        }
        let facts = self
            .functions
            .get(id)
            .map(|function| capture_layout::FunctionFacts::of(function, &self.atoms))
            .expect("a function's record exists once it is compiled");
        let cursors = self.cursors();
        self.function_exits.set(id, cursors);
        self.function_facts.set(id, facts);
    }

    /// Notes a function expression called where it is made: `(function () {})()`, an arrow
    /// likewise, or either through `.call` or `.apply`; or one marked as about to run by
    /// parentheses, as engines read `(function () {})`. A replay of it would only compile it
    /// again a moment after its load did.
    fn note_invoked_callee(&mut self, callee: &Expression<'_>) {
        let callee = match callee.without_parentheses() {
            Expression::StaticMemberExpression(member)
                if matches!(member.property.name.as_str(), "call" | "apply") =>
            {
                member.object.without_parentheses()
            }
            callee => callee,
        };
        match callee {
            Expression::FunctionExpression(function) => {
                self.invoked_functions.insert(function.span.start);
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.invoked_functions.insert(arrow.span.start);
            }
            _ => {}
        }
    }

    /// Keeps how function `id` was compiled from `entry`, unless it resolved private names:
    /// their identities are numbered over the whole program, which a replay does not see.
    fn record_replay_call(
        &mut self,
        id: u32,
        entry: replay::TableCursors,
        private_names: (u64, usize),
        call: replay::ReplayCall,
    ) {
        let invoked = self
            .functions
            .get(id)
            .and_then(|function| function.source_span)
            .is_some_and(|span| self.invoked_functions.remove(&span.start));
        if self.replay_target.is_none()
            && self.replayable_source
            && !invoked && !self.encloses_nested_stub(id, self.function_exits.at(id).functions)
            && private_names == (self.private_name_references, self.private_name_ids.len())
        {
            self.replay_calls.insert(id, (entry, call));
            self.finish_early(id);
        }
    }


    /// Finishes replayable function `id` as soon as it is compiled and releases its record,
    /// so a program's records never all exist beside its tree. Only a function that, with
    /// every function nested in it, resolves names statically, inside no function that may
    /// give it binding sites: its layout then depends on nothing compiled after it.
    fn finish_early(&mut self, id: u32) {
        #[cfg(feature = "profile-aggregate")]
        if crate::vm::switches::profile_retain_bytecode() {
            // A function-level profile needs each replayable function's instruction records.
            return;
        }
        if !self.replayable_source || self.dynamic_binding_scopes != 0 {
            return;
        }
        let exit = self.function_exits.at(id).functions as usize;
        if self.function_facts.range(id, exit as u32)
            .iter()
            .any(|facts| facts.dynamic_access || facts.contains_with || facts.binding_sites)
        {
            return;
        }
        let Some(mut function) = self.functions.take(id) else {
            return;
        };
        let (entry, call) = self
            .replay_calls
            .remove(&id)
            .expect("an early-finished function has a replay call");
        let captured = capture_layout::captured_by(
            id as usize,
            self.function_facts.as_slice(),
            id as usize + 1..exit,
        );
        let layout = capture_layout::LayoutInputs {
            captured: capture_layout::complete_captured(
                captured,
                capture_layout::mapped_arguments(&function),
            ),
            dynamic_descendant: false,
            split_root: false,
            declared_captures: Vec::new(),
        };
        let arguments_slot = function.arguments_slot;
        capture_layout::apply_function(&mut function, id as usize, &layout, &self.atoms);
        let rewrite = self.start_rewrite(&mut function);
        self.settle_early(&rewrite, &mut function);
        if let Some(encoding) = &mut self.load_encoding {
            let tables = LoadTables {
                atoms: &self.atoms,
                functions: &self.functions,
                superinstructions: self.superinstructions.as_slice(),
            };
            encoding.add(id, &function, &tables);
        }
        self.early_replays.insert(
            id,
            replay::FunctionReplay {
                entry,
                call,
                layout,
                arguments_slot,
                rewrite,
            },
        );
        // Name bindings are resolved only for functions a dynamic lookup reaches.
        self.deferred_name_bindings.remove(&id);
        self.functions.release(id, function);
        // Its rewrites and the `undefined` constant may have added sites: a replay of the
        // function making it skips past them too.
        *self.function_exits.at_mut(id) = self.cursors();
    }

    /// A function declaration or expression, compiled from its syntax and `context`, which
    /// is all a replay of it needs besides the program.
    fn compile_plain_function(
        &mut self,
        function: &oxc_ast::ast::Function<'_>,
        mut context: replay::PlainFunctionContext,
    ) -> u32 {
        self.take_outer(function.span.start, |scopes, strict| (context.scopes, context.inherited_strict, context.with_depth) = (scopes, strict, 0));
        // A released body holds its stand-in wherever the function is compiled, and a node
        // may be compiled more than once (the right operand of `??`); the function parsed
        // again holds its own statements.
        if let Some(body) = &function.body
            && lazy_bodies::is_stand_in(body)
        {
            return self.compile_released_function(function, body.span.start, context);
        }
        if self.replayable_source {
            context.scopes = context.scopes.recording();
        }
        let entry = self.cursors();
        let private_names = (self.private_name_references, self.private_name_ids.len());
        let params = FunctionCompiler::params_from_formals(&function.params, self);
        let body = function.body.as_ref().map_or(&[][..], |body| body.statements.as_slice());
        let name = function
            .id
            .as_ref()
            .map(|id| id.name.as_str())
            .or_else(|| context.default_export.then_some("default"));
        // A named function expression binds its name in its own scope; a body that cannot
        // observe the binding needs none, and runs without a frame where nothing else needs
        // one (`CallPlan::registers_only`).
        let name_binding = if context.expression {
            function
                .id
                .as_ref()
                .filter(|id| early::function_may_reference(&function.params, body, id.name.as_str()))
                .map(|id| self.atom(id.name.as_str()))
        } else {
            None
        };
        let strict = context.inherited_strict
            || function.body.as_ref().is_some_and(|body| {
                body.directives
                    .iter()
                    .any(|directive| directive.directive == "use strict")
            });
        let id = self.compile_function(
            name,
            &params,
            FunctionBody::Statements(body),
            &context.scopes,
            Some(context.parent),
            FunctionOptions {
                source_span: self.source_span(function.span),
                defaults: Some(&function.params),
                name_binding,
                async_function: function.r#async,
                generator: function.generator,
                with_depth: context.with_depth,
                strict,
                prototype_unobservable: context.prototype_unobservable,
                ..FunctionOptions::default()
            },
        );
        #[cfg(feature = "verify-replay")]
        self.verify_predicted_captures(id, function.span.start, &context.scopes);
        context.scopes = context.scopes.into_memo();
        self.record_replay_call(id, entry, private_names, replay::ReplayCall::Plain(context));
        id
    }

    /// A function whose body was parsed lazily, compiled from its source parsed again alone
    /// (`lazy_bodies`). A body that fails an early check alone fails the compilation, which
    /// then compiles the program eagerly; until then its stand-in compiles in its place.
    fn compile_lazy_function(
        &mut self,
        function: &oxc_ast::ast::Function<'_>,
        context: replay::PlainFunctionContext,
    ) -> u32 {
        if self.replay_target.is_none()
            && (self.streamed.contains(&function.span.start) || self.stream_candidates.contains(&function.span.start) && self.body_streams(function, &context))
            && let Some(id) = self.compile_streamed_function(function, &context)
        {
            return id;
        }
        let snippet = &self.text[function.span.start as usize..function.span.end as usize];
        let (text, offset) = lazy_bodies::text_alone(
            (self.leading_doc(function.span.start), snippet),
            replay::plain_function_wrapper(&context),
            context.inherited_strict,
            self.source_type,
        );
        let mut allocator = self.lazy_arenas.pop().unwrap_or_else(|| lazy_bodies::arena(&text));
        let id = self.compile_parsed_again(&allocator, &text, offset, function, context);
        lazy_bodies::release_after(&mut allocator);
        self.lazy_arenas.push(allocator);
        id
    }

    /// `compile_lazy_function` of a function whose body streams (`statement_stream`): its
    /// text with an empty body parsed again, and its body compiled from its text a run of
    /// statements at a time; None where the text with an empty body does not parse alone.
    /// Whether the body of `function`, a stream candidate checked without its tree, streams.
    /// The run a chunk compiles of a streamed function after its first: the function at
    /// `span`, compiled with its header alone, takes its body from where the run starts.
    fn later_run(&self, parent: Option<u32>, span: Option<crate::bytecode::SourceSpan>) -> Option<statement_stream::StreamedBody> {
        let run = self.stream_run?;
        let (from, body) = run.from?;
        (parent.is_some() && span?.start == run.start).then(|| {
            run.found.set(Some((body, None)));
            statement_stream::StreamedBody { start: from, end: body.close, strict: body.strict, one_run: true }
        })
    }

    /// Where the run after the one a chunk compiled starts, if any.
    fn found_next_run(&self, next: Option<u32>) {
        if let Some(run) = self.stream_run
            && let Some((body, _)) = run.found.get()
        {
            run.found.set(Some((body, next)));
        }
    }

    fn body_streams(&self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> bool {
        let Some(body) = function.body.as_ref().map(|body| body.span) else { return false };
        statement_stream::body_streams(self.text, self.source_type, statement_stream::StreamedBody { start: body.start + 1, end: body.end - 1, strict: context.inherited_strict, one_run: false })
    }

    fn compile_streamed_function(&mut self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> Option<u32> {
        let body = function.body.as_ref()?.span;
        let strict = context.inherited_strict;
        let header = format!("{}{{}}", &self.text[function.span.start as usize..body.start as usize]);
        let (text, offset) = lazy_bodies::text_alone((self.leading_doc(function.span.start), &header), replay::plain_function_wrapper(context), strict, self.source_type);
        let allocator = Allocator::default();
        let mut reparsed = lazy_bodies::reparse(
            &allocator,
            &text,
            offset,
            function.span.start.wrapping_sub(offset),
            self.source_type,
            lazy_bodies::Nesting::Recorded(&FxHashSet::default()),
            self.body_type_names(),
        )
        .ok()?;
        let again = reparsed.function_mut().filter(|again| again.span.start == function.span.start)?;
        again.span.end = function.span.end;
        again.body.as_mut()?.span = body;
        let one_run = self.stream_run.is_some_and(|run| run.start == function.span.start && run.from.is_none());
        if one_run && let Some(run) = self.stream_run {
            run.found.set(Some((statement_stream::StreamedRun { open: body.start, close: body.end - 1, strict }, None)));
        }
        self.body_stream = Some(statement_stream::StreamedBody { start: body.start + 1, end: body.end - 1, strict, one_run });
        let again = reparsed.function().expect("checked above");
        Some(self.compile_plain_function(again, context.clone()))
    }

    /// `compile_lazy_function`, with `text` (the function's source at `offset`, wrapped)
    /// parsed in `allocator`.
    fn compile_parsed_again(
        &mut self,
        allocator: &Allocator,
        text: &str,
        offset: u32,
        function: &oxc_ast::ast::Function<'_>,
        context: replay::PlainFunctionContext,
    ) -> u32 {
        let saved_names = self.body_type_names();
        let nesting = if self.replay_target.is_some() {
            lazy_bodies::Nesting::Recorded(&self.lazy_bodies)
        } else {
            lazy_bodies::Nesting::Parse(&mut self.first_call.free_names, self.first_call.direct)
        };
        let reparsed = lazy_bodies::reparse(
            allocator,
            text,
            offset,
            function.span.start.wrapping_sub(offset),
            self.source_type,
            nesting,
            saved_names,
        );
        let reparsed = match reparsed {
            Ok(reparsed) => Some(reparsed),
            Err(lazy_bodies::ReparseFailure::StackExhausted) => {
                self.errors.push(Diagnostic::stack_exhausted(self.source));
                return self.compile_plain_function(function, context);
            }
            Err(lazy_bodies::ReparseFailure::Diagnostics) => None,
        };
        let Some(mut reparsed) = reparsed.filter(|reparsed| reparsed.function().is_some_and(|again| again.span == function.span)) else {
            self.reject(function.span, LAZY_FUNCTION_BODY_NOT_ALONE);
            return self.compile_plain_function(function, context);
        };
        // The body's declarations, admitted with the program's names, join the program's declared
        // environment at its positions; its annotations resolve against the names they were admitted with.
        let names = reparsed.declared.type_names.clone();
        self.declared_types.append(std::mem::take(&mut reparsed.declared));
        #[cfg(test)]
        REPARSED_BODIES.with(|count| count.set(count.get() + 1));
        let again = reparsed.function().expect("checked above");
        self.with_type_names(names, |this| this.compile_plain_function(again, context))
    }

    /// Runs `compile` with `names` as the type names its annotations resolve against (`type_names`):
    /// a lazily parsed body's or a first-call chunk's, whose program's names are in view only through
    /// the names saved with it.
    pub(super) fn with_type_names<T>(&mut self, names: Option<std::rc::Rc<[crate::bytecode::SavedTypeName]>>, compile: impl FnOnce(&mut Self) -> T) -> T {
        let outer = std::mem::replace(&mut self.type_names, names);
        let result = compile(self);
        self.type_names = outer;
        result
    }

    /// The type names a body parsed again here resolves against: the names in effect where it lies, else
    /// the program's saved ones.
    pub(super) fn body_type_names(&self) -> Option<std::rc::Rc<[crate::bytecode::SavedTypeName]>> {
        self.type_names.clone().or_else(|| self.declared_types.type_names.clone())
    }

    /// The JSDoc comment that types the function starting at `start` from outside its text
    /// (`DeclaredTypes::leading_doc`), or nothing.
    pub(super) fn leading_doc(&self, start: u32) -> &str {
        self.declared_types.leading_doc(start).and_then(|(from, to)| self.text.get(from as usize..to as usize)).unwrap_or("")
    }

    /// The names a function's formal annotations resolve against (`typescript::declared_parameters`).
    pub(super) fn formal_type_names(&self) -> Option<typescript::TypeNames<'static>> {
        self.type_names.as_ref().map(|saved| typescript::TypeNames::from_saved(Some(saved.clone())))
    }

    /// Checks, under `QUENCH_TRACE=lazy-facts`, that the free names of function `id` (starting
    /// at `start`), resolved through the scopes it closes over, are the captures its compiled
    /// subtree made of bindings outside it: what a lazily compiled function would report to
    /// the layouts around it without its code (TOD-196).
    #[cfg(feature = "verify-replay")]
    fn verify_predicted_captures(&self, id: u32, start: u32, scopes: &capture_scope::ScopeChain) {
        // Only a replayable source's functions can compile lazily.
        if !crate::trace::Channel::LazyFacts.on() || self.replay_target.is_some() || !self.replayable_source {
            return;
        }
        let Some(names) = self.free_names.get(&start) else {
            eprintln!("lazy-facts missing start={start}");
            return;
        };
        let chain = capture_scope::ScopeChain::new(scopes.levels().to_vec());
        let mut predicted: Vec<(u32, u16)> = names
            .iter()
            .filter_map(|name| self.atoms.find(name))
            .filter_map(|atom| chain.find(atom))
            .map(|(depth, slot)| (depth as u32 + 1, slot))
            .collect();
        predicted.sort_unstable();
        predicted.dedup();
        let id = id as usize;
        let exit = self.function_exits.at(id as u32).functions as usize;
        let depth_below = |mut function: usize| {
            let mut depth = 0u32;
            while function != id {
                function = self.function_facts.at(function as u32).parent.expect("a descendant has a parent") as usize;
                depth += 1;
            }
            depth
        };
        let mut actual: Vec<(u32, u16)> = (id..exit)
            .flat_map(|function| {
                let depth = depth_below(function);
                self.function_facts.at(function as u32)
                    .captures
                    .iter()
                    .filter(move |(distance, _)| *distance > depth)
                    .map(move |(distance, slot)| (distance - depth, *slot))
            })
            .collect();
        actual.sort_unstable();
        actual.dedup();
        let verdict = if predicted == actual {
            "same"
        } else if actual.iter().all(|capture| predicted.contains(capture)) {
            "superset"
        } else {
            "missing"
        };
        // The names of captures the prediction lacks, from the scope level holding each slot.
        let unpredicted: Vec<String> = actual
            .iter()
            .filter(|capture| !predicted.contains(capture))
            .map(|&(distance, slot)| {
                chain
                    .levels()
                    .get(distance as usize - 1)
                    .and_then(|level| level.iter().find(|(_, s)| **s == slot).map(|(atom, _)| self.atoms[*atom as usize].to_string()))
                    .unwrap_or_else(|| format!("?{distance}:{slot}"))
            })
            .collect();
        // Bindings the compilation found that the free names do not name: a memo built
        // from the free names would answer such a lookup "unbound".
        // A capture marker (`capture_marker`) is asked for by the name it marks.
        let source_of = |atom: Atom| {
            self.capture_markers
                .iter()
                .find_map(|(&(_, source), &marker)| (marker == atom).then_some(source))
                .unwrap_or(atom)
        };
        let unasked: Vec<String> = scopes
            .recorded_atoms()
            .into_iter()
            .map(|atom| self.atoms[source_of(atom) as usize].to_string())
            .filter(|name| !names.iter().any(|free| **free == **name))
            .collect();
        let verdict = if unasked.is_empty() { verdict } else { "unasked" };
        eprintln!(
            "lazy-facts {verdict} source={} start={start} functions={} unpredicted={unpredicted:?} unasked={unasked:?} predicted={predicted:?} actual={actual:?}",
            self.source,
            exit - id
        );
    }

    /// A method of an object literal, compiled from its syntax and `context`.
    fn compile_replayable_object_method(
        &mut self,
        function: &oxc_ast::ast::Function<'_>,
        mut context: replay::ObjectMethodContext,
    ) -> u32 {
        // A class method's chunk compiles it as an object literal's (`ChunkText::of`).
        self.adopt_outer_method(&mut context);
        if self.replayable_source {
            context.scopes = context.scopes.recording();
        }
        let entry = self.cursors();
        let private_names = (self.private_name_references, self.private_name_ids.len());
        let params = FunctionCompiler::params_from_formals(&function.params, self);
        let body = function
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let id = self.compile_function(
            None,
            &params,
            FunctionBody::Statements(body),
            &context.scopes,
            Some(context.parent),
            FunctionOptions {
                defaults: Some(&function.params),
                source_span: self.source_span(context.property_span),
                async_function: function.r#async,
                generator: function.generator,
                with_depth: context.with_depth,
                non_constructible: true,
                prototype_unobservable: false,
                super_home: true,
                super_home_atom: Some(context.home_atom),
                strict: Self::function_is_strict(context.inherited_strict, function),
                ..FunctionOptions::default()
            },
        );
        context.scopes = context.scopes.into_memo();
        self.record_replay_call(id, entry, private_names, replay::ReplayCall::ObjectMethod(context));
        id
    }

    /// Whether `function` is strict: inherited strict, or its body opens with a `"use strict"` directive.
    fn function_is_strict(inherited: bool, function: &oxc_ast::ast::Function<'_>) -> bool {
        inherited
            || function.body.as_ref().is_some_and(|body| {
                body.directives
                    .iter()
                    .any(|directive| directive.directive == "use strict")
            })
    }

    /// A class's method, getter or setter, compiled from its syntax and `context`.
    fn compile_replayable_class_method(
        &mut self,
        method: &oxc_ast::ast::MethodDefinition<'_>,
        mut context: replay::ClassMethodContext,
    ) -> u32 {
        if self.replayable_source {
            context.scopes = context.scopes.recording();
        }
        let entry = self.cursors();
        let private_names = (self.private_name_references, self.private_name_ids.len());
        let id = self.compile_class_method(
            method,
            Some(context.element_span),
            &context.scopes,
            context.parent,
            None,
            None,
            false,
            context.home_atom,
            context.with_depth,
        );
        context.scopes = context.scopes.into_memo();
        // A decorator's expression is part of the class element's text and belongs to the
        // code around the class, so the method alone does not parse from it.
        if method.decorators.is_empty() {
            self.record_replay_call(id, entry, private_names, replay::ReplayCall::ClassMethod(context));
        }
        id
    }

    /// An arrow function, compiled from its syntax and `context`.
    fn compile_replayable_arrow(
        &mut self,
        arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        mut context: replay::ArrowContext,
    ) -> u32 {
        self.take_outer(arrow.span.start, |scopes, strict| (context.scopes, context.inherited_strict, context.with_depth) = (scopes, strict, 0));
        if self.replayable_source {
            context.scopes = context.scopes.recording();
        }
        let entry = self.cursors();
        let private_names = (self.private_name_references, self.private_name_ids.len());
        let id = if lazy_bodies::is_released_arrow(arrow) { self.compile_released_arrow(arrow, &context) } else { self.compile_arrow_function(arrow, &context) };
        context.scopes = context.scopes.into_memo();
        self.record_replay_call(id, entry, private_names, replay::ReplayCall::Arrow(context));
        id
    }

    /// Locate a delimiter between OXC-owned expression boundaries. Comment text
    /// is excluded using OXC's own spans, including comments containing delimiters.
    fn source_delimiter(&self, start: u32, end: u32, delimiter: char) -> u32 {
        source_syntax::delimiter(self.text, &self.source_comments, start, end, delimiter)
    }

    /// The source offset a position records: a byte of the text, clamped to its end. Its line
    /// and column are derived from it only when something asks (`ProgramSource::position`).
    fn source_offset(&self, offset: u32) -> u32 {
        offset.min(u32::try_from(self.text.len()).unwrap_or(u32::MAX))
    }

    /// Emits every function of `program`: all the compiler reads of its tree. `finish` makes
    /// the residual program from what this leaves, after the caller has dropped the tree.
    fn program(
        mut self,
        program: &Program<'_>,
        module_goal: bool,
        inherited_strict: bool,
    ) -> Result<(Self, ModuleParts), Vec<Diagnostic>> {
        self.source_comments = program.comments.iter().map(|comment| comment.span).collect();
        self.module_goal = module_goal;
        let module_requests = if module_goal {
            module_requests(&program.body)
        } else {
            Vec::new()
        };
        let module_imports = if module_goal {
            module_import_bindings(&program.body)
        } else {
            Vec::new()
        };
        let module_link_plan = module_goal
            .then(|| Engine::module_link_plan(&program.body, self.source))
            .flatten();
        self.root_strict = module_goal
            || inherited_strict
            || program
                .directives
                .iter()
                .any(|directive| directive.directive == "use strict");
        let async_module = module_goal && has_top_level_await(&program.body);
        let (functions, errors) = early::program_early_errors(program, self.root_strict);
        // One slot per source function and the root, reserved once: grown by doubling, the
        // slots of a large script stood up to twice its functions beside its tree.
        self.functions.reserve_exact(functions + 1);
        for error in errors {
            self.reject(Span::default(), error);
        }
        let mut global_lexical_names = Vec::new();
        let mut global_immutable_names = Vec::new();
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(declaration)
                    if is_lexical_binding_declaration(declaration.kind) =>
                {
                    for declarator in &declaration.declarations {
                        early::collect_pattern_names(&declarator.id, &mut global_lexical_names);
                        if is_immutable_binding_declaration(declaration.kind) {
                            early::collect_pattern_names(
                                &declarator.id,
                                &mut global_immutable_names,
                            );
                        }
                    }
                }
                Statement::ClassDeclaration(declaration) => {
                    if let Some(identifier) = &declaration.id {
                        global_lexical_names.push(identifier.name.to_string());
                    }
                }
                Statement::ImportDeclaration(import) if import.phase.is_none() => {
                    for binding in module_import_bindings(std::slice::from_ref(statement)) {
                        global_lexical_names.push(binding.local.clone());
                        global_immutable_names.push(binding.local);
                    }
                }
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(declaration)
                        if is_lexical_binding_declaration(declaration.kind) =>
                    {
                        for declarator in &declaration.declarations {
                            early::collect_pattern_names(&declarator.id, &mut global_lexical_names);
                            if is_immutable_binding_declaration(declaration.kind) {
                                early::collect_pattern_names(
                                    &declarator.id,
                                    &mut global_immutable_names,
                                );
                            }
                        }
                    }
                    Declaration::ClassDeclaration(declaration) => {
                        if let Some(identifier) = &declaration.id {
                            global_lexical_names.push(identifier.name.to_string());
                        }
                    }
                    _ => {}
                },
                Statement::ExportDefaultDeclaration(export) => {
                    if let oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) =
                        &export.declaration
                        && let Some(identifier) = &class.id
                    {
                        global_lexical_names.push(identifier.name.to_string());
                    }
                }
                _ => {}
            }
        }
        let global_lexical_atoms: Vec<_> = global_lexical_names
            .iter()
            .map(|name| self.atom(name))
            .collect();
        let global_immutable_atoms: Vec<_> = global_immutable_names
            .iter()
            .map(|name| self.atom(name))
            .collect();
        // The root's lexicals are known before its nested functions compile, which read them by
        // declaration index (`LoadGlobalLexical`). Only a script's own are global.
        self.root_lexical_atoms = if module_goal || self.eval_context.is_some() {
            Vec::new()
        } else {
            global_lexical_atoms.clone()
        };
        self.compile_function(
            None,
            &[],
            FunctionBody::Statements(&program.body),
            &capture_scope::ScopeChain::new(Vec::new()),
            None,
            FunctionOptions {
                defaults: None,
                directive_completion: program.directives.last().map(|directive| &directive.expression),
                source_span: None,
                name_binding: None,
                async_function: async_module,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
                prototype_unobservable: false,
                class_initializer: self
                    .eval_context
                    .is_some_and(|context| context.class_initializer),
                instance_fields: None,
                instance_private_methods: None,
                super_static: false,
                super_home: self
                    .eval_context
                    .is_some_and(|context| context.home_atom.is_some()),
                super_home_atom: self.eval_context.and_then(|context| context.home_atom),
                rest_override: false,
                implicit_body: None,
                strict: self.root_strict,
                with_depth: 0,
            },
        );
        let global_function_names: Vec<_> = program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => function
                    .id
                    .as_ref()
                    .map(|identifier| identifier.name.to_string()),
                _ => None,
            })
            .collect();
        let global_function_atoms = global_function_names
            .iter()
            .map(|name| self.atom(name))
            .collect();
        let mut global_var_names = early::collect_var_names(&program.body);
        let mut global_annex_b_var_names = Vec::new();
        for statement in &program.body {
            if let Statement::FunctionDeclaration(function) = statement
                && let Some(identifier) = &function.id
            {
                global_var_names.push(identifier.name.to_string());
            }
        }
        let strict_script = inherited_strict
            || program
                .directives
                .iter()
                .any(|directive| directive.directive == "use strict");
        if !strict_script {
            let collisions = self.annex_b_collisions(&program.body);
            let direct_functions: FxHashSet<_> = program
                .body
                .iter()
                .filter_map(|statement| match statement {
                    Statement::FunctionDeclaration(function) => Some(function.span.start),
                    _ => None,
                })
                .collect();
            global_annex_b_var_names = early::annex_b_function_names(&program.body)
                .into_iter()
                .filter(|(span, _)| !direct_functions.contains(span) && !collisions.contains(*span))
                .map(|(_, name)| name)
                .collect();
            global_var_names.extend(global_annex_b_var_names.iter().cloned());
        }
        global_var_names.sort();
        global_var_names.dedup();
        let global_var_atoms = global_var_names
            .iter()
            .map(|name| self.atom(name))
            .collect();
        let global_annex_b_var_atoms = global_annex_b_var_names
            .iter()
            .map(|name| self.atom(name))
            .collect();
        if let Some(root) = self.functions.first_mut() {
            root.set_global_declarations(
                crate::bytecode::GlobalDeclarations::new(
                    global_lexical_atoms,
                    global_var_atoms,
                    global_function_atoms,
                    global_annex_b_var_atoms,
                    global_immutable_atoms,
                )
                .boxed(),
            );
            // A strict eval's vars are its own bindings, not the global object's.
            if !self.module_goal && !(self.eval_context.is_some() && strict_script) {
                fn lower_global_var(
                    op: Op,
                    slot: impl FnOnce() -> usize,
                    local_atoms: &[Atom],
                    global_var_atoms: &[Atom],
                ) -> Option<Op> {
                    let replacement = if op == LOCAL_LOAD_OP {
                        ENV_LOAD_OP
                    } else if op == LOCAL_STORE_OP {
                        ENV_STORE_OP
                    } else {
                        return None;
                    };
                    local_atoms
                        .get(slot())
                        .is_some_and(|atom| global_var_atoms.contains(atom))
                        .then_some(replacement)
                }
                let global_var_atoms = root.global_var_atoms().to_vec();
                for instruction in &mut root.code {
                    if let Some(op) = lower_global_var(
                        instruction.op(),
                        || instruction.local_slot(),
                        &root.local_atoms,
                        &global_var_atoms,
                    ) {
                        instruction.set_op(op);
                    }
                }
                for instruction in &mut root.wide {
                    if let Some(op) = lower_global_var(
                        instruction.op(),
                        || instruction.local_slot(),
                        &root.local_atoms,
                        &global_var_atoms,
                    ) {
                        instruction.set_op(op);
                    }
                }
            }
        }
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() {
            // Per-function-count structures, at the point the whole module has been lowered:
            // the tables that grow with the number of functions rather than with the source.
            let records = self.functions.len();
            eprintln!(
                "{{\"kind\":\"quench-compiler-memory\",\"name\":{:?},\"functions\":{},\"early_replays\":{},\"early_replay_capacity\":{},\"function_exits\":{},\"function_facts\":{},\"atoms_added\":{},\"atom_index\":{},\"atom_index_capacity\":{},\"deferred_name_bindings\":{}}}",
                self.source,
                records,
                self.early_replays.len(),
                self.early_replays.capacity(),
                self.function_exits.capacity() * std::mem::size_of::<replay::TableCursors>(),
                self.function_facts.capacity() * std::mem::size_of::<capture_layout::FunctionFacts>(),
                self.atoms.len(),
                self.atoms.index_len(),
                self.atoms.index_capacity(),
                self.deferred_name_bindings.len(),
            );
        }
        Ok((
            self,
            ModuleParts {
                requests: module_requests,
                imports: module_imports,
                link_plan: module_link_plan,
            },
        ))
    }

    /// The residual program of the functions `program` emitted.
    fn finish(mut self, module: ModuleParts) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let ModuleParts {
            requests: module_requests,
            imports: module_imports,
            link_plan: module_link_plan,
        } = module;
        // A function finished during the load has only its header left; the passes below
        // finish the others.
        let mut released = FxHashMap::default();
        let mut functions: Vec<Option<BcFunction>> = std::mem::take(&mut self.functions).into_entries().into_iter().enumerate()
            .map(|(id, entry)| match entry {
                replay::FunctionEntry::Compiled(function) => Some(*function),
                replay::FunctionEntry::Released(function) => {
                    released.insert(id, function);
                    None
                }
                replay::FunctionEntry::Reserved => unreachable!("function {id} was never compiled"),
            })
            .collect();
        let facts = std::mem::take(&mut self.function_facts).into_vec();
        let dynamic = capture_layout::dynamic_name_resolution(&facts);
        let reachable = capture_layout::dynamic_name_scopes(&facts, &dynamic);
        // A replay compiles one function and records no facts; it keeps no program. A class
        // constructor runs its instance initializer on its own environment
        // (`Vm::initialize_this_binding`); a released constructor's link is not at hand.
        // A private name is found by walking the environment chain from the activation that
        // uses it (`Vm::private_home`), and no capture records that walk, so a program that names
        // any private member reads each closure's whole chain.
        let names_private_members = self.private_name_references != 0;
        let closure_reads = (facts.len() == functions.len()).then(|| {
            let initializers: Vec<capture_layout::InstanceInitializer> = functions
                .iter()
                .enumerate()
                .map(|(id, function)| match function {
                    Some(function) => capture_layout::InstanceInitializer::Known(function.instance_initializer()),
                    None if released.get(&id).is_some_and(|released| released.header.is_class_constructor) => {
                        capture_layout::InstanceInitializer::Unknown
                    }
                    None => capture_layout::InstanceInitializer::Known(None),
                })
                .collect();
            let dynamic_scopes = if names_private_members { vec![true; facts.len()] } else { reachable.clone() };
            std::rc::Rc::new(capture_layout::closure_reads(&facts, &dynamic_scopes, &initializers))
        });
        self.resolve_name_bindings(&mut functions, &reachable);
        let mut layout_inputs = capture_layout::inputs(&facts, &dynamic, |id| {
            functions[id].as_ref().and_then(capture_layout::mapped_arguments)
        });
        // A module body splits around the bindings its link plan addresses; one without a
        // plan keeps every binding in its environment.
        if self.module_goal
            && let (Some(plan), Some(inputs), Some(Some(root))) =
                (&module_link_plan, layout_inputs.first_mut(), functions.first())
        {
            inputs.split_root = true;
            inputs.captured.extend(
                root.module_environment_slots(plan, &module_imports, |atom| self.atoms.get(atom as usize)),
            );
            inputs.captured.sort_unstable();
            inputs.captured.dedup();
        } else if !self.module_goal
            && let (Some(inputs), Some(Some(root))) = (layout_inputs.first_mut(), functions.first())
            && !inputs.dynamic_descendant
            && !root.has_dynamic_scope_access()
            && root.binding_sites.is_empty()
        {
            // A Script's or eval's root splits around its global declarations; without
            // dynamic name resolution nothing else names its block bindings.
            inputs.split_root = true;
            inputs.captured.extend(root.global_declaration_slots(self.eval_context.is_some()));
            inputs.captured.sort_unstable();
            inputs.captured.dedup();
        }
        // A replayable function resolves every name statically and no dynamic lookup reaches
        // its bindings, so nothing outside its own compilation and layout decides its code.
        let mut replays: FxHashMap<u32, replay::FunctionReplay> = std::mem::take(&mut self.early_replays);
        if self.replayable_source {
            for (id, (entry, call)) in std::mem::take(&mut self.replay_calls) {
                if dynamic[id as usize] || reachable[id as usize] {
                    continue;
                }
                let Some(function) = functions[id as usize].as_ref() else {
                    continue;
                };
                replays.insert(
                    id,
                    replay::FunctionReplay {
                        entry,
                        call,
                        layout: layout_inputs[id as usize].clone(),
                        arguments_slot: function.arguments_slot,
                        rewrite: replay::Rewrite::Loaded(replay::RewriteStart { field_sites: 0, superinstructions: 0 }),
                    },
                );
            }
        }
        for (id, function) in functions.iter_mut().enumerate() {
            if let Some(function) = function {
                capture_layout::apply_function(function, id, &layout_inputs[id], &self.atoms);
            }
        }
        for (id, slot) in functions.iter_mut().enumerate() {
            let replay = replays.get_mut(&(id as u32)).filter(|_| !released.contains_key(&id));
            if let Some(function) = self.rewrite_replayable(id, slot, replay) {
                released.insert(id, function);
            }
        }
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() && released.is_empty() {
            let records: Vec<BcFunction> = functions.iter().flatten().cloned().collect();
            capture_profile::report(&records);
        }
        // Lowering adds no table entries, so a record waits for it until its first read
        // (`ResidualProgram::function`), which most of a bundle's functions never have; the
        // header's dispatch class is settled here. The root, which runs as soon as the program
        // does, lowers here, and every record does under `verify-replay`, as each replay it
        // compares with is lowered.
        self.declared_types.apply_parameters(&mut functions);
        let undefined_constant = self.constant(Constant::Undefined);
        let lower_at_load = lowers_at_load();
        let lowered_here = |id: usize| lower_at_load || id == crate::vm::ROOT_FUNCTION_ID as usize;
        for (id, function) in functions.iter_mut().enumerate() {
            let Some(function) = function else { continue };
            if lowered_here(id) {
                let solved = self.lower_function(function, undefined_constant);
                function.register_roots = liveness::register_roots_from(
                    function,
                    self.method_sites.as_slice(),
                    self.field_sites.as_slice(),
                    self.superinstructions.as_slice(),
                    solved,
                );
            } else {
                self.settle_dispatch_only(function);
            }
            function.shrink_to_fit();
        }
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() && released.is_empty() {
            let records: Vec<BcFunction> = functions
                .iter()
                .enumerate()
                .filter_map(|(id, function)| Some((id, function.clone()?)))
                .map(|(id, mut function)| {
                    if !lowered_here(id) {
                        lower_settled(
                            &mut function,
                            &LoweringTables {
                                object_sites: self.object_sites.as_slice(),
                                methods: self.method_arguments(),
                                field_sites: self.field_sites.as_slice(),
                                superinstructions: self.superinstructions.as_slice(),
                                specialized: self.mode == SpecializationMode::Enabled,
                                undefined_constant,
                            },
                            |atom| self.atoms.get(atom),
                            &mut CompileScratch::default(),
                        );
                    }
                    function
                })
                .collect();
            register_profile::report(
                &records,
                self.method_sites.as_slice(),
                self.field_sites.as_slice(),
                self.superinstructions.as_slice(),
            );
        }
        let room = std::mem::take(&mut self.first_call.room);
        let rewrites_pend = self.rewrites_pend(&room);
        let code = self.deferred_rewrite_code + room.code();
        let cache_capacity = room.cache_capacity(self.cache_sites);
        // `AppendVec::with_headroom` takes each table's exact length, so these need no shrinking first.
        let (method_sites, method_arguments) = Self::flatten_method_sites(self.method_sites.into_vec());
        let atoms = self.atoms.table();
        let replay_atoms = self.first_call.keep_replay(replays.is_empty())
            .then(|| replay::ProgramAtomIndex::new(self.atoms.interned(), &atoms));
        let constants = self.constants.into_vec();
        let field_sites = self.field_sites.into_vec();
        // Built by pushes, so each may hold doubling slack the program would keep for good.
        let mut module_requests = module_requests;
        module_requests.shrink_to_fit();
        let mut module_imports = module_imports;
        module_imports.shrink_to_fit();
        let mut declared_types = std::mem::take(&mut self.declared_types);
        declared_types.declared.shrink_to_fit();
        declared_types.enum_constants.shrink_to_fit();
        let program = ResidualProgram {
            specialized: self.mode == SpecializationMode::Enabled,
            // Eval code is code compiled for an eval; a host script that only captures its
            // completion value keeps Script declaration semantics.
            kind: if self.module_goal {
                crate::bytecode::ProgramKind::Module
            } else if self.eval_context.is_some() {
                crate::bytecode::ProgramKind::Eval
            } else {
                crate::bytecode::ProgramKind::Script
            },
            module_requests,
            module_imports,
            module_link_plan,
            declared_types: (!declared_types.is_empty()).then(|| Box::new(declared_types)),
            source_name: self.source.to_owned(),
            source: Some(self.first_call.kept_source.cloned().unwrap_or_else(|| crate::bytecode::ProgramSource {
                text: source_text::kept_or_provided(self.text, self.eval_context.is_some(), self.provided_text.take()),
                typescript: self.source_type.is_typescript(),
                base: 0,
                line_offset: 0,
                column_offset: 0,
                layout: Default::default(),
            })),
            record: crate::bytecode::ProgramRecord {
                atoms,
                constants: crate::bytecode::append_vec::AppendVec::with_headroom(constants, room.constants()),
                call_sites: Default::default(),
                call_pcs: Default::default(),
            },
            // Collected into a table of its own size: an in-place collect would keep the
            // records' larger allocation.
            functions: {
                let mut slots = Vec::with_capacity(functions.len());
                slots.extend(functions.into_iter().enumerate().map(|(id, function)| {
                    match function {
                        Some(function) if lowered_here(id) => FunctionSlot::compiled(function),
                        Some(function) => self.first_call.templates.unlowered(id, function),
                        None => FunctionSlot::released(released[&id].header),
                    }
                }));
                crate::bytecode::append_vec::AppendVec::with_headroom(slots, room.functions())
            },
            cache_sites: crate::bytecode::SiteCount::with_capacity(self.cache_sites, cache_capacity),
            method_sites: crate::bytecode::append_vec::AppendVec::with_headroom(method_sites, room.method_sites()),
            method_arguments: crate::bytecode::append_vec::AppendVec::with_headroom(method_arguments, room.method_arguments()),
            field_sites: crate::bytecode::append_vec::AppendVec::with_headroom(field_sites, code / FIELD_SITE_WINDOW + room.field_sites() + 1),
            object_sites: crate::bytecode::append_vec::AppendVec::with_headroom(self.object_sites.into_vec(), room.object_sites()),
            regexp_literal_sites: crate::bytecode::append_vec::AppendVec::with_headroom(self.regexp_literal_sites.into_vec(), room.regexp_literal_sites()),
            literal_templates: crate::bytecode::append_vec::AppendVec::with_headroom(self.literal_templates.into_vec(), room.literal_templates()),
            superinstructions: crate::bytecode::append_vec::AppendVec::with_headroom(self.superinstructions.into_vec(), code / SUPERINSTRUCTION_WINDOW + 1),
            rewrite_ranges: replay::RewriteRanges::new(rewrites_pend, std::mem::take(&mut self.kept_rewrites)),
            replay: replay_atoms.map(|atoms| {
                std::rc::Rc::new(replay::ProgramReplay {
                    source_type: self.source_type,
                    root_strict: self.root_strict,
                    module_goal: self.module_goal,
                    lazy_bodies: lazy_bodies::sorted(std::mem::take(&mut self.lazy_compiled)),
                    checked_bodies: lazy_bodies::sorted(std::mem::take(&mut self.checked_bodies)),
                    specialized: self.mode == SpecializationMode::Enabled,
                    template_sites: self.template_sites,
                    atoms,
                    exits: std::mem::take(&mut self.function_exits).into_vec().into_boxed_slice(),
                    direct_stubs: self.first_call.direct,
                    chunk_name_sites: replay::ChunkNameSites::new(match &self.name_shared_sites {
                        NameSharedSites::Load { next, .. } => *next,
                        _ => crate::bytecode::SHARED_UNCACHED_SITE,
                    }),
                    name_shared_sites: match &mut self.name_shared_sites {
                        NameSharedSites::Load { sites, .. } => std::mem::take(sites),
                        NameSharedSites::Replay(sites) | NameSharedSites::Chunk(sites, _) => (*sites).clone(),
                    },
                    functions: replays,
                    #[cfg(feature = "verify-replay")]
                    released: released
                        .iter()
                        .map(|(id, function)| (*id as u32, function.record.clone()))
                        .collect(),
                    views: replay::ReplayViews::default(),
                })
            }),
            first_call_scopes: crate::bytecode::append_vec::AppendVec::with_headroom(std::mem::take(&mut self.first_call.nested), room.nested_stubs()),
            encoded_records: None,
            validated: Default::default(),
            undefined_constant: Default::default(),
            closure_reads,
            wasm_lazy: None,
            load_encoding: crate::bytecode::LoadRecords::new(self.load_encoding.take()),
        };
        #[cfg(feature = "profile-aggregate")]
        debug_assert!(crate::profile::instruction_word_domains_fit(&program));
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Memory.on() {
            program_profile::ProgramFootprint::report(&program);
        }
        Ok(program)
    }

    fn atom(&mut self, text: &str) -> Atom {
        self.atoms.intern(text)
    }

    /// The atom marking a captured binding of `source` with `kind` (`None` for a catch
    /// parameter's), recorded so the capture scope finds it without spelling it again.
    fn capture_marker(
        &mut self,
        kind: Option<crate::bytecode::LexicalBindingKind>,
        source: Atom,
    ) -> Atom {
        if let Some(marker) = self.capture_markers.get(&(kind, source)) {
            return *marker;
        }
        let prefix = kind.map_or(CATCH_CAPTURE_PREFIX, |kind| kind.capture_prefix());
        let marker = self.atom(&format!("{prefix}{}", &self.atoms[source as usize]));
        self.capture_markers.insert((kind, source), marker);
        marker
    }

    fn private_name_atom(&mut self, span: Span) -> Atom {
        self.private_name_references += 1;
        let name = match self.private_name_ids.get(&(span.start, span.end)) {
            Some(id) => {
                let label = self.private_name_label(span, *id);
                let identity = format!("\0quench:private:{}:{id}:{label}", self.source);
                if let Some(override_name) = self.private_name_overrides.get(id) {
                    override_name.clone()
                } else {
                    identity
                }
            }
            None => {
                self.reject(span, "OXC did not resolve a private name identity");
                "\0quench:private:unresolved".to_owned()
            }
        };
        self.atom(&name)
    }

    fn private_name_is_overridden(&self, atom: Atom) -> bool {
        self.private_name_overrides
            .values()
            .any(|name| self.atoms.find(name) == Some(atom))
    }

    fn source_span(&self, span: Span) -> Option<crate::bytecode::SourceSpan> {
        self.text.get(span.start as usize..span.end as usize)?;
        Some(crate::bytecode::SourceSpan {
            start: span.start,
            end: span.end,
        })
    }

    fn private_name_label(&self, span: Span, id: u32) -> String {
        if let Some(label) = self.private_name_labels.get(&(span.start, span.end)) {
            return label.clone();
        }
        let label = |span: Span| {
            let source = self.text.get(span.start as usize..span.end as usize)?;
            let source = source.strip_prefix('#').unwrap_or(source);
            (!source.is_empty()).then(|| source.to_owned())
        };
        label(span)
            .or_else(|| {
                self.private_name_ids
                    .iter()
                    .find_map(|((start, end), candidate)| {
                        (*candidate == id)
                            .then(|| label(Span::new(*start, *end)))
                            .flatten()
                    })
            })
            .unwrap_or_default()
    }

    fn reserve_auto_accessor_name(&mut self, span: Span) {
        let id = self
            .private_name_ids
            .values()
            .copied()
            .max()
            .map_or(0, |current| current + 1);
        self.private_name_ids.insert((span.start, span.end), id);
        // Auto-accessor storage has an identity but no source-visible private
        // identifier. Direct eval must not project it into its lexical names.
        self.private_name_labels
            .insert((span.start, span.end), String::new());
    }

    fn private_name_text(&mut self, span: Span) -> String {
        let atom = self.private_name_atom(span);
        self.atoms[atom as usize].to_string()
    }

    fn constant(&mut self, value: Constant) -> u32 {
        if let Some(index) = self.constant_index.get(ConstantKey::from(&value), &self.constants) {
            // A replay finds every constant; one at its cursor was added here at load.
            if index as usize == self.constants.len() {
                self.constants.push(value);
            }
            return index;
        }
        let index = self.constants.len() as u32;
        self.constant_index.insert(ConstantKey::from(&value), index);
        self.constants.push(value);
        index
    }

    fn regexp_literal_site(&mut self, pattern_constant: u32, flags_constant: u32, receiver: bool) -> u32 {
        let index = self.regexp_literal_sites.len() as u32;
        self.regexp_literal_sites
            .push(crate::bytecode::RegExpLiteralSite {
                pattern_constant,
                flags_constant,
                receiver,
            });
        index
    }

    fn constant_run(&mut self, values: Vec<Constant>) -> u32 {
        let start = self.constants.len() as u32;
        for value in values {
            let index = self.constants.len() as u32;
            // A replay's index already holds every constant, so it is only read.
            if self.constant_index.get(ConstantKey::from(&value), &self.constants).is_none() {
                self.constant_index.insert(ConstantKey::from(&value), index);
            }
            self.constants.push(value);
        }
        start
    }

    fn cache_site(&mut self) -> u16 {
        if self.cache_sites < crate::bytecode::UNIQUE_CACHE_SITES {
            self.cache_sites += 1;
            return self.cache_sites - 1;
        }
        // Past the unique sites the program spans the whole site space: the name-shared
        // sites and the shared uncached one.
        self.cache_sites = crate::bytecode::SHARED_UNCACHED_SITE + 1;
        crate::bytecode::SHARED_UNCACHED_SITE
    }

    /// The site a named load or store of `atom` shares past the unique sites
    /// (`NAME_SHARED_CACHE_SITES`): the load gives one on first use, and a replay reads the
    /// load's, so a replayed function makes the record its load made. A load that used them
    /// all keeps the shared uncached site.
    fn name_shared_cache_site(&mut self, access: NamedAccess, atom: u32) -> u16 {
        const SHARED: u16 = crate::bytecode::SHARED_UNCACHED_SITE;
        match &mut self.name_shared_sites {
            NameSharedSites::Replay(sites) => sites.get(&(access, atom)).copied().unwrap_or(SHARED),
            NameSharedSites::Chunk(load, chunks) => load.get(&(access, atom)).copied().unwrap_or_else(|| chunks.site(access, atom)),
            NameSharedSites::Load { sites, next } => {
                if let Some(&site) = sites.get(&(access, atom)) {
                    return site;
                }
                if *next == SHARED {
                    return SHARED;
                }
                let site = *next;
                *next += 1;
                sites.insert((access, atom), site);
                site
            }
        }
    }

    /// The cache site instruction `op` keeps for `site`: a named field load or store emitted
    /// with the shared uncached site takes its name's shared one instead.
    fn emitted_cache_site(&mut self, op: Op, base: Register, site: Register, imm: u32) -> Register {
        if site != crate::bytecode::SHARED_UNCACHED_SITE {
            return site;
        }
        let access = if op.traits().has(Traits::FIELD_LEAF) && base != crate::bytecode::FieldBase::NESTED {
            NamedAccess::Load
        } else if op.traits().has(Traits::SLOPPY_STORE.union(Traits::STRICT_STORE)) {
            NamedAccess::Store
        } else {
            return site;
        };
        self.name_shared_cache_site(access, imm)
    }

    fn template_site(&mut self) -> u32 {
        let site = self.template_sites;
        self.template_sites = site
            .checked_add(1)
            .expect("template-site address space exhausted");
        site
    }

    fn flatten_method_sites(sites: Vec<MethodSiteSpec>) -> (Vec<MethodSite>, Vec<Register>) {
        let argument_capacity = sites.iter().map(|site| site.2.len()).sum();
        let mut arguments = Vec::with_capacity(argument_capacity);
        let metadata = sites
            .into_iter()
            .map(|(atom, cache, values, receiver_path)| {
                let argument_start = arguments.len() as u32;
                let argument_count = values.len() as u16;
                arguments.extend(values);
                MethodSite {
                    atom,
                    cache,
                    argument_start,
                    argument_count,
                    receiver_path: receiver_path.into(),
                }
            })
            .collect();
        (metadata, arguments)
    }
    fn reject(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            kind: DiagnosticKind::Compilation,
            source: self.source.into(),
            message: message.into(),
            span,
        });
    }
    fn reject_syntax(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            kind: DiagnosticKind::Syntax,
            source: self.source.into(),
            message: message.into(),
            span,
        });
    }
    pub(super) fn collect_body_lexical_bindings(
        &mut self,
        body: &[Statement<'_>],
    ) -> Vec<(Atom, LexicalBindingKind)> {
        let mut bindings = Vec::new();
        for statement in body {
            let declaration = match statement {
                Statement::VariableDeclaration(declaration) => Some(declaration.as_ref()),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(declaration) => Some(declaration.as_ref()),
                    _ => None,
                },
                _ => None,
            };
            let mut names = Vec::new();
            let kind = if let Some(declaration) = declaration {
                if !is_lexical_binding_declaration(declaration.kind) {
                    continue;
                }
                for item in &declaration.declarations {
                    early::collect_pattern_names(&item.id, &mut names);
                }
                if is_immutable_binding_declaration(declaration.kind) {
                    LexicalBindingKind::Immutable
                } else {
                    LexicalBindingKind::Mutable
                }
            } else {
                let class = match statement {
                    Statement::ClassDeclaration(class) => Some(class.as_ref()),
                    Statement::ExportDeclaration(export) => match &export.declaration {
                        Declaration::ClassDeclaration(class) => Some(class.as_ref()),
                        _ => None,
                    },
                    Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                        oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                            Some(class.as_ref())
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(identifier) = class.and_then(|class| class.id.as_ref()) {
                    names.push(identifier.name.to_string());
                }
                LexicalBindingKind::Mutable
            };
            for name in names {
                let atom = self.atom(&name);
                if !bindings.iter().any(|(candidate, _)| *candidate == atom) {
                    bindings.push((atom, kind));
                }
            }
        }
        bindings
    }
    /// The local slots a function body keeps in registers, and the promoted slots a nested
    /// function may write (clobbered). `eligible` states the activation's own conditions;
    /// the body's fixed-register facts and the register budget decide the rest. Ordinary
    /// functions and arrows share this plan. `own_arguments` is a function's strictness and
    /// parameters, whose own `arguments` binding a body may read; an arrow has none, so any
    /// `arguments` it names keeps its locals in frame slots.
    #[allow(clippy::too_many_arguments)]
    fn promoted_local_plan(
        &mut self,
        eligible: bool,
        async_body: bool,
        arguments_only: Option<usize>,
        own_arguments: Option<(bool, &[Atom])>,
        body: &[Statement<'_>],
        expression_body: Option<&Expression<'_>>,
        locals: &[Atom],
        function_scope: &FxHashSet<Atom>,
        lexical_atoms: &[Atom],
        self_binding_slot: Option<u16>,
    ) -> PromotedLocalPlan {
        let register_local_facts = early::fixed_register_local_facts(body, expression_body);
        let rebinds_parameter = |parameters: &[Atom]| {
            parameters
                .iter()
                .any(|atom| register_local_facts.bound_names.contains(&self.atoms[*atom as usize]))
        };
        // A strict `arguments` object is a copy of the values passed, which no parameter
        // write reaches; a sloppy one aliases the parameters (`Function::arguments_alias_parameters`)
        // unless the body rebinds no parameter and cannot observe the mapping
        // (`FixedRegisterLocalFacts::arguments_mapping_unobserved`).
        let unaliased_arguments = eligible
            && own_arguments.is_some_and(|(strict, parameters)| {
                !strict
                    && !rebinds_parameter(parameters)
                    && register_local_facts.arguments_mapping_unobserved(
                        parameters.iter().map(|atom| &self.atoms[*atom as usize]),
                    )
            });
        let arguments_allow_promotion = match (register_local_facts.arguments, own_arguments) {
            (early::ArgumentsUse::None, _) => true,
            (_, None) => false,
            (_, Some((strict, _))) => strict || unaliased_arguments,
        };
        // A suspension saves the frame's registers with its locals, so an async or generator
        // body's locals promote as any other body's do.
        // A body that makes functions keeps its locals for the capture layout to split
        // (`capture_layout`, `frame_registers`), but a parameter none of them names is the
        // body's alone: it arrives in a register as in a body that makes none, where the
        // later passes, which leave parameters in their slots, would keep it in the frame.
        // An async or generator body keeps its whole frame for its closures (`capture_layout`
        // splits none).
        let parameters_only = !async_body && register_local_facts.makes_closures_only()
            && own_arguments.is_some_and(|(_, parameters)| register_local_facts.closures_spare(
                parameters.iter().map(|atom| &self.atoms[*atom as usize])));
        if !eligible || !(register_local_facts.safe || parameters_only) || !arguments_allow_promotion {
            return PromotedLocalPlan { unaliased_arguments, ..Default::default() };
        }
        let parameter_count = own_arguments.map_or(0, |(_, parameters)| parameters.len());
        let arguments_atom = self.atom("arguments");
        let mut latest_local_slot = FxHashMap::default();
        for (slot, atom) in locals.iter().copied().enumerate() {
            if let Ok(slot) = u16::try_from(slot) {
                latest_local_slot.insert(atom, slot);
            }
        }
        let mut promoted_local_slots = latest_local_slot
            .into_iter()
            .filter_map(|(atom, slot)| {
                let promotable = if let Some(arguments) = arguments_only {
                    // The hidden argument slots lead the locals, one per parameter.
                    usize::from(slot) < arguments
                } else if parameters_only {
                    usize::from(slot) < parameter_count
                        && own_arguments.is_some_and(|(_, parameters)| parameters.contains(&atom))
                        && atom != arguments_atom
                        && self_binding_slot != Some(slot)
                } else {
                    function_scope.contains(&atom)
                        && !lexical_atoms.contains(&atom)
                        && atom != arguments_atom
                        && self_binding_slot != Some(slot)
                        && !self.atoms[atom as usize].starts_with('\0')
                };
                promotable.then_some(slot)
            })
            .collect::<Vec<_>>();
        // Promoted locals take the lowest registers; past half the register domain a
        // function keeps its locals in frame slots so its temporaries still fit.
        if promoted_local_slots.len() > usize::from(PROMOTED_LOCAL_REGISTER_BUDGET) {
            promoted_local_slots.clear();
        }
        promoted_local_slots.sort_unstable();
        let clobbered_local_slots = promoted_local_slots
            .iter()
            .copied()
            .filter(|&slot| {
                register_local_facts
                    .nested_written_names
                    .contains(&self.atoms[locals[usize::from(slot)] as usize])
            })
            .collect::<Vec<_>>();
        PromotedLocalPlan { promoted_local_slots, clobbered_local_slots, unaliased_arguments }
    }

    fn compile_function(
        &mut self,
        name: Option<&str>,
        params: &[String],
        body: FunctionBody<'_>,
        scopes: &capture_scope::ScopeChain,
        parent: Option<u32>,
        options: FunctionOptions<'_>,
    ) -> u32 {
        let (body, expression_body) = match body {
            FunctionBody::Statements(body) => (body, None),
            FunctionBody::Expression(expression) => (&[][..], Some(expression)),
        };
        let streamed = self.body_stream.take().or_else(|| self.later_run(parent, options.source_span));
        if let Some(id) = self.skip_replayed_child() {
            return id;
        }
        let id = self.functions.reserve();
        let body_lexicals = self.collect_body_lexical_bindings(body);
        let lexical_atoms: Vec<_> = body_lexicals.iter().map(|(atom, _)| *atom).collect();
        let rest_alias = rest_alias::rest_alias(&options, (parent.is_none(), streamed.is_some(), expression_body.is_some()), body);
        let params: Vec<Atom> = params[..params.len().saturating_sub(usize::from(rest_alias.is_some()))].iter().map(|name| self.atom(name)).collect();
        let mut locals = params.clone();
        if let Some(formal) = options.defaults {
            for name in FunctionCompiler::parameter_bound_names(formal) {
                let atom = self.atom(&name);
                if !locals.contains(&atom) {
                    locals.push(atom);
                }
            }
        }
        let parameter_local_count = locals.len();
        let root_strict = self.root_strict || options.strict;
        let annex_b_collisions = self.annex_b_collisions(body);
        let mut function_scope =
            self.collect_locals(body, &mut locals, root_strict, parent.is_some(), &annex_b_collisions);
        // A script's own top-level lexicals are the realm's global lexical bindings, which the root
        // and its nested functions reach by declaration index (`LoadGlobalLexical`,
        // `StoreGlobalLexical`). The root keeps no local slot or environment slot for them, so no
        // closure captures them. `root_lexical_atoms` is empty for a module and for an eval.
        if parent.is_none() && !self.root_lexical_atoms.is_empty() {
            locals.retain(|atom| !self.root_lexical_atoms.contains(atom));
            function_scope.retain(|atom| !self.root_lexical_atoms.contains(atom));
        }
        // Only a function-scope binding (a parameter, `var`, top-level declaration) shadows
        // the expression's own name; a block's or a catch clause's binding does not.
        let name_binding = options.name_binding.and_then(|source_name| {
            if function_scope.contains(&source_name) {
                None
            } else {
                let name = self.atoms[source_name as usize].to_string();
                let binding = self.atom(&format!(
                    "{name}{}{id}",
                    crate::bytecode::SELF_BINDING_MARKER
                ));
                locals.push(binding);
                Some((source_name, binding))
            }
        });
        let self_binding_slot = name_binding.as_ref().and_then(|(_, binding)| {
            locals
                .iter()
                .position(|atom| atom == binding)
                .and_then(|slot| u16::try_from(slot).ok())
        });
        let declared_functions = if parent.is_some() { self.declared_function_slots(body, &locals, params.len()) } else { Vec::new() };
        let arguments = self.atom("arguments");
        let parameter_shadows_arguments = locals[..parameter_local_count].contains(&arguments);
        let has_arguments_binding = locals.contains(&arguments);
        let parameter_arguments_slot = options
            .defaults
            .filter(|parameters| {
                has_arguments_binding
                    && !parameter_shadows_arguments
                    && !FunctionCompiler::binds_parameters_directly(parameters)
            })
            .map(|_| {
                let slot = locals.len() as u16;
                locals.push(self.atom("\0quench:parameter-arguments"));
                slot
            });
        let module_goal = self.module_goal;
        let capture_script_completion = parent.is_none() && self.capture_script_completion;
        if capture_script_completion && !root_strict {
            function_scope.retain(|atom| lexical_atoms.contains(atom));
        }
        let module_source = self.source;
        let arguments_slot = if let Some(slot) = parameter_arguments_slot {
            Some(slot)
        } else if parent.is_none() || options.class_initializer {
            // Field initializers and static blocks cannot name `arguments` (an early error),
            // so they have no binding to materialize.
            None
        } else if parameter_shadows_arguments {
            None
        } else if has_arguments_binding {
            locals
                .iter()
                .position(|atom| *atom == arguments)
                .map(|slot| slot as u16)
        } else {
            locals.push(arguments);
            function_scope.insert(arguments);
            Some((locals.len() - 1) as u16)
        };
        let environment_atoms = locals
            .iter()
            .copied()
            .filter(|atom| function_scope.contains(atom))
            .collect();
        let direct_eval = early::direct_eval_facts(options.defaults, body, expression_body);
        let dynamic_eval = direct_eval.eval;
        let simple_parameters = !options.rest_override
            && options
                .defaults
                .is_none_or(|formal| !FunctionCompiler::has_non_simple_parameters(formal));
        let direct_parameters = !options.rest_override
            && options.defaults.is_none_or(FunctionCompiler::binds_parameters_directly);
        // A list binding its names apart through patterns or defaults that reach no parameter
        // scope promotes just its hidden argument slots; its bindings, which open dead zones,
        // stay plain locals for the later promotion that tracks them (`frame_registers`).
        let hidden_parameters = !direct_parameters
            && !options.rest_override
            && options.defaults.is_some_and(FunctionCompiler::arguments_promotable_apart);
        let promote_plain_locals = parent.is_some()
            && (direct_parameters || hidden_parameters)
            && !options.derived_constructor
            && !options.class_initializer
            && options.implicit_body.is_none()
            && options.with_depth == 0
            // A constructor runs its instance fields' initializers inline, where a function
            // one makes captures the constructor's frame as one its body makes would.
            && !options.instance_fields.is_some_and(|fields| {
                fields
                    .iter()
                    .any(|field| class::class_field_value(*field).is_some_and(early::expression_makes_function))
            })
            // A nested function's own parameters and locals are its own in eval code and host
            // scripts too: names it does not declare resolve as they would without promotion.
            && !dynamic_eval;
        let PromotedLocalPlan { promoted_local_slots, clobbered_local_slots, unaliased_arguments } = self.promoted_local_plan(
            promote_plain_locals,
            options.async_function || options.generator,
            hidden_parameters.then_some(params.len()),
            Some((root_strict, &params)),
            body,
            expression_body,
            &locals,
            &function_scope,
            &lexical_atoms,
            self_binding_slot,
        );
        let binds_dynamically = direct_eval.binds_dynamically();
        self.dynamic_binding_scopes += u32::from(binds_dynamically);
        let names = self.formal_type_names();
        let declared_parameters = options.defaults
            .map_or_else(Vec::new, |formals| typescript::declared_parameters(formals, self.checked_parameters.as_deref(), names.as_ref()));
        let mut function = FunctionCompiler::new(
            self,
            locals,
            function_scope,
            annex_b_collisions,
            scopes.clone(),
            id,
            (options.super_static, options.super_home),
            options.async_function,
            options.generator,
            parameter_arguments_slot,
            parameter_local_count,
            options.with_depth,
            &promoted_local_slots,
            &clobbered_local_slots,
        );
        function.code.reserve(code_capacity_hint(options.source_span));
        if let Some((name, binding)) = name_binding {
            function.push_function_name_binding(name, binding);
        }
        function.super_home_atom = options.super_home_atom;
        function.declared_key = if parent.is_none() { Some(u32::MAX) } else { options.source_span.map(|span| span.start) };
        function.super_call_binds_this = options.derived_constructor
            || (parent.is_none()
                && function
                    .owner
                    .eval_context
                    .is_some_and(|context| context.super_calls));
        function.strict = root_strict;
        function.class_initializer = options.class_initializer;
        if capture_script_completion {
            let completion = function.reg();
            // Directive expressions precede the body and contribute its initial completion.
            let initial = if let Some(directive) = options.directive_completion {
                function.string_literal(directive)
            } else { function.literal(Constant::Undefined) };
            function.emit(REGISTER_MOVE, completion, initial, 0, 0);
            function.statement_completion = StatementCompletion::Track(completion);
        }
        function.dynamic_eval = dynamic_eval;
        function.literal_singletons = literal_singletons::Owner { parent, binds_dynamically, options: &options }.declarations(&function.owner.atoms, &params, body);
        if arguments_slot.is_some() && parameter_arguments_slot.is_none() {
            function.arguments_alias = rest_alias.clone().or_else(|| early::arguments_alias(options.defaults, body));
        }
        let mut lexical_slots: Vec<_> = lexical_atoms
            .iter()
            .filter_map(|atom| function.local_slots.get(atom).copied())
            .collect();
        if parent.is_none() && module_goal {
            for statement in body {
                if let Statement::ExportDefaultDeclaration(export) = statement
                    && let Some(binding) =
                        default_export_binding(&export.declaration, module_source)
                    && let Some(slot) = function.local_slots.get(&function.owner.atom(&binding))
                {
                    lexical_slots.push(*slot);
                }
            }
        }
        function.initialize_tdz_slots(lexical_slots);
        if let Some(fields) = options
            .instance_fields
            .filter(|_| !options.derived_constructor)
        {
            // A base class constructor's receiver is its own [[Construct]]'s, fresh; a derived
            // class's fields run in their initializer on the receiver `super()` returned.
            function.emit_instance_fields(
                fields,
                options.instance_private_methods.unwrap_or_default(),
                options.class_constructor,
            );
        }
        if let Some(defaults) = options.defaults {
            function.emit_parameter_bindings(defaults);
        }
        if let Some(slot) = parameter_arguments_slot
            && !lexical_atoms.contains(&arguments)
        {
            let value = function.reg();
            function.emit(LOCAL_LOAD_OP, value, 0, 0, u32::from(slot));
            function.store_atom(arguments, value);
        }
        let parameter_end_pc = function.code.len() as u32;
        function.push_body_lexical_bindings(&body_lexicals);
        function.emit_hoisted(body);
        if let Some(body) = options.implicit_body {
            function.emit_implicit_body(body);
        }
        match streamed {
            Some(streamed) => {
                if !function.streamed_body(streamed) {
                    function.owner.reject(Span::new(streamed.start, streamed.end), "SyntaxError: a streamed function body does not parse as checked");
                }
            }
            None => function.function_body_statements(body),
        }
        let result = if let Some(expression) = expression_body {
            function.expression(expression)
        } else {
            function
                .statement_completion
                .register()
                .unwrap_or_else(|| function.literal(Constant::Undefined))
        };
        function.emit(RETURN_OP, result, 0, 0, 0);
        // Block scopes add locals after the function's own were counted (`FunctionCompiler::new`):
        // a frame's local count is 16-bit, so one more would wrap it to an empty frame.
        if function.locals.len() > usize::from(u16::MAX) {
            function.owner.reject(Span::default(), "function exceeds the local-slot limit");
        }
        let captures_locals = function
            .code
            .iter()
            .any(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE))
            || function
                .wide
                .iter()
                .any(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE));
        let reads_binding = |op: Op| op.traits().has(Traits::FRAME_SLOT_READ);
        let arguments_slot = arguments_slot.filter(|slot| {
            (captures_locals && function.owner.arrows_reach_arguments(id, *slot))
                || function.dynamic_eval
                || function.code.iter().any(|instruction| {
                    reads_binding(instruction.op()) && instruction.local_slot() == usize::from(*slot)
                })
                || function.wide.iter().any(|instruction| {
                    reads_binding(instruction.op()) && instruction.local_slot() == usize::from(*slot)
                })
        });
        let parameter_atoms = options.defaults.map_or_else(Vec::new, |formal| {
            FunctionCompiler::parameter_bound_names(formal)
                .iter()
                .map(|name| function.owner.atom(name))
                .collect()
        });
        // Grown by doubling, these would keep their slack beside the syntax tree until the
        // whole program has compiled.
        trim_slack(&mut function.code);
        let name_bindings = function.name_bindings();
        let undefined_register = function.used_undefined_register();
        let environment_clones = function.per_site_environment_clones();
        let result = BcFunction {
            parent,
            name: name.map(|value| function.owner.atom(value)),
            inferred_name: None,
            is_arrow: false,
            self_binding_slot,
            source_span: options.source_span,
            params: params.len() as u16,
            length: options.defaults.map_or_else(
                || params.len().saturating_sub(usize::from(options.rest_override)),
                Self::formal_length,
            ) as u16,
            parameter_end_pc: if options.generator {
                parameter_end_pc
            } else {
                0
            },
            parameter_atoms: parameter_atoms.into(),
            rare: crate::bytecode::FunctionRare {
                declared_parameters: declared_parameters.into(),
                ..Default::default()
            }
            .boxed(),
            // A rest alias's list is no rest list of the function's own (`rest_alias`).
            rest: options.rest_override
                || options.defaults.is_some_and(|value| value.rest.is_some()) != rest_alias.is_some(),
            is_async: options.async_function,
            is_generator: options.generator,
            is_class_constructor: options.class_constructor,
            derived_constructor: options.derived_constructor,
            // Only a base class constructor's receiver is made by its own [[Construct]].
            constant_fields: if options.class_constructor && !options.derived_constructor {
                std::mem::take(&mut function.constant_fields).into()
            } else {
                Default::default()
            },
            super_home_atom: options.super_home_atom,
            constructible: !options.non_constructible
                && !options.async_function
                && !options.generator,
            prototype_unobservable: options.prototype_unobservable,
            class_initializer: options.class_initializer,
            parameter_eval_arguments_error: function.parameter_eval_arguments_error,
            unaliased_arguments, rest_alias: rest_alias.as_ref().and(arguments_slot).is_some(),
            arguments_slot,
            simple_parameters,
            strict: root_strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone().into(),
            environment_atoms,
            selective_capture_slots: None,
            declared_functions: crate::heap::ThinBoxSlice::from_slice(&declared_functions),
            declared_captures: Default::default(),
            local_registers: function.promoted_local_layout().into(),
            inherited_with_scope: options.with_depth != 0,
            lexical_atoms: lexical_atoms.into(),
            name_bindings,
            binding_sites: function.binding_sites.into(),
            source_positions: function.source_positions.freeze(),
            environment_clones,
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(), caches: Default::default(),
            lazy_flags: Default::default(),
            call_plan: Default::default(),
            handlers: function.handlers.into(),
            register_roots: Default::default(),
            undefined_register,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        self.functions.set(id, result);
        self.dynamic_binding_scopes -= u32::from(binds_dynamically);
        self.record_function_exit(id);
        id
    }

    /// Makes the anonymous function definition `id` with `name`, when it has none of its own
    /// and is no class constructor; whether it did. A replay skips the definition, which the
    /// load named exactly when it could.
    fn name_made_function(&mut self, id: u32, name: Atom) -> bool {
        if let Some(function) = self.functions.get_mut(id) {
            let nameable = function.name.is_none()
                && function.inferred_name.is_none()
                && !function.is_class_constructor;
            if nameable {
                function.inferred_name = Some(name);
            }
            return nameable;
        }
        let Some(header) = self.functions.header_without_record(id) else {
            return false;
        };
        if self.replay_target.is_some() {
            // A replay skipped the definition, which the load named exactly when it could.
            return header.name.is_none()
                && !header.is_class_constructor
                && header.inferred_name == Some(name);
        }
        // The load finished and released the definition before naming it.
        let nameable =
            header.name.is_none() && header.inferred_name.is_none() && !header.is_class_constructor;
        if nameable {
            self.functions.name_released(id, name);
            if let Some(encoding) = &mut self.load_encoding {
                encoding.name(id, name);
            }
        }
        nameable
    }

    /// Whether code nested in function `owner` can reach its `arguments` binding in local
    /// `slot`. Only an arrow shares its enclosing function's `arguments`; every other function
    /// binds its own. So the binding is reachable exactly where a function nested in `owner`
    /// through arrows alone captures the slot, or resolves names at run time (direct eval, a
    /// `with` statement or an enclosing one), which may denote it.
    fn arrows_reach_arguments(&self, owner: u32, slot: u16) -> bool {
        let facts = &self.function_facts;
        // Ids are reserved in pre-order, so only later ids nest in `owner`; uncompiled ones name no arrow.
        facts.after(owner + 1).iter().any(|function| {
            // The function levels between `function` and `owner`, along arrows alone.
            let mut distance = 0;
            let mut current = function;
            loop {
                if !current.is_arrow {
                    return false;
                }
                distance += 1;
                match current.parent {
                    Some(parent) if parent == owner => break,
                    Some(parent) => match facts.get(parent) {
                        Some(parent) => current = parent,
                        None => return false,
                    },
                    None => return false,
                }
            }
            function.dynamic_access
                || function.contains_with
                || function
                    .captures
                    .iter()
                    .any(|&(at, captured)| at as usize == distance && captured == slot)
        })
    }

    fn formal_length(params: &oxc_ast::ast::FormalParameters<'_>) -> usize {
        params
            .items
            .iter()
            .take_while(|item| item.initializer.is_none())
            .count()
    }

    /// The argument registers of each method site: at load the compiler's own sites; in a
    /// replay, which adds none, the finished program's.
    fn method_arguments(&self) -> &dyn liveness::MethodArguments {
        match self.replay_program {
            Some(program) => program,
            None => &self.method_sites,
        }
    }


    /// The specializing rewrites of one function, the only finishing pass that adds sites.
    fn rewrite_function(&mut self, function: &mut BcFunction) {
        if self.mode == SpecializationMode::Enabled {
            let methods: &dyn liveness::MethodArguments = match self.replay_program {
                Some(program) => program,
                None => &self.method_sites,
            };
            rewrite::apply(
                function,
                methods,
                &mut self.field_sites,
                &mut self.superinstructions,
                &mut self.scratch,
            );
        }
    }

    /// The finishing passes of one function after every function's rewrites: its dispatch
    /// class (`settle_dispatch`), then the passes the class leaves (`lower_settled`).
    fn lower_function(&mut self, function: &mut BcFunction, undefined_constant: u32) -> Option<Vec<liveness::RegisterSet>> {
        self.settle_dispatch(function);
        // Field borrows, not `method_arguments()`, so the scratch borrows alongside the tables.
        let methods: &dyn liveness::MethodArguments = match self.replay_program {
            Some(program) => program,
            None => &self.method_sites,
        };
        lower_settled(
            function,
            &LoweringTables {
                object_sites: self.object_sites.as_slice(),
                methods,
                field_sites: self.field_sites.as_slice(),
                superinstructions: self.superinstructions.as_slice(),
                specialized: self.mode == SpecializationMode::Enabled,
                undefined_constant,
            },
            |atom| self.atoms.get(atom),
            &mut self.scratch,
        )
    }

    /// The lowering passes the dispatch class reads (shared returns and strict sets), then
    /// the class itself (`FunctionHeader::dispatch`).
    fn settle_dispatch(&self, function: &mut BcFunction) {
        rewrite::return_through_shared_returns(function);
        rewrite::express_strict_sets(function);
        function.dispatch = if self.mode == SpecializationMode::Enabled {
            Self::dispatch_class(&function.code, &function.wide)
        } else {
            DispatchClass::General
        };
    }

    /// `settle_dispatch` for a function whose lowering ends there, which therefore drops the
    /// `Nop`s its rewrites left (`lower_settled` does so for the functions it lowers).
    fn settle_dispatch_only(&mut self, function: &mut BcFunction) {
        self.settle_dispatch(function);
        rewrite::remove_nops(function, &mut None, &mut self.scratch);
    }

    fn dispatch_class(code: &[Instr], wide: &[WideInstruction]) -> DispatchClass {
        if !wide.is_empty() {
            return DispatchClass::General;
        }
        let live = rewrite::live_count(code);
        let binaries = code
            .iter()
            .filter(|instruction| instruction.op().traits().has(Traits::BINARY_RESULT))
            .count();
        if live >= 32 && binaries * 3 >= live {
            DispatchClass::Numeric
        } else {
            DispatchClass::General
        }
    }

    /// Renames the operations of `function`'s plain local slots, which it returns.
    fn specialize_plain_local_operations(
        function: &mut BcFunction,
        superinstructions: &[crate::bytecode::Superinstruction],
        flow: &crate::bytecode::initialization::FlowCell,
    ) -> crate::bitset::BitSet {
        let plain_slots = function.plain_local_slots_in(superinstructions, flow);
        for instruction in &mut function.code {
            if let Some(plain) = plain_local_form(instruction.op())
                && plain_slots.contains(instruction.local_slot())
            {
                instruction.set_op(plain);
            }
        }
        for instruction in &mut function.wide {
            if let Some(plain) = plain_local_form(instruction.op())
                && plain_slots.contains(instruction.local_slot())
            {
                instruction.set_op(plain);
            }
        }
        plain_slots
    }
}
#[cfg(test)]
mod tests;

/// `Compiler::promoted_local_plan`: the local slots a body keeps in registers, those a nested
/// function may write, and whether its mapped `arguments` is unaliased
/// (`Function::unaliased_arguments`).
#[derive(Default)]
struct PromotedLocalPlan {
    promoted_local_slots: Vec<u16>,
    clobbered_local_slots: Vec<u16>,
    unaliased_arguments: bool,
}
