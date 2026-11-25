//! TypeScript as the compiler's own input language.
//!
//! A typed tree holds two kinds of nodes. Declarations that only describe values
//! (interfaces, aliases, overload signatures, `declare`, abstract members, index
//! signatures, type-only module syntax) move into the program's declared
//! environment (`Declarations`), where they stay addressable by name and source
//! range; code generation never sees them. Declarations with run-time meaning
//! (enums, namespaces, parameter properties, `import x = …`, `export =`) become
//! the JavaScript nodes of that meaning, in place, with every annotation left on
//! its node: a parameter property keeps its declared type on its parameter, an
//! enum's folded member values are recorded as constants.
//!
//! Every parse of typed text goes through `lazy_bodies::parse`, so a body compiled
//! later (lazily, streamed, on first call) is admitted exactly as at load.

use super::enum_ast;
use super::module_ast;
use super::enum_folding;
use super::enums::{self, Computed, Member, Value};
use super::namespace_ast;
use oxc_allocator::{Allocator, Box as ArenaBox, TakeIn, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::bytecode::{DeclaredKind, DeclaredTypes, EnumConstant};

mod decorators;

/// Admit a typed tree: the declared environment out, run-time meaning in.
pub(in crate::compile) fn admit<'a>(allocator: &'a Allocator, program: &mut Program<'a>, declarations: &mut DeclaredTypes) {
    // Before the class bodies change: signature-only members and parameter modifiers are still in place.
    {
        // The saved top-level names of the program, or of the program a body belongs to, shadowed by
        // what this tree declares; saved again for the bodies compiled from it (`TypeNames::resolved`).
        let names = super::TypeNames::of(program, declarations.type_names.clone());
        super::fields::collect(program, &names, declarations);
        super::signatures::collect(program, &names, declarations);
        declarations.type_names = Some(names.resolved());
    }
    let bindings = if program.source_text.contains("enum") { binding_counts(program) } else { FxHashMap::default() };
    let folded = enum_folding::foldable(program, &bindings);
    let mut admit = Admit {
        allocator, declarations, type_names: FxHashSet::default(), lexical: false,
        bindings,
        const_enums: FxHashMap::default(),
        folded,
        imports: if program.source_text.contains('@') { decorators::import_bindings(program) } else { FxHashMap::default() },
        metadata_namespaces: Vec::new(),
    };
    admit.visit_program(program);
    admit.drop_type_exports(&mut program.body);
    admit.import_metadata_namespaces(&mut program.body);
}

struct Admit<'a, 'd> {
    allocator: &'a Allocator,
    declarations: &'d mut DeclaredTypes,
    /// Top-level names that denote only types, so `export { T }` exports nothing at run time.
    type_names: FxHashSet<String>,
    /// The current statement list is a block's (or a namespace body's): its generated
    /// bindings are `let`, so they stay in it as tsc's function-scoped ones do.
    lexical: bool,
    /// How many bindings of each name the program declares.
    bindings: FxHashMap<String, u32>,
    /// The folded members of each `const enum` whose name binds nothing else: tsc's
    /// semantics inline their reads, so `E.X` is the constant (no load at run time). A plain
    /// enum in `folded` inlines the same way once its declaration is admitted, so the reads
    /// after it see the constants.
    const_enums: FxHashMap<String, FxHashMap<String, Value>>,
    /// The plain enums whose member reads may fold (`enum_folding`).
    folded: FxHashSet<String>,
    /// Value import bindings by local name: source, and imported name (`None`: a namespace).
    /// Decorator metadata reads an imported class through a namespace import of its module,
    /// so a name that turns out to be a type there links (as `Object`) instead of failing.
    imports: FxHashMap<String, (String, Option<String>)>,
    /// The namespace imports metadata reads through: source, local name.
    metadata_namespaces: Vec<(String, String)>,
}

