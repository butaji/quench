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

use super::enums::{self, Member, Value};
use oxc_allocator::{Allocator, Box as ArenaBox, TakeIn, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};

/// What a declaration of the declared environment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeclaredKind {
    Interface,
    TypeAlias,
    /// An overload signature or a `declare function`.
    FunctionSignature,
    Ambient,
    /// A namespace with no run-time content.
    Namespace,
    /// A type-only import or export.
    ModuleType,
    /// An abstract or signature-only class member, a `declare` field or an index signature.
    ClassMember,
    /// A run-time enum, recorded with its folded members.
    Enum,
}

/// A declared name of the typed program, by source range.
#[derive(Clone, Debug)]
#[allow(dead_code, reason = "read once the declared environment reaches bytecode (next commit)")]
pub(crate) struct Declared {
    pub(crate) kind: DeclaredKind,
    pub(crate) name: Option<String>,
    pub(crate) span: Span,
}

/// The declared environment of a typed tree: what it declares beyond its run-time code.
#[derive(Default, Debug)]
pub(crate) struct Declarations {
    pub(crate) declared: Vec<Declared>,
    /// Each enum's members that fold to constants, by enum name (`E.A` reads are constants).
    pub(crate) enum_constants: FxHashMap<String, Vec<(String, EnumConstant)>>,
}

impl Declarations {
    pub(super) fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: Span) {
        self.declared.push(Declared { kind, name: name.map(str::to_owned), span });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EnumConstant {
    Number(f64),
    String(String),
}

/// Admit a typed tree: the declared environment out, run-time meaning in.
pub(in crate::compile) fn admit<'a>(allocator: &'a Allocator, program: &mut Program<'a>) -> Declarations {
    let mut admit = Admit { allocator, declarations: Declarations::default(), type_names: FxHashSet::default(), lexical: false };
    admit.visit_program(program);
    admit.drop_type_exports(&mut program.body);
    admit.declarations
}

struct Admit<'a> {
    allocator: &'a Allocator,
    declarations: Declarations,
    /// Top-level names that denote only types, so `export { T }` exports nothing at run time.
    type_names: FxHashSet<String>,
    /// The current statement list is a block's (or a namespace body's): its generated
    /// bindings are `let`, so they stay in it as tsc's function-scoped ones do.
    lexical: bool,
}

/// How a declaration is exported: not, from the module, or from namespace `N`.
#[derive(Clone, Copy)]
enum Export<'n> {
    None,
    Module,
    Namespace(&'n str),
}

/// A statement the admission produced, and whether its children were admitted already.
type Produced<'a> = (Statement<'a>, bool);

