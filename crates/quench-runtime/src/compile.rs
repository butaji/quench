use crate::bytecode::{
    Atom, Constant, DispatchClass, FieldBase, FieldSite, Function as BcFunction, FunctionSlot, Instr,
    LexicalBindingKind, MethodSite, ModuleImportBinding, ModuleImportName, ModuleLinkPlan,
    ModuleRequest, ModuleRequestPhase, ObjectSite, Op, Operand, Register, ResidualProgram,
    SET_THIS_REGISTER, SourcePosition, Superinstruction, WideInstruction,
};
use oxc_allocator::Allocator;
use replay::SiteTable;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::{fmt, ops::Range};
/// The most registers a function's promoted locals may take: half of the register domain,
/// leaving the other half for the temporaries of any one statement.
const PROMOTED_LOCAL_REGISTER_BUDGET: Register = crate::bytecode::SET_THIS_REGISTER / 2;
mod annex_b_targets;
mod arrow;
mod ast;
mod atoms;
mod binding_time;
mod capture_layout;
#[cfg(feature = "profile-memory")]
mod capture_profile;
mod call_only;
mod capture_scope;
mod class;
mod early;
mod frame_registers;
mod lazy_bodies;
#[cfg(feature = "verify-replay")]
mod free_names;
mod syntax_message;
pub(crate) mod liveness;
mod locals;
mod numeric;
pub(crate) mod regexp;
pub(crate) mod program_profile;
#[cfg(feature = "profile-memory")]
mod register_profile;
pub(crate) mod replay;
mod rewrite;
mod sequence;
pub(crate) mod stack;
mod string;
mod template;
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
            DiagnosticKind::Compilation => &self.message,
            DiagnosticKind::StackExhausted => crate::stack::STACK_EXHAUSTED_MESSAGE,
        }
    }

    /// The message of the SyntaxError this diagnostic throws, without the class name some
    /// compiler messages carry.
    pub fn syntax_message(&self) -> &str {
        let message = self.message();
        message.strip_prefix(SYNTAX_ERROR_PREFIX).unwrap_or(message)
    }

    /// A parse failure, reported with V8's message at OXC's position.
    fn parse_failure(source_name: &str, source: &str, error: &oxc_diagnostics::OxcDiagnostic) -> Self {
        let offset = error.labels.first().map_or(0, |label| label.offset());
        Self {
            kind: DiagnosticKind::Compilation,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicFunctionKind {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

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

#[derive(Clone, Copy, Default)]
pub(crate) struct EvalContext {
    pub in_function: bool,
    pub home_atom: Option<Atom>,
    pub super_calls: bool,
    pub class_initializer: bool,
}

#[derive(Clone, Copy)]
enum ParsedProgramShape {
    Any,
    /// A script whose functions keep their records: a host script whose program keeps only
    /// part of its compiled text (a static body), from which they cannot compile again, or an
    /// application's entry, whose own functions mostly run.
    KeptRecords,
    DynamicFunction { parameter_list_end: usize },
    Eval(EvalContext, Option<Span>),
}

pub(crate) struct EvalRegExpLiteral {
    pub(crate) span: Range<usize>,
    pub(crate) flags: String,
}

/// The first characters, past white space, `/*…*/` and `//` comments and opening
/// parentheses, after which a Script of one parenthesized regular expression literal may
/// still begin: the literal's own `/`, and the starts of the comments this scan leaves to
/// the parser (Annex B's `<!--` and `-->`, and a hashbang `#!`).
const SINGLE_REGEXP_LITERAL_STARTS: [u8; 4] = [b'/', b'<', b'-', b'#'];

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
}

impl StaticModuleTextKey {
    fn of(source: &str, module_name: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        source.hash(&mut hasher);
        Self { length: source.len(), hash: hasher.finish(), name: module_name.into() }
    }
}
impl Engine {
    /// V8 excludes tagged templates from eval's compilation cache because each
    /// evaluation owns distinct template objects, including nested function sites.
    pub(crate) fn eval_has_tagged_template(source: &str) -> bool {
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
        Self::static_module_analysis_with(source, module_name, |analyze| analyze())
    }

    /// `static_module_analysis`, whose memo misses `obtain`, which analyzes the text with the
    /// function it is given or finds the analysis elsewhere (a host's compile cache).
    pub(crate) fn static_module_analysis_with(
        source: &str,
        module_name: &str,
        obtain: impl FnOnce(&dyn Fn() -> StaticModuleAnalysis) -> StaticModuleAnalysis,
    ) -> Rc<StaticModuleAnalysis> {
        thread_local! {
            static ANALYSES: std::cell::RefCell<FxHashMap<StaticModuleTextKey, Rc<StaticModuleAnalysis>>> =
                std::cell::RefCell::default();
        }
        let key = StaticModuleTextKey::of(source, module_name);
        if let Some(analysis) = ANALYSES.with(|analyses| analyses.borrow().get(&key).cloned()) {
            return analysis;
        }
        let analysis = Rc::new(obtain(&|| Self::analyze_static_module(source, module_name)));
        ANALYSES.with(|analyses| analyses.borrow_mut().insert(key, analysis.clone()));
        analysis
    }

    fn analyze_static_module(source: &str, module_name: &str) -> StaticModuleAnalysis {
        let unbounded = StaticModuleAnalysis { early_error: None, plan: None };
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, SourceType::mjs()).parse();
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
        StaticModuleAnalysis {
            early_error: None,
            plan: Some(StaticModulePlan {
                link_plan: Self::module_link_plan(&parsed.program.body, module_name),
                requests: module_requests(&parsed.program.body),
                has_top_level_await: has_top_level_await(&parsed.program.body),
            }),
        }
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

    pub(crate) fn static_module_throw(source: &str) -> Option<StaticModuleThrow> {
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
        let mut thrown = None;
        for statement in &parsed.program.body {
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
                                [argument] => Some(
                                    binding_time::expression(argument.as_expression()?)
                                        .static_value()?,
                                ),
                                _ => return None,
                            };
                            StaticModuleThrow::Error { name, message }
                        }
                        expression => StaticModuleThrow::Value(
                            binding_time::expression(expression).static_value()?,
                        ),
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
        let mut bindings = FxHashMap::default();
        let mut exports = Vec::new();
        for statement in &parsed.program.body {
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
                        expression => binding_time::expression(expression).static_value()?,
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

    pub fn specialize(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
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
        Self::specialize_script_shaped(source, name, atom_prefix, ParsedProgramShape::Any, released)
    }

    /// `specialize_script_with_atom_prefix` for a program whose functions keep their records:
    /// a host script whose program will keep only a static body of `source`, or an entry.
    pub(crate) fn specialize_kept_script_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_script_shaped(source, name, atom_prefix, ParsedProgramShape::KeptRecords, ReleasedRecords::Dropped)
    }

    fn specialize_script_shaped(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        shape: ParsedProgramShape,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            SourceType::script(),
            false,
            true,
            false,
            &[],
            &[],
            shape,
            released,
        )
    }
    pub fn specialize_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
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
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
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
        )
    }

    pub fn specialize_module_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Disabled)
    }
    /// Compile a module against an existing program's canonical atom prefix, specializing
    /// exactly when the VM it extends does, as a run-time script does.
    pub(crate) fn specialize_module_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode_and_private_names(
            source,
            name,
            SpecializationMode::of(atom_prefix),
            atom_prefix,
            SourceType::mjs(),
            true,
            false,
            false,
            &[],
            &[],
            ParsedProgramShape::Any,
            released,
        )
    }
    pub fn specialize_module(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Enabled)
    }
    /// Compile a Function-constructor program's source through the same OXC script pipeline
    /// used by ordinary source.
    pub(crate) fn specialize_function_constructor(
        source: &DynamicFunctionSource,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let expected_shape = ParsedProgramShape::DynamicFunction {
            parameter_list_end: source.parameter_list_end,
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
        expected_shape: ParsedProgramShape,
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
        )
    }
    #[allow(clippy::too_many_arguments)]
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
        expected_shape: ParsedProgramShape,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
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
            )
        };
        // Lazy function bodies apply to source a replay can parse again; a program that does
        // not compile with them compiles eagerly, which reports its own diagnostics.
        if matches!(expected_shape, ParsedProgramShape::Any)
            && lazy_bodies::enabled()
            && let Ok(program) = compile(true)
        {
            return Ok(program);
        }
        compile(false)
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
        expected_shape: ParsedProgramShape,
        lazy: bool,
        released: ReleasedRecords,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let mut parsed = lazy_bodies::parse(&allocator, &normalized, source_type, lazy.then_some(0));
        #[cfg(feature = "verify-replay")]
        if !lazy {
            lazy_bodies::verify(name, &normalized, &parsed.program);
        }
        if parsed.stack_exhausted {
            return Err(vec![Diagnostic::stack_exhausted(name)]);
        }
        if !parsed.diagnostics.is_empty() {
            return Err(parsed
                .diagnostics
                .iter()
                .map(|error| Diagnostic::parse_failure(name, &normalized, error))
                .collect());
        }
        stack::validate(&parsed.program).map_err(|()| vec![Diagnostic::stack_exhausted(name)])?;
        if let Some(error) = early::regexp_early_error(&parsed.program) {
            return Err(vec![match error {
                early::RegExpEarlyError::StackExhausted => Diagnostic::stack_exhausted(name),
                early::RegExpEarlyError::Syntax(message) => Diagnostic {
                    kind: DiagnosticKind::Compilation,
                    source: name.into(),
                    message,
                    span: Span::default(),
                },
            }]);
        }
        if capture_script_completion
            && source_type.is_commonjs()
            && let Some(span) = early::eval_return_outside_function(&parsed.program)
        {
            return Err(vec![Diagnostic {
                kind: DiagnosticKind::Compilation,
                source: name.into(),
                message: "SyntaxError: return outside function in eval".into(),
                span,
            }]);
        }
        if capture_script_completion {
            // CommonJS provides OXC's inherited new.target parsing context;
            // eval still has Script semantics for declarations and scopes.
            parsed.program.source_type = SourceType::script();
        }
        if let Some(span) = annex_b_targets::invalid_target(&parsed.program) {
            return Err(vec![Diagnostic {
                kind: DiagnosticKind::Compilation,
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
            ParsedProgramShape::Any | ParsedProgramShape::KeptRecords => true,
            ParsedProgramShape::Eval(_, expected_span) => {
                !wrapped_eval || eval_method_body(&mut parsed.program, ordinary_private_context)
                    .is_some_and(|body| Some(body.span) == expected_span)
            }
            ParsedProgramShape::DynamicFunction { parameter_list_end } => {
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
            return Err(vec![Diagnostic::unsupported(
                name,
                "SyntaxError: invalid dynamic Function source shape",
            )]);
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if !semantic.diagnostics.is_empty() {
            return Err(semantic
                .diagnostics
                .into_iter()
                .map(|error| Diagnostic {
                    kind: DiagnosticKind::Compilation,
                    source: name.into(),
                    message: format!("SyntaxError: {error}"),
                    span: Span::default(),
                })
                .collect());
        }
        let mut private_name_ids = private_name_ids(&semantic.semantic);
        let mut private_name_labels = private_name_labels(&semantic.semantic);
        let mut prototype_unobservable = call_only::prototype_unobservable_functions(&semantic.semantic, &parsed.program);
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
        drop(semantic);
        if lazy {
            // The lazily parsed bodies, checked and analyzed one at a time before code
            // generation (`lazy_bodies::body_facts`).
            let root_strict = module_goal
                || inherited_strict
                || parsed.program.directives.iter().any(|directive| directive.directive == "use strict");
            let first_private_id = private_name_ids.values().max().map_or(0, |id| id + 1);
            let Some(facts) = lazy_bodies::body_facts(
                &normalized,
                &parsed.program,
                &parsed.lazy_function_bodies,
                source_type,
                root_strict,
                first_private_id,
            ) else {
                return Err(vec![Diagnostic::unsupported(name, "SyntaxError: a lazily parsed function body does not check alone")]);
            };
            private_name_ids.extend(facts.private_name_ids);
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
                    oxc_allocator::Vec::new_in(&&allocator),
                );
                let directives = std::mem::replace(
                    &mut body.directives,
                    oxc_allocator::Vec::new_in(&&allocator),
                );
                parsed.program.body = statements;
                parsed.program.directives = directives;
                if let Some(span) = early::eval_return_outside_function(&parsed.program) {
                    return Err(vec![Diagnostic {
                        kind: DiagnosticKind::Compilation,
                        source: name.into(),
                        message: "SyntaxError: return outside function in eval".into(),
                        span,
                    }]);
                }
            }
            Some(context)
        } else {
            None
        };
        let mut compiler = Compiler::new(name, &normalized, mode, atom_prefix, private_name_ids);
        compiler.prototype_unobservable = prototype_unobservable;
        #[cfg(feature = "verify-replay")]
        {
            compiler.free_names = free_names;
        }
        compiler.source_type = source_type;
        // Eval code and dynamic functions are compiled from a wrapper, or a tree changed after
        // parsing, that their source text alone does not give back. A released function
        // compiles again from its replay on first call (`ResidualProgram::function`).
        compiler.replayable_source = matches!(expected_shape, ParsedProgramShape::Any);
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
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"quench-oxc-memory\",\"used\":{},\"capacity\":{}}}",
                allocator.used_bytes(),
                allocator.capacity(),
            );
        }
        // The tree is the largest structure of a compilation; the passes that finish the
        // program do not read it.
        drop(parsed);
        drop(allocator);
        let (compiler, module) = emitted?;
        let program = compiler.finish(module)?;
        #[cfg(feature = "verify-replay")]
        replay::verify(&program);
        Ok(program)
    }

    pub(crate) fn strict_eval_syntax_error(source: &str, in_function: bool) -> Option<String> {
        let strict_source = format!("'use strict';\n{source}");
        let source = strict_source.as_str();
        let allocator = Allocator::with_capacity(source.len().saturating_mul(2));
        let parsed = Parser::new(
            &allocator,
            source,
            if in_function {
                SourceType::cjs()
            } else {
                SourceType::script()
            },
        )
        .parse();
        if stack::validate_parsed(&parsed).is_err() {
            return None;
        }
        if !parsed.diagnostics.is_empty() {
            return Some(format!("{SYNTAX_ERROR_PREFIX}{}", syntax_message::parse_failure(source, &parsed.diagnostics[0])));
        }
        oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .first()
            .map(|diagnostic| format!("SyntaxError: {diagnostic}"))
    }
}