/// How a declaration is exported: not, from the module, or from namespace `N`.
#[derive(Clone, Copy)]
pub(super) enum Export<'n> {
    None,
    Module,
    Namespace(&'n str),
}

/// A statement the admission produced, and whether its children were admitted already.
type Produced<'a> = (Statement<'a>, bool);

impl<'a> Admit<'a, '_> {
    fn keyword(&self) -> &'static str {
        if self.lexical { "let" } else { "var" }
    }

    fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: Span) {
        self.declarations.declare(kind, name, span);
    }

    /// JavaScript statements of `text`, at `span` of the typed source.
    fn javascript(&self, text: &str, span: Span) -> ArenaVec<'a, Statement<'a>> {
        let text = self.allocator.alloc_str(text);
        let parsed = oxc_parser::Parser::new(self.allocator, text, SourceType::mjs()).parse();
        debug_assert!(parsed.diagnostics.is_empty(), "admitted TypeScript emits valid JavaScript: {text}");
        let mut program = parsed.program;
        SetSpans(span).visit_program(&mut program);
        program.body
    }

    /// Admit a statement list (of a namespace body when `namespace` names it).
    fn statements(&mut self, input: ArenaVec<'a, Statement<'a>>, namespace: Option<&str>) -> ArenaVec<'a, Statement<'a>> {
        let mut seen = FxHashSet::default();
        let mut output = ArenaVec::with_capacity_in(input.len(), &self.allocator);
        for statement in input {
            for (mut statement, admitted) in self.statement(statement, namespace, &mut seen) {
                if !admitted { self.visit_statement(&mut statement); }
                output.push(statement);
            }
        }
        output
    }

    fn statement(&mut self, statement: Statement<'a>, namespace: Option<&str>, seen: &mut FxHashSet<String>) -> Vec<Produced<'a>> {
        match statement {
            Statement::ImportDeclaration(import) => self.import(import),
            Statement::ExportDeclaration(mut export) => {
                let declaration = export.declaration.take_in(&self.allocator);
                let how = namespace.map_or(Export::Module, Export::Namespace);
                self.declaration(declaration, export.span, how, Some(export), seen)
            }
            Statement::ExportNamedDeclaration(export) => self.export_list(export),
            Statement::ExportFromDeclaration(export) => self.export_from(export),
            Statement::ExportAllDeclaration(export) if export.export_kind.is_type() => {
                self.declare(DeclaredKind::ModuleType, None, export.span);
                Vec::new()
            }
            Statement::ExportDefaultDeclaration(export) => self.export_default(export),
            Statement::TSExportAssignment(mut assignment) => {
                let span = assignment.span;
                let expression = assignment.expression.take_in(&self.allocator);
                vec![(module_ast::export_assignment(self.allocator, expression, span), false)]
            }
            Statement::TSNamespaceExportDeclaration(declaration) => {
                self.declare(DeclaredKind::ModuleType, Some(declaration.id.name.as_str()), declaration.span);
                Vec::new()
            }
            statement if statement.is_declaration() => {
                let declaration = Declaration::try_from(statement).unwrap_or_else(|()| unreachable!("a declaration"));
                let span = declaration.span();
                self.declaration(declaration, span, Export::None, None, seen)
            }
            statement => vec![(statement, false)],
        }
    }

    /// Record a declaration that has no run-time meaning; false when it has one.
    fn type_declaration(&mut self, declaration: &Declaration<'a>) -> bool {
        let (kind, name, span) = match declaration {
            Declaration::TSTypeAliasDeclaration(alias) => (DeclaredKind::TypeAlias, Some(alias.id.name.as_str()), alias.span),
            Declaration::TSInterfaceDeclaration(interface) => (DeclaredKind::Interface, Some(interface.id.name.as_str()), interface.span),
            Declaration::TSGlobalDeclaration(global) => (DeclaredKind::Ambient, None, global.span),
            Declaration::TSExternalModuleDeclaration(module) => (DeclaredKind::Ambient, Some(module.id.value.as_str()), module.span),
            Declaration::VariableDeclaration(variables) if variables.declare => (DeclaredKind::Ambient, None, variables.span),
            Declaration::FunctionDeclaration(function) if function.declare || function.body.is_none() => {
                (DeclaredKind::FunctionSignature, function.id.as_ref().map(|id| id.name.as_str()), function.span)
            }
            Declaration::ClassDeclaration(class) if class.declare => (DeclaredKind::Ambient, class.id.as_ref().map(|id| id.name.as_str()), class.span),
            Declaration::TSEnumDeclaration(enumeration) if enumeration.declare => (DeclaredKind::Ambient, Some(enumeration.id.name.as_str()), enumeration.span),
            Declaration::TSNamespaceDeclaration(namespace) if namespace.declare || !namespace_instantiated(namespace) => {
                (DeclaredKind::Namespace, Some(namespace.id.name.as_str()), namespace.span)
            }
            Declaration::TSImportEqualsDeclaration(alias) if alias.import_kind.is_type() => (DeclaredKind::ModuleType, Some(alias.id.name.as_str()), alias.span),
            _ => return false,
        };
        if let Some(name) = name
            && matches!(kind, DeclaredKind::Interface | DeclaredKind::TypeAlias | DeclaredKind::Namespace)
        {
            self.type_names.insert(name.to_owned());
        }
        self.declare(kind, name, span);
        true
    }

    /// Admit a declaration whose statement covers `span`; `export` is its `export` wrapper.
    fn declaration(&mut self, declaration: Declaration<'a>, span: Span, how: Export<'_>, export: Option<ArenaBox<'a, ExportDeclaration<'a>>>, seen: &mut FxHashSet<String>) -> Vec<Produced<'a>> {
        if self.type_declaration(&declaration) { return Vec::new(); }
        match declaration {
            Declaration::TSEnumDeclaration(enumeration) => self.enumeration(enumeration, how, seen),
            Declaration::TSNamespaceDeclaration(namespace) => self.namespace(namespace, span, how, seen),
            Declaration::TSImportEqualsDeclaration(alias) => self.import_equals(alias, span, how),
            declaration => {
                let declaration = match declaration {
                    Declaration::ClassDeclaration(class) => match self.decorated_class(class, span, how) {
                        Ok((name, produced)) => {
                            seen.insert(name);
                            return produced;
                        }
                        Err(class) => Declaration::ClassDeclaration(class),
                    },
                    declaration => declaration,
                };
                let bindings = bound_names(&declaration);
                seen.extend(bindings.iter().cloned());
                match (how, export) {
                    (Export::Namespace(namespace), _) => {
                        let mut produced = vec![(Statement::from(declaration), false)];
                        let published = namespace_ast::publish(self.allocator, span, namespace, &bindings);
                        produced.extend(published.into_iter().map(|statement| (statement, true)));
                        produced
                    }
                    (_, Some(mut export)) => {
                        export.declaration = declaration;
                        vec![(Statement::ExportDeclaration(export), false)]
                    }
                    (_, None) => vec![(Statement::from(declaration), false)],
                }
            }
        }
    }

    /// Whether a whole import or export is type-only (`import type`, `export type`): it is
    /// declared as a module type and leaves no statement.
    fn type_only(&mut self, kind: ImportOrExportKind, span: Span) -> bool {
        if kind.is_type() {
            self.declare(DeclaredKind::ModuleType, None, span);
        }
        kind.is_type()
    }

    fn import(&mut self, mut import: ArenaBox<'a, ImportDeclaration<'a>>) -> Vec<Produced<'a>> {
        if self.type_only(import.import_kind, import.span) {
            return Vec::new();
        }
        // An import whose names were all types keeps its evaluation edge.
        if let Some(specifiers) = &mut import.specifiers {
            let declared = &mut *self.declarations;
            specifiers.retain(|specifier| match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) if named.import_kind.is_type() => {
                    declared.declare(DeclaredKind::ModuleType, Some(named.local.name.as_str()), named.span);
                    false
                }
                _ => true,
            });
        }
        vec![(Statement::ImportDeclaration(import), true)]
    }

    fn export_list(&mut self, mut export: ArenaBox<'a, ExportNamedDeclaration<'a>>) -> Vec<Produced<'a>> {
        if self.type_only(export.export_kind, export.span) {
            return Vec::new();
        }
        strip_type_only_exports(self.declarations, &mut export.specifiers);
        vec![(Statement::ExportNamedDeclaration(export), true)]
    }

    fn export_from(&mut self, mut export: ArenaBox<'a, ExportFromDeclaration<'a>>) -> Vec<Produced<'a>> {
        if self.type_only(export.export_kind, export.span) {
            return Vec::new();
        }
        strip_type_only_exports(self.declarations, &mut export.specifiers);
        vec![(Statement::ExportFromDeclaration(export), true)]
    }

    fn export_default(&mut self, export: ArenaBox<'a, ExportDefaultDeclaration<'a>>) -> Vec<Produced<'a>> {
        let signature = match &export.declaration {
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => Some((DeclaredKind::Interface, interface.span)),
            ExportDefaultDeclarationKind::FunctionDeclaration(function) if function.body.is_none() => Some((DeclaredKind::FunctionSignature, function.span)),
            _ => None,
        };
        if let Some((kind, span)) = signature {
            self.declare(kind, Some("default"), span);
            return Vec::new();
        }
        let mut export = export;
        if let ExportDefaultDeclarationKind::ClassDeclaration(_) = &export.declaration {
            let ExportDefaultDeclarationKind::ClassDeclaration(class) = export.declaration.take_in(&self.allocator) else { unreachable!() };
            match self.decorated_class(class, export.span, Export::None) {
                Ok((name, mut produced)) => {
                    produced.push((module_ast::export_default_name(self.allocator, &name, export.span), true));
                    return produced;
                }
                Err(class) => export.declaration = ExportDefaultDeclarationKind::ClassDeclaration(class),
            }
        }
        vec![(Statement::ExportDefaultDeclaration(export), false)]
    }

    /// `export { T }` of a name that denotes only a type exports nothing at run time.
    fn drop_type_exports(&mut self, body: &mut ArenaVec<'a, Statement<'a>>) {
        if self.type_names.is_empty() { return; }
        for statement in body.iter_mut() {
            let Statement::ExportNamedDeclaration(export) = statement else { continue };
            let names = &self.type_names;
            let declared = &mut *self.declarations;
            export.specifiers.retain(|specifier| {
                let local = specifier.local.name();
                if !names.contains(local.as_ref()) { return true; }
                declared.declare(DeclaredKind::ModuleType, Some(&local), specifier.span);
                false
            });
        }
    }

    /// `import x = require("m")` / `import x = A.B`: a binding to that value.
    fn import_equals(&mut self, alias: ArenaBox<'a, TSImportEqualsDeclaration<'a>>, span: Span, how: Export<'_>) -> Vec<Produced<'a>> {
        let statement = module_ast::import_equals(self.allocator, alias.unbox(), how, self.lexical, span);
        vec![(statement, true)]
    }
}

