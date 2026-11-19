//! Declared function signatures, local bindings and parameter shapes: what a TypeScript annotation
//! says a function returns, takes as its rest parameter or as `this`, what each annotated local
//! holds, and what each parameter's type literal, local interface or literal alias, or top-level
//! class type declares
//! (`DeclaredTypes::signatures`, `DeclaredTypes::locals`, `DeclaredTypes::members`,
//! `DeclaredTypes::parameter_classes`, `DeclaredTypes::parameters`). A declaration is a seed, not
//! a proof (TypeScript is unsound), so a tier checks it once where a value enters and trusts it.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashMap;

use crate::bytecode::{DeclaredSignature, DeclaredTypes};
use crate::compile::class::class_method_source_span;
use crate::typed_ir::language_bit;
use super::{DECLARED_ANY, Named, TypeNames, declared_language, declared_language_in, element_languages};

/// The function a local is declared in when it is top-level code of a script or module.
const TOP_LEVEL: u32 = u32::MAX;

/// Record each declared signature and annotated local of `program`, keyed by the source span
/// start each function's bytecode header begins at. Run before admission, so the annotations
/// are still in place.
pub(super) fn collect<'a>(program: &Program<'a>, names: &TypeNames<'_>, declarations: &mut DeclaredTypes) {
    let shapes = Shapes::of(program);
    let mut collect = Collect {
        text: program.source_text,
        declarations,
        function: Some(TOP_LEVEL),
        class: TOP_LEVEL,
        method: None,
        loop_head: false,
        names,
        shapes: &shapes,
    };
    collect.visit_program(program);
}

/// The nesting bound of `shape_members`' walk through `extends` clauses: a cycle or a chain this
/// deep records nothing.
const MAX_EXTENDS_DEPTH: u8 = 8;

/// A top-level declaration whose members a parameter's type reference can denote.
#[derive(Clone, Copy)]
enum Shape<'s> {
    /// An interface, one declaration of a name that may be declared by several interfaces.
    Interface(&'s TSInterfaceDeclaration<'s>),
    /// A non-generic type alias whose type is a type literal.
    Literal(&'s TSTypeLiteral<'s>),
}

/// The top-level interfaces and literal aliases of a program, by name. A name is only looked up
/// here after `TypeNames` has resolved it to a single declaration or to merged interfaces, so every
/// declaration of a name is found.
struct Shapes<'s> {
    declarations: FxHashMap<&'s str, Vec<Shape<'s>>>,
}

impl<'s> Shapes<'s> {
    /// The shapes of `program`'s top-level declarations, exported ones included.
    fn of(program: &'s Program<'s>) -> Self {
        let mut shapes = Shapes { declarations: FxHashMap::default() };
        for statement in &program.body {
            match statement {
                Statement::TSInterfaceDeclaration(declaration) => shapes.interface(declaration),
                Statement::TSTypeAliasDeclaration(declaration) => shapes.alias(declaration),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::TSInterfaceDeclaration(declaration) => shapes.interface(declaration),
                    Declaration::TSTypeAliasDeclaration(declaration) => shapes.alias(declaration),
                    _ => {}
                },
                _ => {}
            }
        }
        shapes
    }

    fn interface(&mut self, declaration: &'s TSInterfaceDeclaration<'s>) {
        self.declarations.entry(declaration.id.name.as_str()).or_default().push(Shape::Interface(declaration));
    }

    /// Record an alias only when it is non-generic and its type is a type literal; any other alias
    /// has no member facts, so it records nothing.
    fn alias(&mut self, declaration: &'s TSTypeAliasDeclaration<'s>) {
        let TSType::TSTypeLiteral(literal) = &declaration.type_annotation else { return };
        if declaration.type_parameters.is_none() {
            self.declarations.entry(declaration.id.name.as_str()).or_default().push(Shape::Literal(literal));
        }
    }
}

