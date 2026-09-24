use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, FieldSite, Function as BcFunction, Instr,
    MAPPED_ARGUMENTS_BIT, MethodSite, ModuleImportBinding, ModuleImportName, ModuleRequest,
    ModuleRequestPhase, ObjectSite, Op, Operand, Register, ResidualProgram, SET_THIS_REGISTER,
    Superinstruction, WideInstruction,
};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;
use std::rc::Rc;
mod arrow;
mod ast;
mod binding_time;
#[cfg(feature = "profile-memory")]
mod capture_profile;
mod class;
mod early;
mod liveness;
mod locals;
mod numeric;
mod regexp;
#[cfg(feature = "profile-memory")]
mod register_profile;
mod rewrite;
mod sequence;
mod string;
mod template;
use ast::FunctionCompiler;
#[derive(Clone, Debug)]
pub struct Diagnostic {
    source: String,
    message: String,
    span: Span,
}
impl Diagnostic {
    pub(crate) fn unsupported(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            message: message.into(),
            span: Span::default(),
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}..{}: {}",
            self.source, self.span.start, self.span.end, self.message
        )
    }
}
pub struct Engine;
pub(crate) enum StaticModuleThrow {
    Value(Constant),
    Error {
        name: String,
        message: Option<Constant>,
    },
}
#[derive(Clone, Debug, Default)]
pub(crate) struct StaticModulePlan {
    pub(crate) locals: Vec<(String, String)>,
    pub(crate) reexports: Vec<StaticModuleReexport>,
    pub(crate) requests: Vec<ModuleRequest>,
    pub(crate) has_top_level_await: bool,
}
#[derive(Clone, Debug)]
pub(crate) enum StaticModuleReexport {
    Named {
        source: String,
        imported: String,
        exported: String,
    },
    Star {
        source: String,
    },
    Namespace {
        source: String,
        exported: String,
    },
}
impl Engine {
    pub(crate) fn eval_var_declared_names(source: &str) -> Option<Vec<String>> {
        let allocator = Allocator::with_capacity(source.len());
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        let mut names = early::collect_var_names(&parsed.program.body);
        names.extend(parsed.program.body.iter().filter_map(|statement| {
            match statement {
                Statement::FunctionDeclaration(function) => function
                    .id
                    .as_ref()
                    .map(|identifier| identifier.name.to_string()),
                _ => None,
            }
        }));
        names.sort();
        names.dedup();
        Some(names)
    }

    pub(crate) fn eval_single_expression(source: &str) -> Option<&str> {
        let allocator = Allocator::with_capacity(source.len());
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if !parsed.diagnostics.is_empty() || parsed.program.body.len() != 1 {
            return None;
        }
        let Statement::ExpressionStatement(statement) = &parsed.program.body[0] else {
            return None;
        };
        let span = statement.expression.span();
        source.get(span.start as usize..span.end as usize)
    }

    pub(crate) fn eval_block_completion(source: &str) -> Option<(&str, &str)> {
        let allocator = Allocator::with_capacity(source.len());
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if !parsed.diagnostics.is_empty() || parsed.program.body.len() != 2 {
            return None;
        }
        let Statement::BlockStatement(block) = &parsed.program.body[0] else {
            return None;
        };
        if !block.body.iter().all(|statement| {
            matches!(
                statement,
                Statement::EmptyStatement(_) | Statement::ExpressionStatement(_)
            )
        }) {
            return None;
        }
        let Statement::ExpressionStatement(expression) = &parsed.program.body[1] else {
            return None;
        };
        let block_start = block.span.start as usize;
        let block_end = block.span.end as usize;
        let expression_start = expression.span.start as usize;
        let expression_end = expression.span.end as usize;
        Some((
            source.get(block_start + 1..block_end.checked_sub(1)?)?,
            source.get(expression_start..expression_end)?,
        ))
    }