fn qualified_text(name: &TSQualifiedName<'_>) -> String {
    let left = match &name.left {
        TSTypeName::IdentifierReference(identifier) => identifier.name.to_string(),
        TSTypeName::QualifiedName(left) => qualified_text(left),
        TSTypeName::ThisExpression(_) => "this".to_owned(),
    };
    format!("{left}.{}", name.right.name)
}

fn bound_names(declaration: &Declaration<'_>) -> Vec<String> {
    match declaration {
        Declaration::VariableDeclaration(variables) => variables.declarations.iter()
            .flat_map(|declarator| declarator.id.get_binding_identifiers())
            .map(|binding| binding.name.to_string()).collect(),
        Declaration::FunctionDeclaration(function) => function.id.iter().map(|id| id.name.to_string()).collect(),
        Declaration::ClassDeclaration(class) => class.id.iter().map(|id| id.name.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Replace the placeholders `$$quench_ts_<n>` of generated statements with `values[n]`.
fn fill<'a>(statements: &mut ArenaVec<'a, Statement<'a>>, values: Vec<Expression<'a>>) {
    struct Fill<'a> { values: Vec<Option<Expression<'a>>> }
    impl<'a> VisitMut<'a> for Fill<'a> {
        fn visit_expression(&mut self, expression: &mut Expression<'a>) {
            if let Expression::Identifier(identifier) = expression
                && let Some(index) = identifier.name.strip_prefix("$$quench_ts_").and_then(|n| n.parse::<usize>().ok())
                && let Some(value) = self.values.get_mut(index).and_then(Option::take)
            {
                *expression = value;
                return;
            }
            walk_mut::walk_expression(self, expression);
        }
    }
    let mut fill = Fill { values: values.into_iter().map(Some).collect() };
    for statement in statements.iter_mut() { fill.visit_statement(statement); }
}