/// A point in compilation where the code about to run differs from the code that ran, which
/// an embedding may act on (`set_compile_phase_boundary`), as a host returns read-only pages
/// the next phase does not touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilePhase {
    /// A compilation has parsed and analysed its source; its code generation starts.
    CodeGeneration,
    /// A function compiles, or decodes from its program's kept bytes, on its first call: code
    /// that has not run before is about to.
    FirstCall,
    /// A compiled program is complete and its encoding for the host's compile cache starts:
    /// the parser's and code generator's work is over, and the encoder's begins.
    Encoding,
    /// A function's lane view derives from its bytecode (`dispatch_fast`) once its first runs
    /// are over: the deriver's code, and then the lane handlers, are about to run.
    LaneDerivation,
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
            Some(expression) => binding_time::expression(expression).static_value()?,
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
}

type NameSharedSiteMap = FxHashMap<(NamedAccess, u32), u16>;

struct Compiler<'a> {
    source: &'a str,
    text: &'a str,
    source_line_starts: std::borrow::Cow<'a, [usize]>,
    /// The line start partition `source_position` found last, which the next position,
    /// emitted from the same or a nearby node, most often falls on.
    last_source_line: std::cell::Cell<usize>,
    /// The UTF-16 excess partitions `source_position` found last, at a position and at its
    /// line's start.
    last_utf16_excess: [std::cell::Cell<usize>; 2],
    source_comments: Vec<Span>,
    /// Byte offset of each non-ASCII character with the running excess of
    /// UTF-8 bytes over UTF-16 units through it; columns count UTF-16 units.
    source_utf16_excess: std::borrow::Cow<'a, [(usize, usize)]>,
    mode: SpecializationMode,
    root_strict: bool,
    module_goal: bool,
    capture_script_completion: bool,
    eval_context: Option<EvalContext>,
    eval_annex_b_collisions: FxHashSet<u32>,
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
    /// The arenas lazily parsed functions are parsed again in, one per nesting of such
    /// functions, each reset and reused: an arena per function interleaved its blocks with
    /// the program's long-lived records, and the heap kept the holes between them.
    lazy_arenas: Vec<Allocator>,
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
    function_exits: Vec<replay::TableCursors>,
    /// What the program-wide layout analyses read of each compiled function.
    function_facts: Vec<capture_layout::FunctionFacts>,
    /// Functions being compiled whose bodies may give nested functions binding sites
    /// (`early::body_may_bind_dynamically`). While any is open, a function finished inside
    /// it may still resolve names dynamically, so none is released early.
    dynamic_binding_scopes: u32,
    /// The replays of functions finished and released during the load.
    early_replays: FxHashMap<u32, replay::FunctionReplay>,
    /// The source starts of function expressions called where they are made or marked as
    /// about to run, which keep their records (`note_invoked_callee`).
    invoked_functions: FxHashSet<u32>,
    /// How each replayable function was compiled, with the cursors it started from.
    replay_calls: FxHashMap<u32, (replay::TableCursors, replay::ReplayCall)>,
    /// In a replay, the function it recompiles and where the load left each function.
    replay_target: Option<(u32, &'a [replay::TableCursors])>,
    source_type: oxc_span::SourceType,
    /// In a replay, the finished program whose function it compiles.
    replay_program: Option<&'a ResidualProgram>,
    /// Whether the program's functions compile again from their text alone.
    replayable_source: bool,
    /// The compile-cache records of the functions released during the load, when its
    /// program will be encoded (`ReleasedRecords::Encoded`).
    load_encoding: Option<crate::bytecode::LoadEncoding>,
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

/// Byte offset of each non-ASCII character of `text` with the running excess of UTF-8 bytes
/// over UTF-16 units through it.
fn source_utf16_excess(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut table = Vec::new();
    let mut excess = 0;
    let mut offset = 0;
    while let Some(skipped) = ascii_prefix_len(&bytes[offset..]) {
        offset += skipped;
        let character = text[offset..].chars().next().expect("a non-ASCII byte starts a character");
        excess += character.len_utf8() - character.len_utf16();
        table.push((offset, excess));
        offset += character.len_utf8();
    }
    table
}

/// The bytes of `bytes` before its first non-ASCII byte, `None` when every one is ASCII:
/// a word of bytes at a time, as source text is nearly all ASCII.
fn ascii_prefix_len(bytes: &[u8]) -> Option<usize> {
    const WORD: usize = size_of::<u64>();
    const HIGH_BITS: u64 = u64::from_ne_bytes([0x80; WORD]);
    let mut words = bytes.chunks_exact(WORD);
    let mut offset = 0;
    for word in &mut words {
        if u64::from_ne_bytes(word.try_into().expect("a whole word")) & HIGH_BITS != 0 {
            break;
        }
        offset += WORD;
    }
    bytes[offset..].iter().position(|byte| !byte.is_ascii()).map(|position| offset + position)
}

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
    implicit_super: bool,
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

/// The index of the line of `starts` holding `offset`: the last line start at or before it, or
/// the first line when none is. `last`, the line found before, is tried first and updated.
fn source_line_index(starts: &[usize], last: &std::cell::Cell<usize>, offset: usize) -> usize {
    partition_point_from(starts, last, |start| *start <= offset).saturating_sub(1)
}

/// `slice.partition_point(holds)`, trying `last`, the point found before, first and updating
/// it: positions are asked for in nearly source order, so most fall where the last did.
fn partition_point_from<T>(slice: &[T], last: &std::cell::Cell<usize>, holds: impl Fn(&T) -> bool) -> usize {
    let point = last.get();
    let before = point.checked_sub(1).is_none_or(|index| slice.get(index).is_some_and(&holds));
    if point <= slice.len() && before && slice.get(point).is_none_or(|item| !holds(item)) {
        return point;
    }
    let point = slice.partition_point(holds);
    last.set(point);
    point
}

/// The program tables a function's lowering reads, final once every function's rewrites
/// have added their sites: at load the compiler's, after it the finished program's.
pub(crate) struct LoweringTables<'t> {
    pub(crate) methods: &'t dyn liveness::MethodArguments,
    pub(crate) field_sites: &'t [FieldSite],
    pub(crate) superinstructions: &'t [crate::bytecode::Superinstruction],
    pub(crate) specialized: bool,
    pub(crate) undefined_constant: u32,
}