impl<'a> Admit<'a> {
    fn keyword(&self) -> &'static str {
        if self.lexical { "let" } else { "var" }
    }

    fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: Span) {
        self.declarations.declared.push(Declared { kind, name: name.map(str::to_owned), span });
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

    fn javascript_expression(&self, text: &str, span: Span) -> Expression<'a> {
        let mut statements = self.javascript(&format!("({text});"), span);
        match statements.first_mut() {
            Some(Statement::ExpressionStatement(statement)) => match statement.expression.take_in(&self.allocator) {
                Expression::ParenthesizedExpression(mut inner) => inner.expression.take_in(&self.allocator),
                expression => expression,
            },
            _ => unreachable!("an expression statement"),
        }
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
                let mut statements = self.javascript("module.exports = $$quench_ts_0;", assignment.span);
                fill(&mut statements, vec![assignment.expression.take_in(&self.allocator)]);
                statements.into_iter().map(|statement| (statement, false)).collect()
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
                let bindings = bound_names(&declaration);
                seen.extend(bindings.iter().cloned());
                match (how, export) {
                    (Export::Namespace(namespace), _) => {
                        let mut produced = vec![(Statement::from(declaration), false)];
                        let published: String = bindings.iter().map(|name| format!("{namespace}.{name} = {name}; ")).collect();
                        produced.extend(self.javascript(&published, span).into_iter().map(|statement| (statement, true)));
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

    fn import(&mut self, mut import: ArenaBox<'a, ImportDeclaration<'a>>) -> Vec<Produced<'a>> {
        if import.import_kind.is_type() {
            self.declare(DeclaredKind::ModuleType, None, import.span);
            return Vec::new();
        }
        // An import whose names were all types keeps its evaluation edge.
        if let Some(specifiers) = &mut import.specifiers {
            let declared = &mut self.declarations.declared;
            specifiers.retain(|specifier| match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) if named.import_kind.is_type() => {
                    declared.push(Declared { kind: DeclaredKind::ModuleType, name: Some(named.local.name.to_string()), span: named.span });
                    false
                }
                _ => true,
            });
        }
        vec![(Statement::ImportDeclaration(import), true)]
    }

    fn export_list(&mut self, mut export: ArenaBox<'a, ExportNamedDeclaration<'a>>) -> Vec<Produced<'a>> {
        if export.export_kind.is_type() {
            self.declare(DeclaredKind::ModuleType, None, export.span);
            return Vec::new();
        }
        let declared = &mut self.declarations.declared;
        export.specifiers.retain(|specifier| {
            if !specifier.export_kind.is_type() { return true; }
            declared.push(Declared { kind: DeclaredKind::ModuleType, name: Some(specifier.exported.name().to_string()), span: specifier.span });
            false
        });
        vec![(Statement::ExportNamedDeclaration(export), true)]
    }

    fn export_from(&mut self, mut export: ArenaBox<'a, ExportFromDeclaration<'a>>) -> Vec<Produced<'a>> {
        if export.export_kind.is_type() {
            self.declare(DeclaredKind::ModuleType, None, export.span);
            return Vec::new();
        }
        let declared = &mut self.declarations.declared;
        export.specifiers.retain(|specifier| {
            if !specifier.export_kind.is_type() { return true; }
            declared.push(Declared { kind: DeclaredKind::ModuleType, name: Some(specifier.exported.name().to_string()), span: specifier.span });
            false
        });
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
        vec![(Statement::ExportDefaultDeclaration(export), false)]
    }

    /// `export { T }` of a name that denotes only a type exports nothing at run time.
    fn drop_type_exports(&mut self, body: &mut ArenaVec<'a, Statement<'a>>) {
        if self.type_names.is_empty() { return; }
        for statement in body.iter_mut() {
            let Statement::ExportNamedDeclaration(export) = statement else { continue };
            let names = &self.type_names;
            let declared = &mut self.declarations.declared;
            export.specifiers.retain(|specifier| {
                let local = specifier.local.name();
                if !names.contains(local.as_ref()) { return true; }
                declared.push(Declared { kind: DeclaredKind::ModuleType, name: Some(local.to_string()), span: specifier.span });
                false
            });
        }
    }

    /// `import x = require("m")` / `import x = A.B`: a binding to that value.
    fn import_equals(&mut self, alias: ArenaBox<'a, TSImportEqualsDeclaration<'a>>, span: Span, how: Export<'_>) -> Vec<Produced<'a>> {
        let name = alias.id.name.as_str();
        let target = match &alias.module_reference {
            TSModuleReference::ExternalModuleReference(external) => format!("require({})", enums::quote(external.expression.value.as_str())),
            TSModuleReference::IdentifierReference(identifier) => identifier.name.to_string(),
            TSModuleReference::QualifiedName(qualified) => qualified_text(qualified),
        };
        let keyword = self.keyword();
        let text = match how {
            Export::None => format!("{keyword} {name} = {target};"),
            Export::Module => format!("export var {name} = {target};"),
            Export::Namespace(namespace) => format!("{keyword} {name} = {namespace}.{name} = {target};"),
        };
        self.javascript(&text, span).into_iter().map(|statement| (statement, true)).collect()
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

impl<'a> Admit<'a> {
    /// An enum: one object literal with fixed keys when every member folds (a stable
    /// shape, member reads are field reads), else tsc's member-by-member emit.
    fn enumeration(&mut self, mut enumeration: ArenaBox<'a, TSEnumDeclaration<'a>>, how: Export<'_>, seen: &mut FxHashSet<String>) -> Vec<Produced<'a>> {
        let name = enumeration.id.name.to_string();
        let first = seen.insert(name.clone());
        let (members, values) = self.enum_members(&mut enumeration, &name);
        let constants: Vec<(String, EnumConstant)> = members.iter().filter_map(|(member, value)| match value {
            Member::Constant(Value::Number(number)) => Some((member.clone(), EnumConstant::Number(*number))),
            Member::Constant(Value::String(text)) => Some((member.clone(), EnumConstant::String(text.clone()))),
            Member::Computed(_) => None,
        }).collect();
        self.declarations.enum_constants.entry(name.clone()).or_default().extend(constants);
        self.declare(DeclaredKind::Enum, Some(&name), enumeration.span);
        let keyword = self.keyword();
        let text = object_enum(&name, &members, first, how, keyword).unwrap_or_else(|| incremental_enum(&name, &members, first, how, keyword));
        let mut statements = self.javascript(&text, enumeration.span);
        fill(&mut statements, values);
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
                        Qualify { admit: self, enum_name, names: &names }.visit_expression(&mut expression);
                        values.push(expression);
                        Member::Computed(format!("$$quench_ts_{}", values.len() - 1))
                    }
                },
                (None, None) => Member::Constant(Value::Number(0.0)),
                (None, Some((_, Some(number)))) => Member::Constant(Value::Number(number + 1.0)),
                (None, Some((previous_name, None))) => Member::Computed(format!("{enum_name}[{}] + 1", enums::quote(previous_name))),
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
    admit: &'s Admit<'a>,
    enum_name: &'s str,
    names: &'s [String],
}

impl<'a> VisitMut<'a> for Qualify<'_, 'a> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::Identifier(identifier) = expression
            && self.names.iter().any(|name| name == identifier.name.as_str())
        {
            let text = format!("{}[{}]", self.enum_name, enums::quote(identifier.name.as_str()));
            *expression = self.admit.javascript_expression(&text, identifier.span);
            return;
        }
        walk_mut::walk_expression(self, expression);
    }
}