struct SetSpans(Span);

impl<'a> VisitMut<'a> for SetSpans {
    fn visit_span(&mut self, span: &mut Span) {
        *span = self.0;
    }
}

impl<'a> Admit<'a, '_> {
    /// An enum: one object literal with fixed keys when every member folds (a stable
    /// shape, member reads are field reads), else tsc's member-by-member emit.
    fn enumeration(&mut self, mut enumeration: ArenaBox<'a, TSEnumDeclaration<'a>>, how: Export<'_>, seen: &mut FxHashSet<String>) -> Vec<Produced<'a>> {
        let name = enumeration.id.name.to_string();
        let first = seen.insert(name.clone());
        let (members, values) = self.enum_members(&mut enumeration, &name);
        let constants: Vec<(String, EnumConstant)> = members.iter().filter_map(|(member, value)| match value {
            Member::Constant(Value::Number(number)) => Some((member.clone(), EnumConstant::Number(*number))),
            Member::Constant(Value::String(text)) => Some((member.clone(), EnumConstant::String(text.as_str().into()))),
            Member::Computed(_) => None,
        }).collect();
        self.declarations.enum_constants.extend(constants.into_iter().map(|(member, value)| (Box::from(name.as_str()), Box::from(member), value)));
        self.declare(DeclaredKind::Enum, Some(&name), enumeration.span);
        let inline = if enumeration.r#const {
            self.bindings.get(&name) == Some(&1)
        } else {
            self.folded.contains(&name) && members.iter().all(|(_, value)| matches!(value, Member::Constant(_)))
        };
        if inline {
            let literals = members.iter().filter_map(|(member, value)| match value {
                Member::Constant(value) => enums::display(value).map(|_| (member.clone(), value.clone())),
                Member::Computed(_) => None,
            });
            self.const_enums.entry(name.clone()).or_default().extend(literals);
        }
        let statements = enum_ast::statements(self.allocator, &name, &members, values, first, how, self.lexical, enumeration.span);
        statements.into_iter().map(|statement| (statement, false)).collect()
    }

    /// Each member's value: folded, or a placeholder for its initializer with earlier
    /// members qualified (`A` → `E["A"]`).
    fn enum_members(&self, enumeration: &mut TSEnumDeclaration<'a>, enum_name: &str) -> (Vec<(String, Member)>, Vec<Expression<'a>>) {
        let mut known: FxHashMap<String, Value> = FxHashMap::default();
        let mut names: Vec<String> = Vec::new();
        let mut members = Vec::new();
        let mut values = Vec::new();
        let mut previous: Option<(String, Option<f64>)> = None;
        for member in enumeration.body.members.iter_mut() {
            let Some(name) = enums::member_name(&member.id) else { continue };
            let value = match (&mut member.initializer, &previous) {
                (Some(initializer), _) => match enums::fold(initializer, enum_name, &known) {
                    Some(value) => Member::Constant(value),
                    None => {
                        let mut expression = initializer.take_in(&self.allocator);
                        Qualify { allocator: self.allocator, enum_name, names: &names }.visit_expression(&mut expression);
                        values.push(expression);
                        Member::Computed(Computed::Initializer(values.len() - 1))
                    }
                },
                (None, None) => Member::Constant(Value::Number(0.0)),
                (None, Some((_, Some(number)))) => Member::Constant(Value::Number(number + 1.0)),
                (None, Some((previous_name, None))) => Member::Computed(Computed::Successor(previous_name.clone())),
            };
            let number = match &value { Member::Constant(Value::Number(number)) => Some(*number), _ => None };
            if let Member::Constant(constant) = &value { known.insert(name.clone(), constant.clone()); }
            previous = Some((name.clone(), number));
            names.push(name.clone());
            members.push((name, value));
        }
        (members, values)
    }
}