/// The lowering passes after a function's dispatch class is settled (`settle_dispatch`):
/// numeric and plain-local specialization and frame registers. They add no table entries,
/// so they run equally at load and on a function's first read from its finished program.
pub(crate) fn lower_settled<'a>(
    function: &mut BcFunction,
    tables: &LoweringTables<'_>,
    atom_name: impl Fn(usize) -> Option<&'a str>,
) {
    let LoweringTables { methods, field_sites, superinstructions, specialized, undefined_constant } = *tables;
    if function.dispatch == DispatchClass::Numeric {
        let live = liveness::analyze(function, methods, field_sites, superinstructions);
        numeric::apply(function, live.as_deref());
    }
    // Renaming a plain slot's operations keeps it plain (validation re-derives the same
    // proof from the specialized code), so frame registers start from the same slots.
    let plain_slots = Compiler::specialize_plain_local_operations(function, superinstructions);
    frame_registers::promote(function, plain_slots, superinstructions, undefined_constant, atom_name);
    if specialized {
        rewrite::coalesce_promoted_moves(function, methods, field_sites, superinstructions);
    }
    if function
        .code
        .iter()
        .any(|instruction| instruction.op() == Op::StoreLocalPlain)
    {
        let live = liveness::analyze(function, methods, field_sites, superinstructions);
        numeric::apply_plain_local_stores(function, live.as_deref());
    }
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
            let text = self.atoms[*marker as usize].to_owned();
            let Some((kind, name)) = LexicalBindingKind::ALL.into_iter().find_map(|kind| {
                text.strip_prefix(kind.capture_prefix())
                    .map(|name| (kind, name))
            }) else {
                continue;
            };
            let atom = self.atom(name);
            if !visible.insert(atom) {
                continue;
            }
            let catch_marker = format!("{CATCH_CAPTURE_PREFIX}{name}");
            bindings.push(crate::bytecode::EvalBinding {
                atom,
                location: capture(*slot),
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
            if self.atoms[*atom as usize].contains('\0') || !visible.insert(*atom) {
                continue;
            }
            bindings.push(crate::bytecode::EvalBinding {
                atom: *atom,
                location: capture(*slot),
                with_depth: 0,
                kind: LexicalBindingKind::Mutable,
                declaration: crate::bytecode::EvalBindingDeclaration::Variable,
            });
        }
        bindings.sort_unstable_by_key(|binding| binding.atom);
        bindings
    }

    fn annex_b_collisions(&self, body: &[Statement<'_>]) -> FxHashSet<u32> {
        let mut collisions = early::annex_b_lexical_collisions(body);
        collisions.extend(self.eval_annex_b_collisions.iter().copied());
        collisions
    }

    fn new(
        source: &'a str,
        text: &'a str,
        mode: SpecializationMode,
        atom_prefix: &'a dyn AtomPrefix,
        private_name_ids: FxHashMap<(u32, u32), u32>,
    ) -> Self {
        Self::with_layout(
            source,
            text,
            mode,
            atom_prefix,
            private_name_ids,
            std::borrow::Cow::Owned(crate::bytecode::source_line_starts(text).collect()),
            std::borrow::Cow::Owned(source_utf16_excess(text)),
        )
    }

    /// `new`, with `text`'s line starts and UTF-16 excess table already derived.
    fn with_layout(
        source: &'a str,
        text: &'a str,
        mode: SpecializationMode,
        atom_prefix: &'a dyn AtomPrefix,
        private_name_ids: FxHashMap<(u32, u32), u32>,
        source_line_starts: std::borrow::Cow<'a, [usize]>,
        source_utf16_excess: std::borrow::Cow<'a, [(usize, usize)]>,
    ) -> Self {
        let atoms = CompilerAtoms::new(atom_prefix);
        Self {
            source,
            text,
            source_line_starts,
            last_source_line: std::cell::Cell::new(0),
            last_utf16_excess: Default::default(),
            source_comments: Vec::new(),
            source_utf16_excess,
            mode,
            root_strict: false,
            module_goal: false,
            capture_script_completion: false,
            eval_context: None,
            eval_annex_b_collisions: FxHashSet::default(),
            atoms,
            capture_markers: FxHashMap::default(),
            private_name_references: 0,
            private_name_ids,
            prototype_unobservable: FxHashSet::default(),
            lazy_bodies: FxHashSet::default(),
            lazy_compiled: Vec::new(),
            lazy_arenas: Vec::new(),
            #[cfg(feature = "verify-replay")]
            free_names: FxHashMap::default(),
            private_name_labels: FxHashMap::default(),
            private_name_overrides: FxHashMap::default(),
            constants: SiteTable::default(),
            constant_index: replay::ConstantIndex::default(),
            functions: replay::FunctionTable::default(),
            errors: vec![],
            cache_sites: 0,
            name_shared_sites: NameSharedSites::Load {
                sites: FxHashMap::default(),
                next: crate::bytecode::UNIQUE_CACHE_SITES,
            },
            template_sites: 0,
            method_sites: SiteTable::default(),
            field_sites: SiteTable::default(),
            object_sites: SiteTable::default(),
            regexp_literal_sites: SiteTable::default(),
            literal_templates: SiteTable::default(),
            superinstructions: SiteTable::default(),
            deferred_name_bindings: FxHashMap::default(),
            captured_name_scopes: FxHashMap::default(),
            function_exits: Vec::new(),
            function_facts: Vec::new(),
            dynamic_binding_scopes: 0,
            early_replays: FxHashMap::default(),
            invoked_functions: FxHashSet::default(),
            replay_calls: FxHashMap::default(),
            replay_target: None,
            source_type: oxc_span::SourceType::default(),
            replay_program: None,
            replayable_source: false,
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
        let id = id as usize;
        if self.function_exits.len() <= id {
            self.function_exits.resize(id + 1, replay::TableCursors::default());
            self.function_facts.resize(id + 1, capture_layout::FunctionFacts::default());
        }
        self.function_exits[id] = self.cursors();
        self.function_facts[id] = facts;
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
            && !invoked
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
        if std::env::var_os("QUENCH_PROFILE_RETAIN_BYTECODE").is_some() {
            // A function-level profile needs each replayable function's instruction records.
            return;
        }
        if !self.replayable_source || self.dynamic_binding_scopes != 0 {
            return;
        }
        let exit = self.function_exits[id as usize].functions as usize;
        let subtree = id as usize..exit;
        if self.function_facts[subtree]
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
            &self.function_facts,
            id as usize + 1..exit,
        );
        let layout = capture_layout::LayoutInputs {
            captured: capture_layout::complete_captured(
                captured,
                capture_layout::mapped_arguments(&function),
            ),
            dynamic_descendant: false,
            module_root: false,
        };
        let arguments_slot = function.arguments_slot;
        capture_layout::apply_function(&mut function, id as usize, &layout, &self.atoms);
        let rewrite_cursors = (
            self.field_sites.len() as u32,
            self.superinstructions.len() as u32,
        );
        self.rewrite_function(&mut function);
        let undefined_constant = self.constant(Constant::Undefined);
        // The record is released below and only its header stays, so a load that drops it
        // settles just the dispatch class the header keeps. One that encodes it finishes it as
        // a replay would, against tables whose entries for it are final (`lower_settled`);
        // `verify-replay` keeps the whole record to compare each replay with.
        if cfg!(feature = "verify-replay") || self.load_encoding.is_some() {
            self.lower_function(&mut function, undefined_constant);
            liveness::mark_register_roots(
                std::iter::once(&mut function),
                self.method_arguments(),
                self.field_sites.as_slice(),
                self.superinstructions.as_slice(),
            );
        } else {
            self.settle_dispatch(&mut function);
        }
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
                rewrite_cursors,
            },
        );
        // Name bindings are resolved only for functions a dynamic lookup reaches.
        self.deferred_name_bindings.remove(&id);
        self.functions.release(id, function);
        // Its rewrites and the `undefined` constant may have added sites: a replay of the
        // function making it skips past them too.
        self.function_exits[id as usize] = self.cursors();
    }

    /// A function declaration or expression, compiled from its syntax and `context`, which
    /// is all a replay of it needs besides the program.
    fn compile_plain_function(
        &mut self,
        function: &oxc_ast::ast::Function<'_>,
        mut context: replay::PlainFunctionContext,
    ) -> u32 {
        // A released body holds its stand-in wherever the function is compiled, and a node
        // may be compiled more than once (the right operand of `??`); the function parsed
        // again holds its own statements.
        if let Some(body) = &function.body
            && lazy_bodies::is_stand_in(body)
        {
            self.lazy_compiled.push(body.span.start);
            return self.compile_lazy_function(function, context);
        }
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
        let name = function
            .id
            .as_ref()
            .map(|id| id.name.as_str())
            .or(context.default_export.then_some("default"));
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
        let snippet = &self.text[function.span.start as usize..function.span.end as usize];
        let (text, offset) = lazy_bodies::text_alone(
            snippet,
            replay::plain_function_wrapper(&context),
            context.inherited_strict,
            self.source_type,
        );
        let mut allocator = self.lazy_arenas.pop().unwrap_or_else(|| lazy_bodies::arena(&text));
        allocator.reset();
        let id = self.compile_parsed_again(&allocator, &text, offset, function, context);
        self.lazy_arenas.push(allocator);
        id
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
        let nesting = if self.replay_target.is_some() {
            lazy_bodies::Nesting::Recorded(&self.lazy_bodies)
        } else {
            lazy_bodies::Nesting::Parse
        };
        let reparsed = lazy_bodies::reparse(
            allocator,
            text,
            offset,
            function.span.start.wrapping_sub(offset),
            self.source_type,
            nesting,
        );
        let Some(reparsed) = reparsed.filter(|reparsed| reparsed.function().is_some_and(|again| again.span == function.span)) else {
            self.reject(function.span, "SyntaxError: a lazily parsed function body does not compile alone");
            return self.compile_plain_function(function, context);
        };
        let again = reparsed.function().expect("checked above");
        self.compile_plain_function(again, context)
    }

    /// Checks, under `QUENCH_LAZY_FACTS_LOG`, that the free names of function `id` (starting
    /// at `start`), resolved through the scopes it closes over, are the captures its compiled
    /// subtree made of bindings outside it: what a lazily compiled function would report to
    /// the layouts around it without its code (TOD-196).
    #[cfg(feature = "verify-replay")]
    fn verify_predicted_captures(&self, id: u32, start: u32, scopes: &capture_scope::ScopeChain) {
        // Only a replayable source's functions can compile lazily.
        if std::env::var_os("QUENCH_LAZY_FACTS_LOG").is_none() || self.replay_target.is_some() || !self.replayable_source {
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
        let exit = self.function_exits[id].functions as usize;
        let depth_below = |mut function: usize| {
            let mut depth = 0u32;
            while function != id {
                function = self.function_facts[function].parent.expect("a descendant has a parent") as usize;
                depth += 1;
            }
            depth
        };
        let mut actual: Vec<(u32, u16)> = (id..exit)
            .flat_map(|function| {
                let depth = depth_below(function);
                self.function_facts[function]
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
        let strict = context.inherited_strict
            || function.body.as_ref().is_some_and(|body| {
                body.directives
                    .iter()
                    .any(|directive| directive.directive == "use strict")
            });
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
                strict,
                ..FunctionOptions::default()
            },
        );
        context.scopes = context.scopes.into_memo();
        self.record_replay_call(id, entry, private_names, replay::ReplayCall::ObjectMethod(context));
        id
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
        if self.replayable_source {
            context.scopes = context.scopes.recording();
        }
        let entry = self.cursors();
        let private_names = (self.private_name_references, self.private_name_ids.len());
        let id = self.compile_arrow_function(arrow, &context);
        context.scopes = context.scopes.into_memo();
        self.record_replay_call(id, entry, private_names, replay::ReplayCall::Arrow(context));
        id
    }

    /// Locate a delimiter between OXC-owned expression boundaries. Comment text
    /// is excluded using OXC's own spans, including comments containing delimiters.
    fn source_delimiter(&self, start: u32, end: u32, delimiter: char) -> u32 {
        let mut cursor = start as usize;
        let end = (end as usize).min(self.text.len());
        let mut comment = self.source_comments.partition_point(|span| span.end <= start);
        while cursor < end {
            if let Some(span) = self.source_comments.get(comment) {
                if span.start as usize <= cursor && cursor < span.end as usize {
                    cursor = span.end as usize;
                    comment += 1;
                    continue;
                }
            }
            let Some(character) = self.text[cursor..end].chars().next() else { break; };
            if character == delimiter { return cursor as u32; }
            cursor += character.len_utf8();
        }
        start
    }

    fn source_position(&self, offset: u32) -> SourcePosition {
        let offset = (offset as usize).min(self.text.len());
        let line_index = self.source_line_index(offset);
        let line_start = self.source_line_starts[line_index];
        let excess_before = |offset: usize, last: &std::cell::Cell<usize>| {
            let index = partition_point_from(&self.source_utf16_excess, last, |(character, _)| *character < offset);
            index
                .checked_sub(1)
                .map_or(0, |index| self.source_utf16_excess[index].1)
        };
        let [at_offset, at_line] = &self.last_utf16_excess;
        let column = offset - line_start - (excess_before(offset, at_offset) - excess_before(line_start, at_line));
        SourcePosition {
            pc: 0,
            line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
            column: u32::try_from(column.saturating_add(1)).unwrap_or(u32::MAX),
        }
    }

    /// The index of the line holding `offset` (`source_line_index`).
    fn source_line_index(&self, offset: usize) -> usize {
        source_line_index(&self.source_line_starts, &self.last_source_line, offset)
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
                implicit_super: false,
                strict: self.root_strict,
                with_depth: 0,
            },
        );
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
            .filter_map(|name| self.atoms.find(name))
            .collect();
        let global_immutable_atoms: Vec<_> = global_immutable_names
            .iter()
            .filter_map(|name| self.atoms.find(name))
            .collect();
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
            .filter_map(|name| self.atoms.find(name))
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
                .filter(|(span, _)| !direct_functions.contains(span) && !collisions.contains(span))
                .map(|(_, name)| name)
                .collect();
            global_var_names.extend(global_annex_b_var_names.iter().cloned());
        }
        global_var_names.sort();
        global_var_names.dedup();
        let global_var_atoms = global_var_names
            .iter()
            .filter_map(|name| self.atoms.find(name))
            .collect();
        let global_annex_b_var_atoms = global_annex_b_var_names
            .iter()
            .filter_map(|name| self.atoms.find(name))
            .collect();
        if let Some(root) = self.functions.first_mut() {
            root.global_declarations = crate::bytecode::GlobalDeclarations {
                lexical: global_lexical_atoms,
                var: global_var_atoms,
                function: global_function_atoms,
                annex_b_var: global_annex_b_var_atoms,
                immutable: global_immutable_atoms,
            }
            .boxed();
            if !self.module_goal {
                fn lower_global_var(
                    op: Op,
                    slot: impl FnOnce() -> usize,
                    local_atoms: &[Atom],
                    global_var_atoms: &[Atom],
                ) -> Option<Op> {
                    let replacement = match op {
                        Op::LoadLocal => Op::LoadEnvLocal,
                        Op::StoreLocal => Op::StoreEnvLocal,
                        _ => return None,
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
        let mut functions: Vec<Option<BcFunction>> = std::mem::take(&mut self.functions)
            .into_entries()
            .into_iter()
            .enumerate()
            .map(|(id, entry)| match entry {
                replay::FunctionEntry::Compiled(function) => Some(*function),
                replay::FunctionEntry::Released(function) => {
                    released.insert(id, function);
                    None
                }
                replay::FunctionEntry::Reserved => unreachable!("function {id} was never compiled"),
            })
            .collect();
        let facts = std::mem::take(&mut self.function_facts);
        let dynamic = capture_layout::dynamic_name_resolution(&facts);
        let reachable = capture_layout::dynamic_name_scopes(&facts, &dynamic);
        // A replay compiles one function and records no facts; it keeps no program. A class
        // constructor runs its instance initializer on its own environment
        // (`Vm::initialize_this_binding`); a released constructor's link is not at hand.
        let closure_reads = (facts.len() == functions.len()).then(|| {
            let initializers: Vec<capture_layout::InstanceInitializer> = functions
                .iter()
                .enumerate()
                .map(|(id, function)| match function {
                    Some(function) => capture_layout::InstanceInitializer::Known(function.instance_initializer),
                    None if released.get(&id).is_some_and(|released| released.header.is_class_constructor) => {
                        capture_layout::InstanceInitializer::Unknown
                    }
                    None => capture_layout::InstanceInitializer::Known(None),
                })
                .collect();
            std::rc::Rc::new(capture_layout::closure_reads(&facts, &reachable, &initializers))
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
            inputs.module_root = true;
            inputs.captured.extend(
                root.module_environment_slots(plan, &module_imports, |atom| self.atoms.get(atom as usize)),
            );
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
                        rewrite_cursors: (0, 0),
                    },
                );
            }
        }
        for (id, function) in functions.iter_mut().enumerate() {
            if let Some(function) = function {
                capture_layout::apply_function(function, id, &layout_inputs[id], &self.atoms);
            }
        }
        for (id, function) in functions.iter_mut().enumerate() {
            let Some(function) = function else { continue };
            if let Some(replay) = replays.get_mut(&(id as u32))
                && !released.contains_key(&id)
            {
                replay.rewrite_cursors = (
                    self.field_sites.len() as u32,
                    self.superinstructions.len() as u32,
                );
            }
            self.rewrite_function(function);
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() && released.is_empty() {
            let records: Vec<BcFunction> = functions.iter().flatten().cloned().collect();
            capture_profile::report(&records);
        }
        // Lowering adds no table entries, so a record waits for it until its first read
        // (`ResidualProgram::function`), which most of a bundle's functions never have; the
        // header's dispatch class is settled here. The root, which runs as soon as the program
        // does, lowers here, and every record does under `verify-replay`, as each replay it
        // compares with is lowered.
        let undefined_constant = self.constant(Constant::Undefined);
        let lower_at_load = lowers_at_load();
        let lowered_here = |id: usize| lower_at_load || id == crate::vm::ROOT_FUNCTION_ID as usize;
        for (id, function) in functions.iter_mut().enumerate() {
            let Some(function) = function else { continue };
            if lowered_here(id) {
                self.lower_function(function, undefined_constant);
                function.register_roots = liveness::register_roots(
                    function,
                    self.method_sites.as_slice(),
                    self.field_sites.as_slice(),
                    self.superinstructions.as_slice(),
                );
            } else {
                self.settle_dispatch(function);
            }
            function.shrink_to_fit();
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() && released.is_empty() {
            let records: Vec<BcFunction> = functions
                .iter()
                .enumerate()
                .filter_map(|(id, function)| Some((id, function.clone()?)))
                .map(|(id, mut function)| {
                    if !lowered_here(id) {
                        lower_settled(
                            &mut function,
                            &LoweringTables {
                                methods: self.method_arguments(),
                                field_sites: self.field_sites.as_slice(),
                                superinstructions: self.superinstructions.as_slice(),
                                specialized: self.mode == SpecializationMode::Enabled,
                                undefined_constant,
                            },
                            |atom| self.atoms.get(atom),
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
        let (mut method_sites, mut method_arguments) =
            Self::flatten_method_sites(self.method_sites.into_vec());
        let atoms = self.atoms.table();
        let replay_atoms = (!replays.is_empty())
            .then(|| replay::ProgramAtomIndex::new(self.atoms.interned(), &atoms));
        let mut constants = self.constants.into_vec();
        constants.shrink_to_fit();
        method_sites.shrink_to_fit();
        method_arguments.shrink_to_fit();
        let mut field_sites = self.field_sites.into_vec();
        field_sites.shrink_to_fit();
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
            source_name: self.source.to_owned(),
            source: Some(crate::bytecode::ProgramSource {
                text: crate::bytecode::SourceText::Shared(std::sync::Arc::from(self.text)),
                base: 0,
                line_offset: 0,
                column_offset: 0,
                line_starts: Default::default(),
            }),
            atoms,
            constants,
            // Collected into a table of its own size: an in-place collect would keep the
            // records' larger allocation.
            functions: {
                let mut slots = Vec::with_capacity(functions.len());
                slots.extend(functions.into_iter().enumerate().map(|(id, function)| {
                    match function {
                        Some(function) if lowered_here(id) => FunctionSlot::compiled(function),
                        Some(function) => FunctionSlot::unlowered(function),
                        None => FunctionSlot::released(released[&id].header),
                    }
                }));
                slots
            },
            cache_sites: self.cache_sites,
            method_sites,
            method_arguments,
            field_sites,
            object_sites: self.object_sites.into_vec(),
            regexp_literal_sites: self.regexp_literal_sites.into_vec(),
            literal_templates: self.literal_templates.into_vec(),
            superinstructions: self.superinstructions.into_vec(),
            replay: replay_atoms.map(|atoms| {
                std::rc::Rc::new(replay::ProgramReplay {
                    source_type: self.source_type,
                    root_strict: self.root_strict,
                    module_goal: self.module_goal,
                    lazy_bodies: {
                        let mut starts = std::mem::take(&mut self.lazy_compiled);
                        starts.sort_unstable();
                        starts.dedup();
                        starts.into_boxed_slice()
                    },
                    specialized: self.mode == SpecializationMode::Enabled,
                    atoms,
                    exits: std::mem::take(&mut self.function_exits).into_boxed_slice(),
                    name_shared_sites: match &mut self.name_shared_sites {
                        NameSharedSites::Load { sites, .. } => std::mem::take(sites),
                        NameSharedSites::Replay(sites) => (*sites).clone(),
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
            encoded_records: None,
            validated: Default::default(),
            undefined_constant: Default::default(),
            closure_reads,
            load_encoding: crate::bytecode::LoadRecords::new(self.load_encoding.take()),
        };
        #[cfg(feature = "profile-aggregate")]
        debug_assert!(crate::profile::instruction_word_domains_fit(&program));
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
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

    fn regexp_literal_site(&mut self, pattern_constant: u32, flags_constant: u32) -> u32 {
        let index = self.regexp_literal_sites.len() as u32;
        self.regexp_literal_sites
            .push(crate::bytecode::RegExpLiteralSite {
                pattern_constant,
                flags_constant,
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
        let access = match op {
            Op::GetField if base != crate::bytecode::FieldBase::NESTED => NamedAccess::Load,
            Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict => NamedAccess::Store,
            _ => return site,
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
                    receiver_path,
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
        // An async body that may suspend keeps its locals in frame slots; one that never
        // awaits runs to completion as an ordinary body does (`CallPlan::settles_in_loop`).
        let suspends = async_body && register_local_facts.suspends;
        if !eligible || suspends || !register_local_facts.safe || !arguments_allow_promotion {
            return PromotedLocalPlan { unaliased_arguments, ..Default::default() };
        }
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
        if let Some(id) = self.skip_replayed_child() {
            return id;
        }
        let id = self.functions.reserve();
        let body_lexicals = self.collect_body_lexical_bindings(body);
        let lexical_atoms: Vec<_> = body_lexicals.iter().map(|(atom, _)| *atom).collect();
        let params: Vec<Atom> = params.iter().map(|name| self.atom(name)).collect();
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
        let mut function_scope =
            self.collect_locals(body, &mut locals, root_strict, parent.is_some());
        let name_binding = options.name_binding.and_then(|source_name| {
            if locals.contains(&source_name) {
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
        let annex_b_collisions = self.annex_b_collisions(body);
        let arguments_slot = if let Some(slot) = parameter_arguments_slot {
            Some(slot)
        } else if parent.is_none() || options.class_initializer {
            // Field initializers and static blocks cannot name `arguments` (an early error),
            // so they have no binding to materialize.
            None
        } else {
            if parameter_shadows_arguments {
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
            }
        };
        let environment_atoms = locals
            .iter()
            .copied()
            .filter(|atom| function_scope.contains(atom))
            .collect();
        let dynamic_eval = options
            .defaults
            .is_some_and(early::parameters_contain_direct_eval)
            || early::body_contains_direct_eval(body)
            || expression_body.is_some_and(early::expression_contains_direct_eval);
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
            && !options.generator
            && !options.derived_constructor
            && !options.class_initializer
            && !options.implicit_super
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
            options.async_function,
            hidden_parameters.then_some(params.len()),
            Some((root_strict, &params)),
            body,
            expression_body,
            &locals,
            &function_scope,
            &lexical_atoms,
            self_binding_slot,
        );
        let binds_dynamically =
            dynamic_eval || early::body_may_bind_dynamically(body, expression_body);
        self.dynamic_binding_scopes += u32::from(binds_dynamically);
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
        if let Some((name, binding)) = name_binding {
            function.push_function_name_binding(name, binding);
        }
        function.super_home_atom = options.super_home_atom;
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
            function.emit(Op::Move, completion, initial, 0, 0);
            function.statement_completion = StatementCompletion::Track(completion);
        }
        function.dynamic_eval = dynamic_eval;
        if arguments_slot.is_some() && parameter_arguments_slot.is_none() {
            function.arguments_alias = early::arguments_alias(options.defaults, body);
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
            function
                .emit_instance_fields(fields, options.instance_private_methods.unwrap_or_default());
        }
        if let Some(defaults) = options.defaults {
            function.emit_parameter_bindings(defaults);
        }
        if let Some(slot) = parameter_arguments_slot
            && !lexical_atoms.contains(&arguments)
        {
            let value = function.reg();
            function.emit(Op::LoadLocal, value, 0, 0, u32::from(slot));
            function.store_atom(arguments, value);
        }
        let parameter_end_pc = function.code.len() as u32;
        function.push_body_lexical_bindings(&body_lexicals);
        function.emit_hoisted(body);
        if options.implicit_super {
            function.emit_implicit_super();
        }
        function.function_body_statements(body);
        let result = if let Some(expression) = expression_body {
            function.expression(expression)
        } else {
            function
                .statement_completion
                .register()
                .unwrap_or_else(|| function.literal(Constant::Undefined))
        };
        function.emit(Op::Return, result, 0, 0, 0);
        let captures_locals = function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
            || function
                .wide
                .iter()
                .any(|instruction| instruction.op() == Op::MakeClosure);
        let reads_binding =
            |op: Op| matches!(op, Op::LoadLocal | Op::ArgumentsLength | Op::ArgumentGet);
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
        function.code.shrink_to_fit();
        function.source_positions.shrink_to_fit();
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
            rest: options.rest_override
                || options.defaults.is_some_and(|value| value.rest.is_some()),
            is_async: options.async_function,
            is_generator: options.generator,
            is_class_constructor: options.class_constructor,
            derived_constructor: options.derived_constructor,
            instance_initializer: None,
            super_home_atom: options.super_home_atom,
            constructible: !options.non_constructible
                && !options.async_function
                && !options.generator,
            prototype_unobservable: options.prototype_unobservable,
            class_initializer: options.class_initializer,
            parameter_eval_arguments_error: function.parameter_eval_arguments_error,
            unaliased_arguments,
            arguments_slot,
            simple_parameters,
            strict: root_strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone().into(),
            environment_atoms,
            selective_capture_slots: None,
            local_registers: function.promoted_local_layout().into(),
            inherited_with_scope: options.with_depth != 0,
            lexical_atoms: lexical_atoms.into(),
            global_declarations: None,
            name_bindings,
            binding_sites: function.binding_sites.into(),
            source_positions: function.source_positions.into(),
            environment_clones: environment_clones.into(),
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            dynamic_scope_access: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            initializer: Default::default(),
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
        // A function not yet compiled has default facts, which name no arrow.
        facts.iter().any(|function| {
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
                    Some(parent) => match facts.get(parent as usize) {
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
            );
        }
    }

    /// The finishing passes of one function after every function's rewrites: its dispatch
    /// class (`settle_dispatch`), then the passes the class leaves (`lower_settled`).
    fn lower_function(&self, function: &mut BcFunction, undefined_constant: u32) {
        self.settle_dispatch(function);
        lower_settled(
            function,
            &LoweringTables {
                methods: self.method_arguments(),
                field_sites: self.field_sites.as_slice(),
                superinstructions: self.superinstructions.as_slice(),
                specialized: self.mode == SpecializationMode::Enabled,
                undefined_constant,
            },
            |atom| self.atoms.get(atom),
        );
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

    fn dispatch_class(code: &[Instr], wide: &[WideInstruction]) -> DispatchClass {
        if !wide.is_empty() {
            return DispatchClass::General;
        }
        let binaries = code
            .iter()
            .filter(|instruction| instruction.op() == Op::Binary)
            .count();
        if code.len() >= 32 && binaries * 3 >= code.len() {
            DispatchClass::Numeric
        } else {
            DispatchClass::General
        }
    }

    /// Renames the operations of `function`'s plain local slots, which it returns.
    fn specialize_plain_local_operations(
        function: &mut BcFunction,
        superinstructions: &[crate::bytecode::Superinstruction],
    ) -> Vec<bool> {
        let plain_slots = function.plain_local_slots(superinstructions);
        for instruction in &mut function.code {
            match instruction.op() {
                Op::LoadLocal | Op::StoreLocal
                    if plain_slots.get(instruction.local_slot()) == Some(&true) =>
                {
                    instruction.set_op(if instruction.op() == Op::LoadLocal {
                        Op::LoadLocalPlain
                    } else {
                        Op::StoreLocalPlain
                    });
                }
                _ => {}
            }
        }
        for instruction in &mut function.wide {
            match instruction.op() {
                Op::LoadLocal | Op::StoreLocal
                    if plain_slots.get(instruction.local_slot()) == Some(&true) =>
                {
                    instruction.set_op(if instruction.op() == Op::LoadLocal {
                        Op::LoadLocalPlain
                    } else {
                        Op::StoreLocalPlain
                    });
                }
                _ => {}
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