    pub(crate) fn static_module_has_early_error(source: &str) -> bool {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, SourceType::mjs()).parse();
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
        if !parsed.diagnostics.is_empty() {
            return None;
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if !semantic.diagnostics.is_empty() {
            return None;
        }
        let mut plan = StaticModulePlan {
            requests: module_requests(&parsed.program.body),
            has_top_level_await: has_top_level_await(&parsed.program.body),
            ..StaticModulePlan::default()
        };
        for statement in &parsed.program.body {
            match statement {
                Statement::EmptyStatement(_) => {}
                Statement::ExportDeclaration(export) => {
                    static_declaration_exports(&export.declaration, &mut plan.locals)?;
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let local = match &export.declaration {
                        oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(
                            function,
                        ) => function
                            .id
                            .as_ref()
                            .map(|identifier| identifier.name.to_string())
                            .unwrap_or_else(|| module_default_binding(module_name)),
                        oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                            class
                                .id
                                .as_ref()
                                .map(|identifier| identifier.name.to_string())
                                .unwrap_or_else(|| module_default_binding(module_name))
                        }
                        _ if export.declaration.as_expression().is_some() => {
                            module_default_binding(module_name)
                        }
                        _ => return None,
                    };
                    plan.locals.push((local, "default".into()));
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
                        .extend(export.specifiers.iter().map(|specifier| {
                            StaticModuleReexport::Named {
                                source: source.clone(),
                                imported: module_export_name(&specifier.local),
                                exported: module_export_name(&specifier.exported),
                            }
                        }));
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
                Statement::ImportDeclaration(_) => {}
                _ => {}
            }
        }
        Some(plan)
    }

    pub(crate) fn static_module_throw(source: &str) -> Option<StaticModuleThrow> {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, SourceType::mjs()).parse();
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
                    exports.push(("\0rqj:module-default".into(), "default".into()));
                    bindings.insert("\0rqj:module-default".into(), value);
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
        plan.reexports.is_empty().then_some(plan.locals)
    }

    pub fn specialize(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Enabled,
            &[],
            SourceType::script(),
            false,
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
            &[],
            SourceType::script(),
            false,
        )
    }
    /// Compile a script whose atom indices extend an existing immutable
    /// program prefix. Dynamic sources use the same atom authority so values
    /// and property sites compare identically across program boundaries.
    pub fn specialize_unspecialized_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &[String],
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Disabled,
            atom_prefix,
            SourceType::unambiguous(),
            false,
        )
    }
    pub fn specialize_module_unspecialized(
        source: &str,
        name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Disabled)
    }
    /// Compile a module against an existing program's canonical atom prefix.
    pub fn specialize_module_unspecialized_with_atom_prefix(
        source: &str,
        name: &str,
        atom_prefix: &[String],
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(
            source,
            name,
            SpecializationMode::Disabled,
            atom_prefix,
            SourceType::mjs(),
            true,
        )
    }
    pub fn specialize_module(source: &str, name: &str) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_module_with_mode(source, name, SpecializationMode::Enabled)
    }
    /// Compile already-coerced Function-constructor parameter and body strings
    /// through the same OXC script pipeline used by ordinary source.
    pub fn specialize_dynamic_function(
        parameters: &str,
        body: &str,
        name: &str,
        atom_prefix: &[String],
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let source = format!("(function anonymous({parameters}) {{{body}\n}})");
        Self::specialize_with_mode(
            &source,
            name,
            SpecializationMode::Disabled,
            atom_prefix,
            SourceType::script(),
            false,
        )
    }
    fn specialize_module_with_mode(
        source: &str,
        name: &str,
        mode: SpecializationMode,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        Self::specialize_with_mode(source, name, mode, &[], SourceType::mjs(), true)
    }
    fn specialize_with_mode(
        source: &str,
        name: &str,
        mode: SpecializationMode,
        atom_prefix: &[String],
        source_type: SourceType,
        module_goal: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let normalized = early::normalize_hashbang(source);
        let allocator = Allocator::with_capacity(normalized.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &normalized, source_type).parse();
        if !parsed.diagnostics.is_empty() {
            return Err(parsed
                .diagnostics
                .into_iter()
                .map(|error| Diagnostic {
                    source: name.into(),
                    message: error.to_string(),
                    span: Span::default(),
                })
                .collect());
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if !semantic.diagnostics.is_empty() {
            return Err(semantic
                .diagnostics
                .into_iter()
                .map(|error| Diagnostic {
                    source: name.into(),
                    message: format!("SyntaxError: {error}"),
                    span: Span::default(),
                })
                .collect());
        }
        let private_name_ids = private_name_ids(&semantic.semantic);
        let program =
            Compiler::new_with_mode(name, &normalized, mode, atom_prefix, private_name_ids)
                .program(&parsed.program, module_goal);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            eprintln!(
                "{{\"kind\":\"rqj-oxc-memory\",\"used\":{},\"capacity\":{}}}",
                allocator.used_bytes(),
                allocator.capacity(),
            );
        }
        program
    }

    pub(crate) fn eval_parameter_early_error(source: &str, strict: bool) -> Option<String> {
        let allocator = Allocator::with_capacity(source.len().saturating_mul(2));
        let parsed = Parser::new(&allocator, source, SourceType::unambiguous()).parse();
        if !parsed.diagnostics.is_empty() {
            return Some(format!("SyntaxError: {}", parsed.diagnostics[0]));
        }
        early::parameter_early_error(&parsed.program, strict)
    }
}