/// Earlier members named bare inside an initializer read the enum object.
struct Qualify<'s, 'a> {
    allocator: &'a Allocator,
    enum_name: &'s str,
    names: &'s [String],
}

impl<'a> VisitMut<'a> for Qualify<'_, 'a> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::Identifier(identifier) = expression
            && self.names.iter().any(|name| name == identifier.name.as_str())
        {
            *expression = enum_ast::read(self.allocator, identifier.span, self.enum_name, identifier.name.as_str());
            return;
        }
        walk_mut::walk_expression(self, expression);
    }
}

impl<'a> Admit<'a, '_> {
    /// `namespace A.B { … }`: the namespace objects, then the body as a block in which
    /// each export is published on its object after its declaration.
    fn namespace(&mut self, namespace: ArenaBox<'a, TSNamespaceDeclaration<'a>>, span: Span, how: Export<'_>, seen: &mut FxHashSet<String>) -> Vec<Produced<'a>> {
        let mut names = vec![namespace.id.name.to_string()];
        let mut body = namespace.unbox().body;
        let block = loop {
            match body {
                TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => {
                    names.push(inner.id.name.to_string());
                    body = inner.unbox().body;
                }
                TSNamespaceDeclarationBody::TSModuleBlock(block) => break block,
            }
        };
        let first = seen.insert(names[0].clone());
        let kind = if self.lexical { VariableDeclarationKind::Let } else { VariableDeclarationKind::Var };
        let mut produced: Vec<Produced<'a>> = namespace_ast::head(self.allocator, span, &names[0], first, how, kind)
            .into_iter().map(|statement| (statement, true)).collect();
        let innermost = names.last().cloned().unwrap_or_default();
        let lexical = std::mem::replace(&mut self.lexical, true);
        let statements = self.statements(block.unbox().body, Some(&innermost));
        self.lexical = lexical;
        produced.push((namespace_ast::body(self.allocator, span, &names, statements), true));
        produced
    }
}