/// `var E = {"A": 0, "0": "A", …};`: when every member folds to an exact literal.
fn object_enum(name: &str, members: &[(String, Member)], first: bool, how: Export<'_>, keyword: &str) -> Option<String> {
    if !first { return None; }
    let mut fields = Vec::with_capacity(members.len() * 2);
    for (member, value) in members {
        let Member::Constant(value) = value else { return None };
        let key = enums::quote(member);
        fields.push(format!("{key}: {}", enums::literal(value)?));
        if let Value::Number(_) = value {
            fields.push(format!("{}: {key}", enums::quote(&enums::display(value)?)));
        }
    }
    let object = format!("{{{}}}", fields.join(", "));
    Some(match how {
        Export::None => format!("{keyword} {name} = {object};"),
        Export::Module => format!("export var {name} = {object};"),
        Export::Namespace(namespace) => format!("{keyword} {name} = {namespace}.{name} = {object};"),
    })
}

/// tsc's emit, without its function: `var E; E || (E = {}); E[E["A"] = 0] = "A"; …`.
fn incremental_enum(name: &str, members: &[(String, Member)], first: bool, how: Export<'_>, keyword: &str) -> String {
    let mut text = format!("{}{};", declaration_prefix(name, first, how, keyword), namespace_object(name, how));
    for (member, value) in members {
        let key = enums::quote(member);
        match value {
            Member::Constant(Value::String(string)) => text.push_str(&format!(" {name}[{key}] = {};", enums::quote(string))),
            Member::Constant(Value::Number(number)) => text.push_str(&format!(" {name}[{name}[{key}] = {}] = {key};", number_text(*number))),
            Member::Computed(expression) => text.push_str(&format!(" {name}[{name}[{key}] = ({expression})] = {key};")),
        }
    }
    text
}