pub(crate) fn module_default_binding(module_name: &str) -> String {
    format!("\0rqj:module-default:{module_name}")
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

fn has_top_level_await(statements: &[Statement<'_>]) -> bool {
    struct Finder(bool);
    impl<'a> oxc_ast_visit::Visit<'a> for Finder {
        fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
            self.0 = true;
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

fn same_module_path(module_name: &str, specifier: &str) -> bool {
    crate::module_identity::resolves_to(module_name, specifier)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecializationMode {
    Enabled,
    Disabled,
}

struct Compiler<'a> {
    source: &'a str,
    text: &'a str,
    mode: SpecializationMode,
    root_strict: bool,
    module_goal: bool,
    atoms: Vec<Rc<str>>,
    atom_index: FxHashMap<Rc<str>, Atom>,
    private_name_ids: FxHashMap<(u32, u32), u32>,
    constants: Vec<Constant>,
    constant_index: FxHashMap<ConstantKey, u32>,
    functions: Vec<Option<BcFunction>>,
    errors: Vec<Diagnostic>,
    cache_sites: u16,
    template_sites: u32,
    method_sites: Vec<MethodSiteSpec>,
    field_sites: Vec<FieldSite>,
    object_sites: Vec<ObjectSite>,
    superinstructions: Vec<Superinstruction>,
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

#[derive(Default)]
struct FunctionOptions<'a> {
    defaults: Option<&'a FormalParameters<'a>>,
    name_binding: Option<Atom>,
    async_function: bool,
    generator: bool,
    class_constructor: bool,
    derived_constructor: bool,
    non_constructible: bool,
    class_field_initializer: bool,
    instance_fields: Option<&'a [ClassField<'a>]>,
    instance_private_methods: Option<&'a [Atom]>,
    defer_instance_fields: bool,
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
    fn new_with_mode(
        source: &'a str,
        text: &'a str,
        mode: SpecializationMode,
        atom_prefix: &[String],
        private_name_ids: FxHashMap<(u32, u32), u32>,
    ) -> Self {
        let atoms: Vec<Rc<str>> = atom_prefix
            .iter()
            .map(|atom| Rc::from(atom.as_str()))
            .collect();
        let mut atom_index = FxHashMap::default();
        for (index, atom) in atoms.iter().enumerate() {
            atom_index.entry(Rc::clone(atom)).or_insert(index as Atom);
        }
        Self {
            source,
            text,
            mode,
            root_strict: false,
            module_goal: false,
            atoms,
            atom_index,
            private_name_ids,
            constants: vec![],
            constant_index: FxHashMap::default(),
            functions: vec![],
            errors: vec![],
            cache_sites: 0,
            template_sites: 0,
            method_sites: vec![],
            field_sites: vec![],
            object_sites: vec![],
            superinstructions: vec![],
        }
    }

    fn program(
        mut self,
        program: &Program<'_>,
        module_goal: bool,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
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
        self.root_strict = module_goal
            || program
                .directives
                .iter()
                .any(|directive| directive.directive == "use strict");
        if self.root_strict && early::strict_arguments_early_error(self.text) {
            self.reject(
                Span::default(),
                "SyntaxError: assignment to arguments is not allowed in strict mode",
            );
        }
        if self.root_strict && early::strict_eval_early_error(self.text) {
            self.reject(
                Span::default(),
                "SyntaxError: assignment to eval is not allowed in strict mode",
            );
        }
        if let Some(error) = early::regexp_early_error(program) {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::block_early_error(program) {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::strict_binding_early_error(program) {
            self.reject(Span::default(), error);
        }
        if let Some(error) = early::parameter_early_error(program, self.root_strict) {
            self.reject(Span::default(), error);
        }
        self.compile_function(
            None,
            &[],
            &program.body,
            &[],
            None,
            FunctionOptions {
                defaults: None,
                name_binding: None,
                async_function: module_goal,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
                class_field_initializer: false,
                instance_fields: None,
                instance_private_methods: None,
                defer_instance_fields: false,
                super_static: false,
                super_home: false,
                super_home_atom: None,
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
                    if matches!(
                        declaration.kind,
                        VariableDeclarationKind::Let | VariableDeclarationKind::Const
                    ) =>
                {
                    for declarator in &declaration.declarations {
                        early::collect_pattern_names(&declarator.id, &mut global_lexical_names);
                        if declaration.kind == VariableDeclarationKind::Const {
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
                        if matches!(
                            declaration.kind,
                            VariableDeclarationKind::Let | VariableDeclarationKind::Const
                        ) =>
                    {
                        for declarator in &declaration.declarations {
                            early::collect_pattern_names(&declarator.id, &mut global_lexical_names);
                            if declaration.kind == VariableDeclarationKind::Const {
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
            .filter_map(|name| self.atom_index.get(name.as_str()).copied())
            .collect();
        let global_immutable_atoms: Vec<_> = global_immutable_names
            .iter()
            .filter_map(|name| self.atom_index.get(name.as_str()).copied())
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
            .filter_map(|name| self.atom_index.get(name.as_str()).copied())
            .collect();
        let mut global_var_names = early::collect_var_names(&program.body);
        for statement in &program.body {
            if let Statement::FunctionDeclaration(function) = statement
                && let Some(identifier) = &function.id
            {
                global_var_names.push(identifier.name.to_string());
            }
        }
        global_var_names.sort();
        global_var_names.dedup();
        let global_var_atoms = global_var_names
            .iter()
            .filter_map(|name| self.atom_index.get(name.as_str()).copied())
            .collect();
        if let Some(Some(root)) = self.functions.first_mut() {
            root.global_lexical_atoms = global_lexical_atoms;
            root.global_immutable_atoms = global_immutable_atoms;
            root.global_var_atoms = global_var_atoms;
            root.global_function_atoms = global_function_atoms;
        }
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        let mut functions: Vec<_> = self.functions.into_iter().map(Option::unwrap).collect();
        if self.mode == SpecializationMode::Enabled {
            binding_time::apply(&mut functions);
            Self::apply_rewrites(
                &mut functions,
                &mut self.field_sites,
                &mut self.superinstructions,
            );
        }
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            capture_profile::report(&functions);
        }
        for function in &mut functions {
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
        }
        let register_roots = liveness::derive(
            &mut functions,
            &self.method_sites,
            &self.field_sites,
            &self.superinstructions,
        );
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
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
        let atoms = AtomTable::from_rcs(&self.atoms);
        self.constants.shrink_to_fit();
        functions.shrink_to_fit();
        method_sites.shrink_to_fit();
        method_arguments.shrink_to_fit();
        self.field_sites.shrink_to_fit();
        let program = ResidualProgram {
            specialized: self.mode == SpecializationMode::Enabled,
            module: self.module_goal,
            module_requests,
            module_imports,
            source_name: self.source.to_owned(),
            atoms,
            constants: self.constants,
            functions,
            cache_sites: self.cache_sites,
            method_sites,
            method_arguments,
            field_sites: self.field_sites,
            object_sites: self.object_sites,
            superinstructions: self.superinstructions,
            register_roots,
        };
        #[cfg(feature = "profile-aggregate")]
        debug_assert!(crate::profile::instruction_word_domains_fit(&program));
        Ok(program)
    }

    fn atom(&mut self, text: &str) -> Atom {
        if let Some(atom) = self.atom_index.get(text) {
            return *atom;
        }
        let atom = self.atoms.len() as Atom;
        let text: Rc<str> = Rc::from(text);
        self.atoms.push(Rc::clone(&text));
        self.atom_index.insert(text, atom);
        atom
    }

    fn private_name_atom(&mut self, span: Span) -> Atom {
        let name = match self.private_name_ids.get(&(span.start, span.end)) {
            Some(id) => format!("\0rqj:private:{}:{id}", self.source),
            None => {
                self.reject(span, "OXC did not resolve a private name identity");
                "\0rqj:private:unresolved".to_owned()
            }
        };
        self.atom(&name)
    }

    fn reserve_auto_accessor_name(&mut self, span: Span) {
        let id = self
            .private_name_ids
            .values()
            .copied()
            .max()
            .map_or(0, |current| current + 1);
        self.private_name_ids.insert((span.start, span.end), id);
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
        let site = self.cache_sites;
        self.cache_sites = self
            .cache_sites
            .checked_add(1)
            .expect("property cache limit");
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
            source: self.source.into(),
            message: message.into(),
            span,
        });
    }
    pub(super) fn collect_lexical_atoms(&mut self, body: &[Statement<'_>]) -> Vec<Atom> {
        let mut names = Vec::new();
        for statement in body {
            match statement {
                Statement::VariableDeclaration(declaration)
                    if matches!(
                        declaration.kind,
                        VariableDeclarationKind::Let | VariableDeclarationKind::Const
                    ) =>
                {
                    for item in &declaration.declarations {
                        early::collect_pattern_names(&item.id, &mut names);
                    }
                }
                Statement::ClassDeclaration(declaration) => {
                    if let Some(name) = &declaration.id {
                        names.push(name.name.to_string());
                    }
                }
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(declaration)
                        if matches!(
                            declaration.kind,
                            VariableDeclarationKind::Let | VariableDeclarationKind::Const
                        ) =>
                    {
                        for item in &declaration.declarations {
                            early::collect_pattern_names(&item.id, &mut names);
                        }
                    }
                    Declaration::ClassDeclaration(declaration) => {
                        if let Some(name) = &declaration.id {
                            names.push(name.name.to_string());
                        }
                    }
                    _ => {}
                },
                Statement::ExportDefaultDeclaration(export) => {
                    if let oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) =
                        &export.declaration
                        && let Some(identifier) = &class.id
                    {
                        names.push(identifier.name.to_string());
                    }
                }
                _ => {}
            }
        }
        let mut atoms = Vec::new();
        for name in names {
            let atom = self.atom(&name);
            if !atoms.contains(&atom) {
                atoms.push(atom);
            }
        }
        atoms
    }
    fn compile_function(
        &mut self,
        name: Option<&str>,
        params: &[String],
        body: &[Statement<'_>],
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        options: FunctionOptions<'_>,
    ) -> u32 {
        let id = self.functions.len() as u32;
        self.functions.push(None);
        let lexical_atoms = self.collect_lexical_atoms(body);
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
        self.collect_locals(body, &mut locals, root_strict);
        let name_binding = options.name_binding.and_then(|source_name| {
            if locals.contains(&source_name) {
                None
            } else {
                let name = self.atoms[source_name as usize].to_string();
                let binding = self.atom(&format!("{name}\0rqj:self-binding:{id}"));
                locals.push(binding);
                Some((source_name, binding))
            }
        });
        let has_arguments_binding = locals.contains(&self.atom("arguments"));
        let parameter_arguments_slot = options
            .defaults
            .filter(|parameters| {
                name != Some("\0rqj:arrow")
                    && has_arguments_binding
                    && FunctionCompiler::has_non_simple_parameters(parameters)
            })
            .map(|_| {
                let slot = locals.len() as u16;
                locals.push(self.atom("\0rqj:parameter-arguments"));
                slot
            });
        let arguments_slot = if has_arguments_binding {
            parameter_arguments_slot
        } else if parent.is_none() {
            None
        } else {
            locals.push(self.atom("arguments"));
            Some((locals.len() - 1) as u16)
        };
        let mut function = FunctionCompiler::new(
            self,
            locals,
            scopes.to_vec(),
            id,
            (options.super_static, options.super_home),
            options.async_function,
            options.generator,
            options.defer_instance_fields,
            parameter_arguments_slot,
            parameter_local_count,
            options.with_depth,
        );
        if let Some((name, binding)) = name_binding {
            function.push_lexical_bindings(FxHashMap::from_iter([(name, binding)]));
        }
        function.super_home_atom = options.super_home_atom;
        function.super_call_binds_this = options.derived_constructor;
        function.strict = root_strict;
        function.dynamic_eval = options
            .defaults
            .is_some_and(early::parameters_contain_direct_eval);
        if parent.is_none() {
            let mut names = Vec::new();
            for statement in body {
                match statement {
                    Statement::VariableDeclaration(declaration)
                        if matches!(
                            declaration.kind,
                            VariableDeclarationKind::Let | VariableDeclarationKind::Const
                        ) =>
                    {
                        for item in &declaration.declarations {
                            early::collect_pattern_names(&item.id, &mut names);
                        }
                    }
                    Statement::ClassDeclaration(declaration) => {
                        if let Some(name) = &declaration.id {
                            names.push(name.name.to_string());
                        }
                    }
                    Statement::ExportDeclaration(export) => match &export.declaration {
                        Declaration::VariableDeclaration(declaration)
                            if matches!(
                                declaration.kind,
                                VariableDeclarationKind::Let | VariableDeclarationKind::Const
                            ) =>
                        {
                            for item in &declaration.declarations {
                                early::collect_pattern_names(&item.id, &mut names);
                            }
                        }
                        Declaration::ClassDeclaration(declaration) => {
                            if let Some(name) = &declaration.id {
                                names.push(name.name.to_string());
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            let slots: Vec<_> = names
                .iter()
                .filter_map(|name| {
                    function
                        .local_slots
                        .get(&function.owner.atom(name))
                        .copied()
                })
                .collect();
            for slot in slots {
                function.emit(Op::InitializeTdz, 0, 0, 0, u32::from(slot));
            }
        }
        if let Some(defaults) = options.defaults {
            function.emit_parameter_bindings(defaults);
        }
        let parameter_end_pc = function.code.len() as u32;
        function.emit_hoisted(body);
        if options.implicit_super {
            function.emit_implicit_super(
                options.instance_fields.unwrap_or_default(),
                options.instance_private_methods.unwrap_or_default(),
            );
        } else if let Some(fields) = options
            .instance_fields
            .filter(|_| !options.defer_instance_fields)
        {
            function
                .emit_instance_fields(fields, options.instance_private_methods.unwrap_or_default());
        }
        function.statements(body);
        if options.defer_instance_fields {
            let edges = std::mem::take(&mut function.deferred_instance_field_edges);
            let skip_blocks = (!edges.is_empty()).then(|| function.emit(Op::Jump, 0, 0, 0, 0));
            if !edges.is_empty() {
                function.next_reg = function.max_reg;
            }
            for (edge, continuation) in edges {
                function.patch(edge);
                function.emit_instance_fields(
                    options.instance_fields.unwrap_or_default(),
                    options.instance_private_methods.unwrap_or_default(),
                );
                let resume = function.emit(Op::Jump, 0, 0, 0, 0);
                function.patch_instruction(resume, continuation);
            }
            if let Some(skip_blocks) = skip_blocks {
                function.patch(skip_blocks);
            }
        }
        function.emit_disposal();
        let undefined = function.literal(Constant::Undefined);
        function.emit(Op::Return, undefined, 0, 0, 0);
        let arguments_slot = arguments_slot.filter(|slot| {
            function.code.iter().any(|instruction| {
                instruction.op() == Op::LoadLocal && instruction.imm() == u32::from(*slot)
            }) || function.wide.iter().any(|instruction| {
                instruction.op() == Op::LoadLocal && instruction.imm() == u32::from(*slot)
            })
        });
        let captures_locals = function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
            || function
                .wide
                .iter()
                .any(|instruction| instruction.op() == Op::MakeClosure);
        if captures_locals {
            for instruction in &mut function.code {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
            for instruction in &mut function.wide {
                let op = match instruction.op() {
                    Op::LoadLocal => Op::LoadEnvLocal,
                    Op::StoreLocal => Op::StoreEnvLocal,
                    other => other,
                };
                instruction.set_op(op);
            }
        }
        let simple_parameters = options.defaults.is_none_or(|formal| {
            formal.rest.is_none()
                && formal.items.iter().all(|item| {
                    item.initializer.is_none()
                        && matches!(item.pattern, BindingPattern::BindingIdentifier(_))
                })
        });
        let arguments_slot = arguments_slot.map(|slot| {
            if !root_strict && !options.rest_override && simple_parameters {
                slot | MAPPED_ARGUMENTS_BIT
            } else {
                slot
            }
        });
        let parameter_atoms = options.defaults.map_or_else(Vec::new, |formal| {
            FunctionCompiler::parameter_bound_names(formal)
                .iter()
                .map(|name| function.owner.atom(name))
                .collect()
        });
        let result = BcFunction {
            parent,
            name: name.map(|value| function.owner.atom(value)),
            params: params.len() as u16,
            length: options.defaults.map_or(params.len(), Self::formal_length) as u16,
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
            super_home_atom: options.super_home_atom,
            constructible: !options.non_constructible,
            class_field_initializer: options.class_field_initializer,
            parameter_eval_arguments_error: function.parameter_eval_arguments_error,
            arguments_slot,
            strict: root_strict,
            locals: function.locals.len() as u16,
            local_atoms: function.locals.clone(),
            lexical_atoms,
            global_lexical_atoms: Vec::new(),
            global_var_atoms: Vec::new(),
            global_function_atoms: Vec::new(),
            global_immutable_atoms: Vec::new(),
            code: function.code,
            wide: function.wide,
            registers: function.max_reg,
            dispatch: DispatchClass::General,
            handlers: function.handlers,
            register_root_offset: u32::MAX,
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
        field_sites: &mut Vec<FieldSite>,
        superinstructions: &mut Vec<Superinstruction>,
    ) {
        for function in functions {
            if function.wide.is_empty() {
                rewrite::apply(function, field_sites, superinstructions);
            }
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
}
#[cfg(test)]
mod tests;