/// Whether a namespace has run-time content (tsc's "instantiated" namespace).
fn namespace_instantiated(namespace: &TSNamespaceDeclaration<'_>) -> bool {
    match &namespace.body {
        TSNamespaceDeclarationBody::TSModuleBlock(block) => block.body.iter().any(statement_instantiated),
        TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => !inner.declare && namespace_instantiated(inner),
    }
}

fn statement_instantiated(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::EmptyStatement(_) => false,
        Statement::ImportDeclaration(import) => !import.import_kind.is_type(),
        Statement::ExportDeclaration(export) => declaration_instantiated(&export.declaration),
        Statement::ExportNamedDeclaration(export) => !export.export_kind.is_type(),
        _ => statement.as_declaration().is_none_or(declaration_instantiated),
    }
}

fn declaration_instantiated(declaration: &Declaration<'_>) -> bool {
    match declaration {
        Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_)
        | Declaration::TSGlobalDeclaration(_) | Declaration::TSExternalModuleDeclaration(_) => false,
        Declaration::VariableDeclaration(variables) => !variables.declare,
        Declaration::FunctionDeclaration(function) => !function.declare && function.body.is_some(),
        Declaration::ClassDeclaration(class) => !class.declare,
        Declaration::TSEnumDeclaration(enumeration) => !enumeration.declare,
        Declaration::TSNamespaceDeclaration(namespace) => !namespace.declare && namespace_instantiated(namespace),
        Declaration::TSImportEqualsDeclaration(alias) => !alias.import_kind.is_type(),
    }
}