/// The members a parameter typed by the local name `name` has, in the order TypeScript merges
/// them: an `extends` base's members first, then the declaration's own, so a derived member that
/// repeats a base member comes later and wins `DeclaredTypes::declared_member`. Every declaration of
/// `name` contributes. `None` records nothing: no declaration of `name` is in `shapes`, a
/// declaration is generic, an `extends` clause is generic or names anything but a local interface
/// (`names` resolves it), or the walk exceeds `MAX_EXTENDS_DEPTH`.
fn shape_members<'s>(shapes: &Shapes<'s>, names: &TypeNames<'_>, name: &str, depth: u8) -> Option<Vec<&'s TSSignature<'s>>> {
    if depth > MAX_EXTENDS_DEPTH {
        return None;
    }
    let mut members = Vec::new();
    for shape in shapes.declarations.get(name)? {
        match *shape {
            Shape::Literal(literal) => members.extend(literal.members.iter()),
            Shape::Interface(declaration) => {
                if declaration.type_parameters.is_some() {
                    return None;
                }
                for heritage in &declaration.extends {
                    if heritage.type_arguments.is_some() {
                        return None;
                    }
                    let TSTypeName::IdentifierReference(base) = &heritage.type_name else { return None };
                    if !matches!(names.get(base.name.as_str()), Some(Named::Interface)) {
                        return None;
                    }
                    members.extend(shape_members(shapes, names, base.name.as_str(), depth + 1)?);
                }
                members.extend(declaration.body.body.iter());
            }
        }
    }
    Some(members)
}

struct Collect<'a, 'd> {
    text: &'a str,
    declarations: &'d mut DeclaredTypes,
    /// The innermost enclosing function's key; `None` where that function's bytecode has no
    /// source span, so its locals have no key to attach to.
    function: Option<u32>,
    /// The enclosing class's span start: a constructor's header begins there.
    class: u32,
    /// The method whose value is visited next: its value's span and its key (`None` where the
    /// method has no source span).
    method: Option<(Span, Option<u32>)>,
    /// Whether the visit is inside a `for`-`in`/`of` head, whose bindings take no annotation.
    loop_head: bool,
    /// The program's type-level names, which a parameter annotation may name a class by.
    names: &'d TypeNames<'d>,
    /// The program's top-level interfaces and literal aliases, whose members a parameter may name.
    shapes: &'d Shapes<'d>,
}