fn number_text(number: f64) -> String {
    if number.is_nan() { "NaN".into() }
    else if number.is_infinite() { if number > 0.0 { "Infinity".into() } else { "-Infinity".into() } }
    else if number == 0.0 && number.is_sign_negative() { "-0".into() }
    else { format!("{number}") }
}

/// `var N; ` before the first declaration of a merged name (`export var N; ` from a module).
fn declaration_prefix(name: &str, first: bool, how: Export<'_>, keyword: &str) -> String {
    match (first, how) {
        (false, _) => String::new(),
        (true, Export::Module) => format!("export var {name}; "),
        (true, _) => format!("{keyword} {name}; "),
    }
}

/// The object a merged declaration fills: the existing one or a new one, published on
/// the parent namespace.
fn namespace_object(name: &str, how: Export<'_>) -> String {
    match how {
        Export::Namespace(parent) => format!("{name} = {parent}.{name} || ({parent}.{name} = {{}})"),
        _ => format!("{name} || ({name} = {{}})"),
    }
}

impl<'a> Admit<'a> {
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
        let head = format!("{}{};", declaration_prefix(&names[0], first, how, self.keyword()), namespace_object(&names[0], how));
        let mut produced: Vec<Produced<'a>> = self.javascript(&head, span).into_iter().map(|statement| (statement, true)).collect();
        let innermost = names.last().cloned().unwrap_or_default();
        let lexical = std::mem::replace(&mut self.lexical, true);
        let statements = self.statements(block.unbox().body, Some(&innermost));
        self.lexical = lexical;
        let mut inner = self.block(statements, span);
        for pair in names.windows(2).rev() {
            let object = format!("let {1} = {0}.{1} || ({0}.{1} = {{}});", pair[0], pair[1]);
            let mut statements = self.javascript(&object, span);
            statements.push(inner);
            inner = self.block(statements, span);
        }
        produced.push((inner, true));
        produced
    }

    fn block(&self, statements: ArenaVec<'a, Statement<'a>>, span: Span) -> Statement<'a> {
        let mut generated = self.javascript("{}", span);
        let Some(Statement::BlockStatement(mut block)) = generated.pop() else { unreachable!("a block") };
        block.body = statements;
        Statement::BlockStatement(block)
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

impl<'a> Admit<'a> {
    /// Class members: signature-only ones move to the declared environment; parameter
    /// properties become `this.x = x;` at the constructor's start (after its root
    /// `super(…)` call when derived), their annotations kept on their parameters.
    fn class_members(&mut self, class: &mut Class<'a>) {
        let declared = &mut self.declarations.declared;
        class.body.body.retain(|element| {
            if !signature_member(element) { return true; }
            let name = element.property_key().and_then(PropertyKey::static_name).map(|name| name.to_string());
            declared.push(Declared { kind: DeclaredKind::ClassMember, name, span: element.span() });
            false
        });
        let derived = class.heritage.is_some();
        for element in class.body.body.iter_mut() {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if !method.kind.is_constructor() { continue; }
            let names = parameter_properties(&mut method.value.params);
            let span = method.value.params.span;
            let Some(body) = method.value.body.as_mut().filter(|_| !names.is_empty()) else { continue };
            let text: String = names.iter().map(|name| format!("this.{name} = {name}; ")).collect();
            let at = if derived { root_super_call(body).map_or(0, |index| index + 1) } else { 0 };
            for (offset, statement) in self.javascript(&text, span).into_iter().enumerate() {
                body.statements.insert(at + offset, statement);
            }
        }
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

impl<'a> VisitMut<'a> for Admit<'a> {
    fn visit_statements(&mut self, statements: &mut ArenaVec<'a, Statement<'a>>) {
        let input = std::mem::replace(statements, ArenaVec::new_in(&self.allocator));
        *statements = self.statements(input, None);
    }

    fn visit_class(&mut self, class: &mut Class<'a>) {
        self.class_members(class);
        walk_mut::walk_class(self, class);
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