impl<'a> Admit<'a, '_> {
    /// Class members: signature-only ones move to the declared environment; parameter
    /// properties become `this.x = x;` at the constructor's start (after its root
    /// `super(…)` call when derived), their annotations kept on their parameters.
    fn class_members(&mut self, class: &mut Class<'a>) {
        let declared = &mut *self.declarations;
        class.body.body.retain(|element| {
            if !signature_member(element) { return true; }
            let name = element.property_key().and_then(PropertyKey::static_name);
            declared.declare(DeclaredKind::ClassMember, name.as_deref(), element.span());
            false
        });
        let derived = class.heritage.is_some();
        for element in class.body.body.iter_mut() {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if !method.kind.is_constructor() { continue; }
            let names = parameter_properties(&mut method.value.params);
            let span = method.value.params.span;
            let Some(body) = method.value.body.as_mut().filter(|_| !names.is_empty()) else { continue };
            let at = if derived { root_super_call(body).map_or(0, |index| index + 1) } else { 0 };
            for (offset, statement) in module_ast::parameter_assignments(self.allocator, &names, span).into_iter().enumerate() {
                body.statements.insert(at + offset, statement);
            }
        }
    }
}

/// `E.X` or `E["X"]`: the enum and member a read names.
fn enum_member_read<'e>(expression: &'e Expression<'_>) -> Option<(&'e str, &'e str)> {
    match expression {
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some((object.name.as_str(), member.property.name.as_str())),
            _ => None,
        },
        Expression::ComputedMemberExpression(member) => match (&member.object, &member.expression) {
            (Expression::Identifier(object), Expression::StringLiteral(key)) => Some((object.name.as_str(), key.value.as_str())),
            _ => None,
        },
        _ => None,
    }
}

/// How many bindings of each name `program` declares (an inlined `const enum` name must
/// denote the enum wherever it is read).
fn binding_counts(program: &Program<'_>) -> FxHashMap<String, u32> {
    use oxc_ast_visit::Visit;
    struct Counts(FxHashMap<String, u32>);
    impl<'a> Visit<'a> for Counts {
        fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
            *self.0.entry(identifier.name.to_string()).or_default() += 1;
        }
    }
    let mut counts = Counts(FxHashMap::default());
    counts.visit_program(program);
    counts.0
}

/// The assignment target an expression under type assertions denotes, if it denotes one.
fn plain_target(expression: Expression<'_>) -> Result<SimpleAssignmentTarget<'_>, Expression<'_>> {
    match expression {
        Expression::Identifier(identifier) => Ok(SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier)),
        Expression::ParenthesizedExpression(inner) => plain_target(inner.unbox().expression),
        Expression::TSAsExpression(inner) => plain_target(inner.unbox().expression),
        Expression::TSSatisfiesExpression(inner) => plain_target(inner.unbox().expression),
        Expression::TSNonNullExpression(inner) => plain_target(inner.unbox().expression),
        Expression::TSTypeAssertion(inner) => plain_target(inner.unbox().expression),
        expression if expression.is_member_expression() => {
            Ok(SimpleAssignmentTarget::from(MemberExpression::try_from(expression).unwrap_or_else(|_| unreachable!("a member expression"))))
        }
        expression => Err(expression),
    }
}