impl<'a> Visit<'a> for Collect<'a, '_> {
    fn visit_class(&mut self, class: &Class<'a>) {
        let outer = std::mem::replace(&mut self.class, class.span.start);
        walk::walk_class(self, class);
        self.class = outer;
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
        let key = if method.kind.is_constructor() {
            Some(self.class)
        } else {
            class_method_source_span(self.text, method.span, method.key.span().start).map(|span| span.start)
        };
        let outer = self.method.replace((method.value.span, key));
        walk::walk_method_definition(self, method);
        self.method = outer;
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let key = match self.method {
            Some((span, key)) if span == function.span => key,
            _ => Some(function.span.start),
        };
        if let Some(start) = key {
            let returns = returns_of(function.r#async || function.generator, function.return_type.as_deref(), self.names);
            let this = this_of(function.this_param.as_deref(), self.names);
            self.record(start, returns, &function.params, this);
        }
        let outer = std::mem::replace(&mut self.function, key);
        walk::walk_function(self, function, flags);
        self.function = outer;
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let start = arrow.span.start;
        let returns = returns_of(arrow.r#async, arrow.return_type.as_deref(), self.names);
        self.record(start, returns, &arrow.params, DECLARED_ANY);
        let outer = std::mem::replace(&mut self.function, Some(start));
        walk::walk_arrow_function_expression(self, arrow);
        self.function = outer;
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if !self.loop_head {
            self.local(declarator.id.get_identifier_name().map(|name| name.as_str()), declarator.type_annotation.as_deref());
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_catch_parameter(&mut self, parameter: &CatchParameter<'a>) {
        self.local(parameter.pattern.get_identifier_name().map(|name| name.as_str()), parameter.type_annotation.as_deref());
        walk::walk_catch_parameter(self, parameter);
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.visit_loop_head(&statement.left);
        self.visit_expression(&statement.right);
        self.visit_statement(&statement.body);
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.visit_loop_head(&statement.left);
        self.visit_expression(&statement.right);
        self.visit_statement(&statement.body);
    }
}

impl<'a> Collect<'a, '_> {
    /// Visit a `for`-`in`/`of` head's binding without recording its annotation.
    fn visit_loop_head(&mut self, left: &ForStatementLeft<'a>) {
        self.loop_head = true;
        self.visit_for_statement_left(left);
        self.loop_head = false;
    }

    /// Record a signature unless every part is `DECLARED_ANY`, and each parameter's declared shape.
    fn record(&mut self, start: u32, returns: u16, params: &FormalParameters<'_>, this: u16) {
        let rest = rest_of(params, self.names);
        if returns != DECLARED_ANY || rest != DECLARED_ANY || this != DECLARED_ANY {
            self.declarations.declare_signature(DeclaredSignature { start, returns, rest, this });
        }
        self.parameters(start, params);
    }

    /// Record the declared shape of each parameter with a plain identifier pattern and a type
    /// annotation: the properties of a type literal, the properties of a local interface or
    /// non-generic literal alias the type reference names, or the top-level class it names.
    fn parameters(&mut self, function: u32, params: &FormalParameters<'_>) {
        for (index, parameter) in params.items.iter().enumerate() {
            let (Ok(index), Some(annotation)) = (u16::try_from(index), &parameter.type_annotation) else { continue };
            if parameter.pattern.get_identifier_name().is_none() {
                continue;
            }
            self.parameter_languages(function, index, parameter, annotation);
            match &annotation.type_annotation {
                TSType::TSTypeLiteral(literal) => {
                    for member in &literal.members {
                        self.member(function, index, member);
                    }
                }
                TSType::TSTypeReference(reference) => {
                    let TSTypeName::IdentifierReference(name) = &reference.type_name else { continue };
                    match self.names.get(name.name.as_str()) {
                        Some(Named::Class(class)) => self.declarations.declare_parameter_class(function, index, class),
                        Some(Named::Interface | Named::Alias { open: false, .. }) if reference.type_arguments.is_none() => {
                            if let Some(members) = shape_members(self.shapes, self.names, name.name.as_str(), 0) {
                                for member in members {
                                    self.member(function, index, member);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Record a parameter's language set when the program's type names resolve it to a different set
    /// than the name-free reading (`declared_language`). An optional or defaulted parameter admits
    /// `undefined`, as its declared set does (`compile::typescript::declared_parameters`).
    fn parameter_languages(&mut self, function: u32, index: u16, parameter: &FormalParameter<'_>, annotation: &TSTypeAnnotation<'_>) {
        let named = declared_language_in(&annotation.type_annotation, self.names);
        if named == declared_language(&annotation.type_annotation) {
            return;
        }
        let languages = if named != DECLARED_ANY && (parameter.optional || parameter.initializer.is_some()) {
            named | language_bit(crate::ValueType::Undefined)
        } else {
            named
        };
        self.declarations.declare_parameter(function, index, languages);
    }

    /// A property of a parameter's type literal with a static identifier key and a type annotation.
    /// An optional property admits `undefined`, as an optional class field does.
    fn member(&mut self, function: u32, parameter: u16, member: &TSSignature<'_>) {
        let TSSignature::TSPropertySignature(property) = member else { return };
        let PropertyKey::StaticIdentifier(identifier) = &property.key else { return };
        let Some(annotation) = &property.type_annotation else { return };
        let mut languages = declared_language_in(&annotation.type_annotation, self.names);
        if property.optional {
            languages |= language_bit(crate::ValueType::Undefined);
        }
        if languages != DECLARED_ANY {
            self.declarations.declare_member(function, parameter, identifier.name.as_str(), languages);
        }
    }

    /// Record a local binding of a plain identifier with a type annotation, in the innermost function.
    /// An array annotation is recorded for its element types even where its own languages are open.
    fn local(&mut self, name: Option<&str>, annotation: Option<&TSTypeAnnotation<'_>>) {
        let (Some(function), Some(name), Some(annotation)) = (self.function, name, annotation) else { return };
        let languages = declared_language_in(&annotation.type_annotation, self.names);
        let elements = element_languages(&annotation.type_annotation, self.names);
        if languages != DECLARED_ANY || elements != DECLARED_ANY {
            self.declarations.declare_local(function, name, languages, elements);
        }
    }
}

/// The return type's languages. An `async` function returns a Promise and a generator returns
/// an iterator: both are objects whatever the annotation names.
fn returns_of(object: bool, annotation: Option<&TSTypeAnnotation<'_>>, names: &TypeNames<'_>) -> u16 {
    if object {
        language_bit(crate::ValueType::Object)
    } else {
        annotation.map_or(DECLARED_ANY, |annotation| declared_language_in(&annotation.type_annotation, names))
    }
}

/// The `this` parameter's languages. A named type stays open (`DECLARED_ANY`): it may be an
/// alias of a primitive or a type parameter, which no syntax tells apart from a class.
fn this_of(parameter: Option<&TSThisParameter<'_>>, names: &TypeNames<'_>) -> u16 {
    parameter.and_then(|parameter| parameter.type_annotation.as_deref())
        .map_or(DECLARED_ANY, |annotation| declared_language_in(&annotation.type_annotation, names))
}

/// The languages of an annotated rest parameter's elements: `...xs: T[]` and `...xs: Array<T>`
/// hold `T`s.
fn rest_of(params: &FormalParameters<'_>, names: &TypeNames<'_>) -> u16 {
    let Some(annotation) = params.rest.as_ref().and_then(|rest| rest.type_annotation.as_deref()) else {
        return DECLARED_ANY;
    };
    element_languages(&annotation.type_annotation, names)
}

#[cfg(test)]
mod tests {
    use crate::bytecode::{DeclaredSignature, DeclaredTypes};
    use crate::compile::typescript::DECLARED_ANY;
    use crate::{Engine, ValueType as V};

    fn bit(kind: V) -> u16 {
        crate::typed_ir::Languages::of(kind).bits()
    }

    fn declared_of(source: &str, name: &str) -> DeclaredTypes {
        let program = Engine::specialize_typescript(source, name).unwrap();
        program.declared_types.as_deref().expect("a typed program keeps its declarations").clone()
    }

    #[test]
    fn function_signature_and_locals_are_declared() {
        let source = "function f(this: Window, a: number, ...r: string[]): boolean { let x: number = 1; const s: string = \"\"; return true }";
        let declared = declared_of(source, "signature.cts");
        let f = source.find("function f").unwrap() as u32;
        assert_eq!(declared.declared_signature(f), Some(&DeclaredSignature {
            start: f, returns: bit(V::Boolean), rest: bit(V::String), this: DECLARED_ANY,
        }));
        assert_eq!(declared.declared_local(f, "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_local(f, "s"), Some(bit(V::String)));
        assert_eq!(declared.declared_local(f, "a"), None);
        assert_eq!(declared.declared_local(u32::MAX, "x"), None);
    }

    #[test]
    fn array_annotations_record_element_types() {
        let source = "let xs: number[] = []; function f() { const ys: readonly string[] = []; let zs: Array<boolean> = []; }";
        let declared = declared_of(source, "arrays.cts");
        let f = source.find("function f").unwrap() as u32;
        assert_eq!(declared.declared_local_elements(u32::MAX, "xs"), Some(bit(V::Number)));
        assert_eq!(declared.declared_local(u32::MAX, "xs"), Some(bit(V::Object)));
        assert_eq!(declared.declared_local_elements(f, "ys"), Some(bit(V::String)));
        assert_eq!(declared.declared_local_elements(f, "zs"), Some(bit(V::Boolean)));
        assert_eq!(declared.declared_local(f, "zs"), None);
        assert_eq!(declared.declared_local_elements(f, "missing"), None);
    }

    #[test]
    fn arrow_return_type_is_declared() {
        let source = "const g = (n: number): number => n;";
        let declared = declared_of(source, "arrow.cts");
        let g = source.find("(n").unwrap() as u32;
        assert_eq!(declared.declared_signature(g).map(|signature| signature.returns), Some(bit(V::Number)));
    }

    #[test]
    fn top_level_locals_and_unannotated_functions() {
        let source = "let top: string = \"\"; function plain(a) { return a }";
        let declared = declared_of(source, "top.cts");
        assert_eq!(declared.declared_local(u32::MAX, "top"), Some(bit(V::String)));
        assert_eq!(declared.declared_signature(source.find("function plain").unwrap() as u32), None);
    }

    #[test]
    fn method_and_constructor_signatures_are_keyed_by_their_headers() {
        let source = "class C { m(...xs: number[]): void {} static s(): number { return 1 } }\nclass K { constructor(...r: boolean[]) {} }";
        let declared = declared_of(source, "members.cts");
        let m = source.find("m(...xs").unwrap() as u32;
        let s = source.find("s(): number").unwrap() as u32;
        let k = source.find("class K").unwrap() as u32;
        assert_eq!(declared.declared_signature(m).map(|signature| (signature.returns, signature.rest)),
            Some((bit(V::Undefined), bit(V::Number))));
        assert_eq!(declared.declared_signature(s).map(|signature| signature.returns), Some(bit(V::Number)));
        assert_eq!(declared.declared_signature(k).map(|signature| signature.rest), Some(bit(V::Boolean)));
    }

    #[test]
    fn parameter_type_literals_and_classes_are_declared() {
        let source = "class P { x: number }\nfunction f(p: {x: number, s: string, o?: boolean}) {}\nfunction g(p: P) {}\nfunction h<P>(p: P) {}\nclass Q {}\ninterface Q {}\nfunction k(q: Q) {}";
        let declared = declared_of(source, "parameters.cts");
        let f = source.find("function f").unwrap() as u32;
        let g = source.find("function g").unwrap() as u32;
        let h = source.find("function h").unwrap() as u32;
        let k = source.find("function k").unwrap() as u32;
        let p = source.find("class P").unwrap() as u32;
        assert_eq!(declared.declared_member(f, 0, "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(f, 0, "s"), Some(bit(V::String)));
        assert_eq!(declared.declared_member(f, 0, "o"), Some(bit(V::Boolean) | bit(V::Undefined)));
        assert_eq!(declared.declared_member(f, 0, "missing"), None);
        assert_eq!(declared.declared_member(f, 1, "x"), None);
        assert_eq!(declared.declared_parameter_class(g, 0), Some(p));
        assert_eq!(declared.declared_parameter_class(h, 0), None);
        assert_eq!(declared.declared_parameter_class(k, 0), None);
        assert_eq!(declared.declared_parameter_class(f, 0), None);
    }

    #[test]
    fn parameter_interfaces_and_aliases_have_the_members_of_their_type_literal() {
        let source = "interface I { x: number; s: string; o?: boolean }\ntype A = { x: number; s: string; o?: boolean };\n\
            interface E extends I { y: number }\n\
            function lit(p: {x: number, s: string, o?: boolean}) {}\n\
            function fi(p: I) {}\nfunction fa(p: A) {}\nfunction fe(p: E) {}";
        let declared = declared_of(source, "interfaces.cts");
        let member_facts = |start: u32| ["x", "s", "o"].map(|name| declared.declared_member(start, 0, name));
        let lit = source.find("function lit").unwrap() as u32;
        let expected = [Some(bit(V::Number)), Some(bit(V::String)), Some(bit(V::Boolean) | bit(V::Undefined))];
        assert_eq!(member_facts(lit), expected);
        assert_eq!(member_facts(source.find("function fi").unwrap() as u32), expected);
        assert_eq!(member_facts(source.find("function fa").unwrap() as u32), expected);
        assert_eq!(member_facts(source.find("function fe").unwrap() as u32), expected);
        assert_eq!(declared.declared_member(source.find("function fe").unwrap() as u32, 0, "y"), Some(bit(V::Number)));
    }

    #[test]
    fn parameter_names_without_local_member_facts_record_nothing() {
        let source = "interface G<T> { x: T }\ntype P<T> = { x: T };\ninterface Open extends Unknown { x: number }\n\
            interface Twice { x: number }\ninterface Twice { s: string }\n\
            function fg(p: G<number>) {}\nfunction fp(p: P<number>) {}\nfunction fo(p: Open) {}\n\
            function fm(p: Missing) {}\nfunction ft(p: Twice) {}\nfunction fu(p: Missing<number>) {}";
        let declared = declared_of(source, "nothing.cts");
        for function in ["fg", "fp", "fo", "fm", "fu"] {
            let start = source.find(&format!("function {function}")).unwrap() as u32;
            assert_eq!(declared.declared_member(start, 0, "x"), None, "{function}");
        }
        let twice = source.find("function ft").unwrap() as u32;
        assert_eq!(declared.declared_member(twice, 0, "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(twice, 0, "s"), Some(bit(V::String)));
    }

    #[test]
    fn signatures_and_locals_survive_the_binary_round_trip() {
        let source = "function f(this: Window, ...r: string[]): boolean { let x: number = 1; return true }\nlet top: string = \"\";\nclass P { x: number }\nfunction m(p: {x: number}, q: P) {}";
        let program = Engine::specialize_typescript(source, "round.cts").unwrap();
        let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
        let decoded = crate::ResidualProgram::decode_trusted(bytes).unwrap();
        assert_eq!(decoded.declared_types, program.declared_types);
        let declared = decoded.declared_types.as_deref().expect("a typed program keeps its declarations");
        assert_eq!(declared.signatures.len(), 1);
        assert_eq!(declared.locals.len(), 2);
        assert_eq!(declared.members.len(), 1);
        assert_eq!(declared.parameter_classes.len(), 1);
    }

    #[test]
    fn named_parameter_types_reach_the_compiled_functions() {
        let source = "type N = number;\ninterface Point { x: number }\nfunction f(a: N) {}\nfunction g(p: Point) {}";
        let program = Engine::specialize_typescript(source, "named.cts").unwrap();
        let declared = declared_of(source, "named.cts");
        let f = source.find("function f").unwrap() as u32;
        let g = source.find("function g").unwrap() as u32;
        assert_eq!(declared.declared_parameter(f, 0), Some(bit(V::Number)));
        let compiled = |start: u32| (0..program.function_count()).filter_map(|index| program.function_get(index))
            .find(|function| function.source_span.map(|span| span.start) == Some(start))
            .map(|function| function.declared_parameters().to_vec())
            .expect("the function is compiled");
        assert_eq!(compiled(f), vec![bit(V::Number)]);
        assert_eq!(compiled(g), vec![bit(V::Object) | bit(V::Function)]);
    }
}
