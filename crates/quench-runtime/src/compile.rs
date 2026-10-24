use crate::bytecode::{
    Atom, Constant, DispatchClass, FieldBase, FieldSite, Function as BcFunction, Instr,
    LexicalBindingKind, MethodSite, ModuleImportBinding, ModuleImportName, ModuleLinkPlan,
    ModuleRequest, ModuleRequestPhase, ObjectSite, Op, Operand, Register, ResidualProgram,
    SET_THIS_REGISTER, SourcePosition, Superinstruction, WideInstruction,
};
use oxc_allocator::Allocator;
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
mod class;
mod early;
pub(crate) mod liveness;
mod locals;
mod numeric;
pub(crate) mod regexp;
#[cfg(feature = "profile-memory")]
mod register_profile;
mod rewrite;
mod sequence;
mod stack;
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
    if !ordinary_function {
        return Some(body);
    }
    let [Statement::ExpressionStatement(statement)] = body.statements.as_mut_slice() else {
        return None;
    };
    let Expression::FunctionExpression(function) = statement.expression.without_parentheses_mut()
    else {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicFunctionKind {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

pub(crate) const DYNAMIC_FUNCTION_PUBLIC_NAME: &str = "anonymous";

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
        format!(
            "{} {DYNAMIC_FUNCTION_PUBLIC_NAME}({parameters}\n) {{\n{body}\n}}",
            self.source_prefix()
        )
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
    DynamicFunction { parameter_list_end: usize },
    Eval(EvalContext, Option<Span>),
}

pub(crate) struct EvalRegExpLiteral {
    pub(crate) span: Range<usize>,
    pub(crate) flags: String,
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
        if stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() {
            return true;
        }
        let mut visitor = TaggedTemplate(false);
        visitor.visit_program(&parsed.program);
        visitor.0
    }

    pub(crate) fn eval_single_regexp_literal(source: &str) -> Option<EvalRegExpLiteral> {
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

    pub(crate) fn static_module_has_early_error(source: &str) -> bool {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, SourceType::mjs()).parse();
        if stack::validate_parsed(&parsed).is_err() {
            return false;
        }
        if !parsed.diagnostics.is_empty() {
            return true;
        }
        oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .len()
            != 0
    }

    pub(crate) fn static_module_plan(source: &str, module_name: &str) -> Option<StaticModulePlan> {
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
        let link_plan = Self::module_link_plan(&parsed.program.body, module_name);
        Some(StaticModulePlan {
            link_plan,
            requests: module_requests(&parsed.program.body),
            has_top_level_await: has_top_level_await(&parsed.program.body),
        })
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
            ) || import.module_type.is_some()
            {
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
            ParsedProgramShape::Any,
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
        let ordinary_private_context =
            context.home_atom.is_none() && context.in_function && !private_names.is_empty();
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
                } else {
                    source
                };
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
            let suffix = if ordinary_private_context {
                "\n});\n}\n})"
            } else if private_names.is_empty() && !context.super_calls {
                "\n}})"
            } else {
                "\n}\n})"
            };
            let body_start = source.len() - suffix.len() - raw_source_len - "{\n".len();
            Some(Span::new(
                body_start as u32,
                (body_start + "{\n".len() + raw_source_len + "\n}".len()) as u32,
            ))
        } else {
            None
        };
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
        )
    }

    pub fn specialize_module_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Disabled)
    }
    /// Compile a module against an existing program's canonical atom prefix.
    pub(crate) fn specialize_module_unspecialized_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Disabled,
            atom_prefix,
            SourceType::mjs(),
            true,
            false,
            false,
        )
    }
    pub fn specialize_module(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Enabled)
    }
    /// Compile already-coerced Function-constructor parameter and body strings
    /// through the same OXC script pipeline used by ordinary source.
    pub(crate) fn specialize_function_constructor(
        parameters: &str,
        body: &str,
        name: &str,
        atom_prefix: &dyn AtomPrefix,
        kind: DynamicFunctionKind,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let body = early::normalize_dynamic_function_body(body);
        let parameters = early::normalize_dynamic_function_parameters(parameters);
        let expression_wrapper = "(";
        let function_prefix = format!("{} {DYNAMIC_FUNCTION_PUBLIC_NAME}(", kind.source_prefix());
        let parameter_list_end =
            expression_wrapper.len() + function_prefix.len() + parameters.len() + "\n)".len();
        let source = format!("{expression_wrapper}{})", kind.source(&parameters, &body));
        let expected_shape = ParsedProgramShape::DynamicFunction { parameter_list_end };
        Self::specialize_with_program_shape(
            &source,
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
        )
    }
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
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let mut parsed = annex_b_targets::parse_program(&allocator, &normalized, source_type);
        if parsed.stack_exhausted {
            return Err(vec![Diagnostic::stack_exhausted(name)]);
        }
        if !parsed.diagnostics.is_empty() {
            return Err(parsed
                .diagnostics
                .into_iter()
                .map(|error| Diagnostic {
                    kind: DiagnosticKind::Compilation,
                    source: name.into(),
                    message: error.to_string(),
                    span: Span::default(),
                })
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
            if context.home_atom.is_some())
            || ordinary_private_context;
        let dynamic_function_shape = match expected_shape {
            ParsedProgramShape::Any => true,
            ParsedProgramShape::Eval(_, expected_span) => {
                !wrapped_eval
                    || eval_method_body(&mut parsed.program, ordinary_private_context)
                        .is_some_and(|body| Some(body.span) == expected_span)
            }
            ParsedProgramShape::DynamicFunction { parameter_list_end } => {
                match parsed.program.body.as_mut_slice() {
                    [Statement::ExpressionStatement(statement)] => {
                        match &mut statement.expression {
                            Expression::ParenthesizedExpression(expression) => {
                                match &mut expression.expression {
                                    Expression::FunctionExpression(function) => {
                                        let valid =
                                            function.id.as_ref().is_some_and(|identifier| {
                                                identifier.name == DYNAMIC_FUNCTION_PUBLIC_NAME
                                            }) && function.body.is_some()
                                                && function.params.span.end as usize
                                                    == parameter_list_end;
                                        // The generated public source name is not a dynamic
                                        // Function self binding. OXC still owns the grammar.
                                        if valid {
                                            function.id = None;
                                        }
                                        valid
                                    }
                                    _ => false,
                                }
                            }
                            _ => false,
                        }
                    }
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
        let private_name_ids = private_name_ids(&semantic.semantic);
        let private_name_labels = private_name_labels(&semantic.semantic);
        let inherited_private_ids = if wrapped_eval {
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
        compiler.private_name_labels = private_name_labels;
        compiler.private_name_overrides = private_aliases;
        compiler.capture_script_completion = capture_script_completion;
        compiler.eval_context = eval_context;
        compiler.eval_annex_b_collisions = early::annex_b_function_names(&parsed.program.body)
            .into_iter()
            .filter_map(|(span, name)| annex_b_forbidden_names.contains(&name).then_some(span))
            .collect();
        let program = compiler.program(&parsed.program, module_goal, inherited_strict);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"quench-oxc-memory\",\"used\":{},\"capacity\":{}}}",
                allocator.used_bytes(),
                allocator.capacity(),
            );
        }
        program
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
            return Some(format!("SyntaxError: {}", parsed.diagnostics[0]));
        }
        oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .first()
            .map(|diagnostic| format!("SyntaxError: {diagnostic}"))
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
fn source_function_count(program: &Program<'_>) -> usize {
    struct Counter(usize);
    impl<'a> oxc_ast_visit::Visit<'a> for Counter {
        fn visit_function(
            &mut self,
            function: &oxc_ast::ast::Function<'a>,
            flags: oxc_syntax::scope::ScopeFlags,
        ) {
            self.0 += 1;
            oxc_ast_visit::walk::walk_function(self, function, flags);
        }

        fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
            self.0 += 1;
            oxc_ast_visit::walk::walk_arrow_function_expression(self, arrow);
        }
    }
    let mut counter = Counter(0);
    oxc_ast_visit::Visit::visit_program(&mut counter, program);
    counter.0
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

struct Compiler<'a> {
    source: &'a str,
    text: &'a str,
    source_line_starts: Vec<usize>,
    source_comments: Vec<Span>,
    /// Byte offset of each non-ASCII character with the running excess of
    /// UTF-8 bytes over UTF-16 units through it; columns count UTF-16 units.
    source_utf16_excess: Vec<(usize, usize)>,
    mode: SpecializationMode,
    root_strict: bool,
    module_goal: bool,
    capture_script_completion: bool,
    eval_context: Option<EvalContext>,
    eval_annex_b_collisions: FxHashSet<u32>,
    atoms: CompilerAtoms<'a>,
    private_name_ids: FxHashMap<(u32, u32), u32>,
    private_name_labels: FxHashMap<(u32, u32), String>,
    private_name_overrides: FxHashMap<u32, String>,
    constants: Vec<Constant>,
    constant_index: FxHashMap<ConstantKey, u32>,
    functions: Vec<Option<BcFunction>>,
    errors: Vec<Diagnostic>,
    cache_sites: u16,
    template_sites: u32,
    method_sites: Vec<MethodSiteSpec>,
    field_sites: Vec<FieldSite>,
    object_sites: Vec<ObjectSite>,
    regexp_literal_sites: Vec<crate::bytecode::RegExpLiteralSite>,
    superinstructions: Vec<Superinstruction>,
    /// Distinct `name_bindings` lists: sibling functions that see the same bindings,
    /// such as every function declared in one module scope, share one list.
    /// Resolved captured scopes keyed by (scope, enclosing chain) addresses; see `captured_names`.
    captured_name_scopes: FxHashMap<
        (usize, Option<usize>),
        (
            std::rc::Rc<FxHashMap<Atom, u16>>,
            std::rc::Rc<crate::bytecode::CapturedNames>,
        ),
    >,
}

type MethodSiteSpec = (Atom, u16, Vec<Register>, Option<(Atom, u16)>);

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

#[derive(Clone, PartialEq, Eq, Hash)]
enum ConstantKey {
    Number(u64),
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    Boolean(bool),
    Null,
    Undefined,
}

impl From<&Constant> for ConstantKey {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(value.to_bits()),
            Constant::WasmBits64(bits) => Self::WasmBits64(*bits),
            Constant::WasmV128(bits) => Self::WasmV128(*bits),
            Constant::String(value) => Self::String(value.clone()),
            Constant::StringUnits(value) => Self::StringUnits(value.clone()),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}

impl<'a> Compiler<'a> {
    /// The captured scopes of `scopes`, innermost first. A scope's names are resolved once per
    /// enclosing chain and shared by every function closing over it.
    fn captured_names(
        &mut self,
        scopes: &[std::rc::Rc<FxHashMap<Atom, u16>>],
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
        scope: &FxHashMap<Atom, u16>,
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
            let catch_marker = format!("\0quench:catch-capture:{name}");
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
        let atoms = CompilerAtoms::new(atom_prefix);
        let source_line_starts = crate::bytecode::source_line_starts(text).collect();
        let mut excess = 0;
        let source_utf16_excess = text
            .char_indices()
            .filter(|(_, character)| !character.is_ascii())
            .map(|(offset, character)| {
                excess += character.len_utf8() - character.len_utf16();
                (offset, excess)
            })
            .collect();
        Self {
            source,
            text,
            source_line_starts,
            source_comments: Vec::new(),
            source_utf16_excess,
            mode,
            root_strict: false,
            module_goal: false,
            capture_script_completion: false,
            eval_context: None,
            eval_annex_b_collisions: FxHashSet::default(),
            atoms,
            private_name_ids,
            private_name_labels: FxHashMap::default(),
            private_name_overrides: FxHashMap::default(),
            constants: vec![],
            constant_index: FxHashMap::default(),
            functions: vec![],
            errors: vec![],
            cache_sites: 0,
            template_sites: 0,
            method_sites: vec![],
            field_sites: vec![],
            object_sites: vec![],
            regexp_literal_sites: vec![],
            superinstructions: vec![],
            captured_name_scopes: FxHashMap::default(),
        }
    }

    /// Locate a delimiter between OXC-owned expression boundaries. Comment text
    /// is excluded using OXC's own spans, including comments containing delimiters.
    fn source_delimiter(&self, start: u32, end: u32, delimiter: char) -> u32 {
        let mut cursor = start as usize;
        let end = (end as usize).min(self.text.len());
        let mut comment = self
            .source_comments
            .partition_point(|span| span.end <= start);
        while cursor < end {
            if let Some(span) = self.source_comments.get(comment) {
                if span.start as usize <= cursor && cursor < span.end as usize {
                    cursor = span.end as usize;
                    comment += 1;
                    continue;
                }
            }
            let Some(character) = self.text[cursor..end].chars().next() else {
                break;
            };
            if character == delimiter {
                return cursor as u32;
            }
            cursor += character.len_utf8();
        }
        start
    }

    fn source_position(&self, offset: u32) -> SourcePosition {
        let offset = (offset as usize).min(self.text.len());
        let line_index = self
            .source_line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.source_line_starts[line_index];
        let excess_before = |offset: usize| {
            let index = self
                .source_utf16_excess
                .partition_point(|(character, _)| *character < offset);
            index
                .checked_sub(1)
                .map_or(0, |index| self.source_utf16_excess[index].1)
        };
        let column = offset - line_start - (excess_before(offset) - excess_before(line_start));
        SourcePosition {
            pc: 0,
            line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
            column: u32::try_from(column.saturating_add(1)).unwrap_or(u32::MAX),
        }
    }

    fn program(
        mut self,
        program: &Program<'_>,
        module_goal: bool,
        inherited_strict: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        self.source_comments = program
            .comments
            .iter()
            .map(|comment| comment.span)
            .collect();
        // One slot per source function and the root, reserved once: grown by doubling, the
        // slots of a large script stood up to twice its functions beside its tree.
        self.functions
            .reserve_exact(source_function_count(program) + 1);
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
        if let Some(name) =
            early::strict_restricted_assignment_early_error(program, self.root_strict)
        {
            self.reject(
                Span::default(),
                format!("SyntaxError: assignment to {name} is not allowed in strict mode"),
            );
        }
        if self.root_strict
            && let Some(error) = early::strict_octal_numeric_early_error(program)
        {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::block_early_error(program, self.root_strict) {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::strict_binding_early_error(program, self.root_strict) {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::parameter_early_error(program, self.root_strict) {
            self.reject(Span::default(), error);
        }
        self.compile_function(
            None,
            &[],
            FunctionBody::Statements(&program.body),
            &[],
            None,
            FunctionOptions {
                defaults: None,
                directive_completion: program
                    .directives
                    .last()
                    .map(|directive| &directive.expression),
                source_span: None,
                name_binding: None,
                async_function: async_module,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
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
        if let Some(Some(root)) = self.functions.first_mut() {
            root.global_lexical_atoms = global_lexical_atoms;
            root.global_immutable_atoms = global_immutable_atoms;
            root.global_var_atoms = global_var_atoms;
            root.global_function_atoms = global_function_atoms;
            root.global_annex_b_var_atoms = global_annex_b_var_atoms;
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
                for instruction in &mut root.code {
                    if let Some(op) = lower_global_var(
                        instruction.op(),
                        || instruction.local_slot(),
                        &root.local_atoms,
                        &root.global_var_atoms,
                    ) {
                        instruction.set_op(op);
                    }
                }
                for instruction in &mut root.wide {
                    if let Some(op) = lower_global_var(
                        instruction.op(),
                        || instruction.local_slot(),
                        &root.local_atoms,
                        &root.global_var_atoms,
                    ) {
                        instruction.set_op(op);
                    }
                }
            }
        }
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        let mut functions: Vec<_> = self.functions.into_iter().map(Option::unwrap).collect();
        // Functions the compiler synthesizes (class constructors and initializers) may have
        // grown the reservation.
        functions.shrink_to_fit();
        capture_layout::apply(&mut functions, &self.atoms);
        for function in &mut functions {
            Self::elide_unobservable_tdz(function, &self.atoms);
        }
        if self.mode == SpecializationMode::Enabled {
            Self::apply_rewrites(
                &mut functions,
                &self.method_sites,
                &mut self.field_sites,
                &mut self.superinstructions,
            );
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            capture_profile::report(&functions);
        }
        for function in &mut functions {
            rewrite::return_through_shared_returns(function);
            rewrite::express_strict_sets(function);
            function.dispatch = if self.mode == SpecializationMode::Enabled {
                Self::dispatch_class(&function.code, &function.wide)
            } else {
                DispatchClass::General
            };
            if function.dispatch == DispatchClass::Numeric {
                let live = liveness::analyze(
                    function,
                    &self.method_sites,
                    &self.field_sites,
                    &self.superinstructions,
                );
                numeric::apply(function, live.as_deref());
            }
            Self::specialize_plain_local_operations(function, &self.atoms);
            if function
                .code
                .iter()
                .any(|instruction| instruction.op() == Op::StoreLocalPlain)
            {
                let live = liveness::analyze(
                    function,
                    &self.method_sites,
                    &self.field_sites,
                    &self.superinstructions,
                );
                numeric::apply_plain_local_stores(function, live.as_deref());
            }
        }
        let register_roots = liveness::derive(
            &mut functions,
            &self.method_sites,
            &self.field_sites,
            &self.superinstructions,
        );
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            register_profile::report(
                &functions,
                &self.method_sites,
                &self.field_sites,
                &self.superinstructions,
            );
        }
        for function in &mut functions {
            function.code.shrink_to_fit();
            function.handlers.shrink_to_fit();
        }
        let (mut method_sites, mut method_arguments) =
            Self::flatten_method_sites(self.method_sites);
        let atoms = self.atoms.table();
        self.constants.shrink_to_fit();
        functions.shrink_to_fit();
        method_sites.shrink_to_fit();
        method_arguments.shrink_to_fit();
        self.field_sites.shrink_to_fit();
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
                text: std::sync::Arc::from(self.text),
                line_offset: 0,
                column_offset: 0,
            }),
            atoms,
            constants: self.constants,
            functions,
            cache_sites: self.cache_sites,
            method_sites,
            method_arguments,
            field_sites: self.field_sites,
            object_sites: self.object_sites,
            regexp_literal_sites: self.regexp_literal_sites,
            superinstructions: self.superinstructions,
            register_roots,
        };
        #[cfg(feature = "profile-aggregate")]
        debug_assert!(crate::profile::instruction_word_domains_fit(&program));
        Ok(program)
    }

    fn atom(&mut self, text: &str) -> Atom {
        self.atoms.intern(text)
    }

    fn private_name_atom(&mut self, span: Span) -> Atom {
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
        let key = ConstantKey::from(&value);
        if let Some(index) = self.constant_index.get(&key) {
            return *index;
        }
        let index = self.constants.len() as u32;
        self.constants.push(value);
        self.constant_index.insert(key, index);
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
            self.constant_index
                .entry(ConstantKey::from(&value))
                .or_insert(index);
            self.constants.push(value);
        }
        start
    }

    fn cache_site(&mut self) -> u16 {
        let site = self.cache_sites.min(crate::bytecode::SHARED_UNCACHED_SITE);
        self.cache_sites = site + 1;
        site
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
    fn compile_function(
        &mut self,
        name: Option<&str>,
        params: &[String],
        body: FunctionBody<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        options: FunctionOptions<'_>,
    ) -> u32 {
        let (body, expression_body) = match body {
            FunctionBody::Statements(body) => (body, None),
            FunctionBody::Expression(expression) => (&[][..], Some(expression)),
        };
        let id = self.functions.len() as u32;
        self.functions.push(None);
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
                    && FunctionCompiler::has_non_simple_parameters(parameters)
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
        } else if parent.is_none() {
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
        let register_local_facts = early::fixed_register_local_facts(body, expression_body);
        let promote_plain_locals = parent.is_some()
            && simple_parameters
            && !options.async_function
            && !options.generator
            && !options.class_constructor
            && !options.derived_constructor
            && !options.class_initializer
            && !options.implicit_super
            && options.with_depth == 0
            && self.eval_context.is_none()
            && !dynamic_eval
            && register_local_facts.safe;
        let arguments_atom = self.atom("arguments");
        let mut latest_local_slot = FxHashMap::default();
        for (slot, atom) in locals.iter().copied().enumerate() {
            if let Ok(slot) = u16::try_from(slot) {
                latest_local_slot.insert(atom, slot);
            }
        }
        let mut promoted_local_slots = if promote_plain_locals {
            latest_local_slot
                .into_iter()
                .filter_map(|(atom, slot)| {
                    (function_scope.contains(&atom)
                        && !lexical_atoms.contains(&atom)
                        && atom != arguments_atom
                        && self_binding_slot != Some(slot)
                        && !self.atoms[atom as usize].starts_with('\0'))
                    .then_some(slot)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
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
        let mut function = FunctionCompiler::new(
            self,
            locals,
            function_scope,
            annex_b_collisions,
            scopes.to_vec(),
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
            } else {
                function.literal(Constant::Undefined)
            };
            function.emit(Op::Move, completion, initial, 0, 0);
            function.statement_completion = StatementCompletion::Track(completion);
        }
        function.dynamic_eval = dynamic_eval;
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
        let arguments_slot = arguments_slot.filter(|slot| {
            captures_locals
                || function.dynamic_eval
                || function.code.iter().any(|instruction| {
                    instruction.op() == Op::LoadLocal
                        && instruction.local_slot() == usize::from(*slot)
                })
                || function.wide.iter().any(|instruction| {
                    instruction.op() == Op::LoadLocal
                        && instruction.local_slot() == usize::from(*slot)
                })
        });
        let parameter_atoms = options.defaults.map_or_else(Vec::new, |formal| {
            FunctionCompiler::parameter_bound_names(formal)
                .iter()
                .map(|name| function.owner.atom(name))
                .collect()
        });
        let name_bindings = function.name_bindings();
        let undefined_register = function.used_undefined_register();
        let environment_clones = function.per_site_environment_clones();
        let result = BcFunction {
            parent,
            name: name.map(|value| function.owner.atom(value)),
            is_arrow: false,
            self_binding_slot,
            source_span: options.source_span,
            params: params.len() as u16,
            length: options.defaults.map_or_else(
                || {
                    params
                        .len()
                        .saturating_sub(usize::from(options.rest_override))
                },
                Self::formal_length,
            ) as u16,
            parameter_end_pc: if options.generator {
                parameter_end_pc
            } else {
                0
            },
            parameter_atoms,
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
            class_initializer: options.class_initializer,
            parameter_eval_arguments_error: function.parameter_eval_arguments_error,
            arguments_slot,
            simple_parameters,
            strict: root_strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone(),
            environment_atoms,
            selective_capture_slots: None,
            local_registers: function.promoted_local_layout(),
            inherited_with_scope: options.with_depth != 0,
            lexical_atoms,
            global_lexical_atoms: Vec::new(),
            global_var_atoms: Vec::new(),
            global_function_atoms: Vec::new(),
            global_annex_b_var_atoms: Vec::new(),
            global_immutable_atoms: Vec::new(),
            name_bindings,
            binding_sites: function.binding_sites,
            source_positions: function.source_positions,
            environment_clones,
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            call_plan: Default::default(),
            handlers: function.handlers,
            register_root_offset: crate::bytecode::NO_REGISTER_ROOT_MAP,
            undefined_register,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        self.functions[id as usize] = Some(result);
        id
    }

    fn formal_length(params: &oxc_ast::ast::FormalParameters<'_>) -> usize {
        params
            .items
            .iter()
            .take_while(|item| item.initializer.is_none())
            .count()
    }

    fn apply_rewrites(
        functions: &mut [BcFunction],
        method_sites: &[MethodSiteSpec],
        field_sites: &mut Vec<FieldSite>,
        superinstructions: &mut Vec<Superinstruction>,
    ) {
        for function in functions {
            rewrite::apply(function, method_sites, field_sites, superinstructions);
        }
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

    /// Drops the TDZ resets of slots whose uninitialized state no access can observe
    /// (`unobservable_tdz_slots`), so their loads and stores need no TDZ check.
    fn elide_unobservable_tdz(function: &mut BcFunction, atoms: &CompilerAtoms<'_>) {
        let slots = function.unobservable_tdz_slots(|atom| atoms.get(atom as usize));
        if !slots.contains(&true) {
            return;
        }
        for instruction in &mut function.code {
            if instruction.op() == Op::InitializeTdz
                && slots.get(instruction.local_slot()) == Some(&true)
            {
                *instruction = Instr::new(Op::Nop, 0, 0, 0, 0);
            }
        }
    }

    fn specialize_plain_local_operations(function: &mut BcFunction, atoms: &CompilerAtoms<'_>) {
        let plain_slots = function.plain_local_slots(|atom| atoms.get(atom as usize));

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
    }
}
#[cfg(test)]
mod tests;