fn signature_member(element: &ClassElement<'_>) -> bool {
    match element {
        ClassElement::MethodDefinition(method) => method.value.body.is_none(),
        ClassElement::PropertyDefinition(property) => property.declare || property.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition,
        ClassElement::AccessorProperty(accessor) => accessor.r#type == AccessorPropertyType::TSAbstractAccessorProperty,
        ClassElement::TSIndexSignature(_) => true,
        ClassElement::StaticBlock(_) => false,
    }
}

/// The parameter properties' names in order, their modifiers consumed (the declared
/// type stays on the parameter). A decorated one stays for the decorator check.
fn parameter_properties(params: &mut FormalParameters<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for parameter in params.items.iter_mut() {
        let modified = parameter.accessibility.is_some() || parameter.readonly || parameter.r#override;
        if !modified || !parameter.decorators.is_empty() { continue; }
        let Some(name) = parameter.pattern.get_identifier_name() else { continue };
        parameter.accessibility = None;
        parameter.readonly = false;
        parameter.r#override = false;
        names.push(name.to_string());
    }
    names
}

fn root_super_call(body: &FunctionBody<'_>) -> Option<usize> {
    body.statements.iter().position(|statement| matches!(statement,
        Statement::ExpressionStatement(expression) if matches!(expression.expression.without_parentheses(),
            Expression::CallExpression(call) if matches!(call.callee, Expression::Super(_)))))
}

impl<'a> VisitMut<'a> for Admit<'a, '_> {
    fn visit_statements(&mut self, statements: &mut ArenaVec<'a, Statement<'a>>) {
        let input = std::mem::replace(statements, ArenaVec::new_in(&self.allocator));
        *statements = self.statements(input, None);
    }

    fn visit_class(&mut self, class: &mut Class<'a>) {
        self.class_members(class);
        walk_mut::walk_class(self, class);
    }

    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if !self.const_enums.is_empty()
            && let Some((enumeration, member)) = enum_member_read(expression)
            && let Some(value) = self.const_enums.get(enumeration).and_then(|members| members.get(member))
        {
            let span = expression.span();
            *expression = enum_ast::constant(self.allocator, span, value);
            return;
        }
        walk_mut::walk_expression(self, expression);
    }

    /// `x! = v`, `(x as T) = v`: the target is the expression the assertion wraps.
    fn visit_simple_assignment_target(&mut self, target: &mut SimpleAssignmentTarget<'a>) {
        if let Some(inner) = target.get_expression_mut() {
            match plain_target(inner.take_in(&self.allocator)) {
                Ok(plain) => *target = plain,
                Err(expression) => *inner = expression,
            }
        }
        walk_mut::walk_simple_assignment_target(self, target);
    }

    fn visit_block_statement(&mut self, block: &mut BlockStatement<'a>) {
        let lexical = std::mem::replace(&mut self.lexical, true);
        walk_mut::walk_block_statement(self, block);
        self.lexical = lexical;
    }

    fn visit_function_body(&mut self, body: &mut FunctionBody<'a>) {
        let lexical = std::mem::replace(&mut self.lexical, false);
        walk_mut::walk_function_body(self, body);
        self.lexical = lexical;
    }
}

/// Drops the type-only specifiers of an export list (`export { type T }`), declaring each as a
/// module type; the export keeps its other specifiers and its evaluation edge.
fn strip_type_only_exports(declared: &mut DeclaredTypes, specifiers: &mut ArenaVec<'_, ExportSpecifier<'_>>) {
    specifiers.retain(|specifier| {
        if !specifier.export_kind.is_type() { return true; }
        declared.declare(DeclaredKind::ModuleType, Some(&specifier.exported.name()), specifier.span);
        false
    });
}
