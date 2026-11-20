//! Declared function signatures, local bindings and parameter shapes: what a TypeScript annotation
//! says a function returns, takes as its rest parameter or as `this`, what each annotated local
//! holds, and what each parameter's or local's object type (a literal, or an interface or alias the
//! program declares) and each parameter's top-level class type declares
//! (`DeclaredTypes::signatures`, `DeclaredTypes::locals`, `DeclaredTypes::members`,
//! `DeclaredTypes::local_members`,
//! `DeclaredTypes::parameter_classes`, `DeclaredTypes::parameters`). A declaration is a seed, not
//! a proof (TypeScript is unsound), so a tier checks it once where a value enters and trusts it.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

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
    let mut collect = Collect {
        text: program.source_text,
        declarations,
        function: Some(TOP_LEVEL),
        class: TOP_LEVEL,
        method: None,
        loop_head: false,
        names,
    };
    collect.visit_program(program);
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
            let name = declarator.id.get_identifier_name().map(|name| name.as_str());
            // An unannotated `const x = e as T` (or `<T>e`) is typed as if written `const x: T = e`.
            // `satisfies` keeps the expression's own type, and `x!` names no type: neither is recorded.
            let ty = match (&declarator.type_annotation, &declarator.init) {
                (Some(annotation), _) => Some(&annotation.type_annotation),
                (None, Some(init)) => asserted_type(init),
                (None, None) => None,
            };
            self.local(name, ty);
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_catch_parameter(&mut self, parameter: &CatchParameter<'a>) {
        self.local(parameter.pattern.get_identifier_name().map(|name| name.as_str()), parameter.type_annotation.as_deref().map(|annotation| &annotation.type_annotation));
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
    /// annotation: the properties of an object type (a type literal, or an interface or alias the
    /// program declares), or the top-level class a type reference names.
    fn parameters(&mut self, function: u32, params: &FormalParameters<'_>) {
        for (index, parameter) in params.items.iter().enumerate() {
            let (Ok(index), Some(annotation)) = (u16::try_from(index), &parameter.type_annotation) else { continue };
            if parameter.pattern.get_identifier_name().is_none() {
                continue;
            }
            self.parameter_languages(function, index, parameter, annotation);
            if let TSType::TSTypeReference(reference) = &annotation.type_annotation
                && let TSTypeName::IdentifierReference(name) = &reference.type_name
                && let Some(Named::Class(class) | Named::Resolved { class: Some(class), .. }) = self.names.get(name.name.as_str())
            {
                self.declarations.declare_parameter_class(function, index, class);
            }
            for (member, languages) in self.object_members(&annotation.type_annotation) {
                self.declarations.declare_member(function, index, &member, languages);
            }
        }
    }

    /// The declared properties an object-shaped annotation names, each with a closed language set
    /// (`DECLARED_ANY` members are left out). An optional property admits `undefined`, as an optional
    /// class field does.
    fn object_members(&self, ty: &TSType<'_>) -> Vec<(Box<str>, u16)> {
        let members = self.names.members_of(ty).unwrap_or_default();
        members.into_iter().filter(|(_, languages)| *languages != DECLARED_ANY).collect()
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

    /// Record a local binding of a plain identifier with a declared type (an annotation, or an
    /// assertion initializer's type), in the innermost function.
    /// An array annotation is recorded for its element types even where its own languages are open.
    /// An object-shaped annotation (a literal, or an interface or alias the program declares) also
    /// records its properties for the local (`DeclaredTypes::declare_local_member`).
    fn local(&mut self, name: Option<&str>, ty: Option<&TSType<'_>>) {
        let (Some(function), Some(name), Some(ty)) = (self.function, name, ty) else { return };
        let languages = declared_language_in(ty, self.names);
        let elements = element_languages(ty, self.names);
        if languages != DECLARED_ANY || elements != DECLARED_ANY {
            self.declarations.declare_local(function, name, languages, elements);
        }
        for (member, languages) in self.object_members(ty) {
            self.declarations.declare_local_member(function, name, &member, languages);
        }
    }
}

/// The type an initializer asserts, for a local without an annotation: `e as T` and `<T>e`, through
/// parentheses. `as const` names no type, and `satisfies` and `e!` keep the operand's own type, so
/// they assert nothing here.
fn asserted_type<'b, 'a>(init: &'b Expression<'a>) -> Option<&'b TSType<'a>> {
    match init {
        Expression::ParenthesizedExpression(inner) => asserted_type(&inner.expression),
        Expression::TSAsExpression(value) if !is_const_assertion(&value.type_annotation) => Some(&value.type_annotation),
        Expression::TSTypeAssertion(value) => Some(&value.type_annotation),
        _ => None,
    }
}

/// Whether `ty` is the `const` of an `as const` assertion (a type reference named `const`).
fn is_const_assertion(ty: &TSType<'_>) -> bool {
    matches!(ty, TSType::TSTypeReference(reference)
        if matches!(&reference.type_name, TSTypeName::IdentifierReference(name) if name.name.as_str() == "const"))
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
    fn asserted_initializers_declare_their_locals_like_annotations() {
        let source = "function f() { const x = foo() as number; const y: number = foo(); const z = <string>bar(); \
            const w = baz() satisfies string; const v = qux()!; const u = quux() as const; }";
        let declared = declared_of(source, "asserted.cts");
        let f = source.find("function f").unwrap() as u32;
        assert_eq!(declared.declared_local(f, "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_local(f, "x"), declared.declared_local(f, "y"));
        assert_eq!(declared.declared_local(f, "z"), Some(bit(V::String)));
        assert_eq!(declared.declared_local(f, "w"), None);
        assert_eq!(declared.declared_local(f, "v"), None);
        assert_eq!(declared.declared_local(f, "u"), None);
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

    #[test]
    fn interface_and_alias_parameters_declare_their_members() {
        let source = "interface Point { x: number; y?: string }\ntype Named = { name: boolean };\nfunction f(p: Point, n: Named, z: Missing) {}";
        let declared = declared_of(source, "interfaces.cts");
        let f = source.find("function f").unwrap() as u32;
        assert_eq!(declared.declared_member(f, 0, "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(f, 0, "y"), Some(bit(V::String) | bit(V::Undefined)));
        assert_eq!(declared.declared_member(f, 0, "missing"), None);
        assert_eq!(declared.declared_member(f, 1, "name"), Some(bit(V::Boolean)));
        assert_eq!(declared.declared_member(f, 2, "anything"), None);
        // The language sets of the parameters: an interface admits objects and functions, an alias a plain object.
        assert_eq!(declared.declared_parameter(f, 0), Some(bit(V::Object) | bit(V::Function)));
        assert_eq!(declared.declared_parameter(f, 1), Some(crate::staged::value::language::OBJECT_PLAIN));
    }

    #[test]
    fn interface_extends_and_merging_build_one_member_set() {
        let source = "interface A { x: number; z: string }\ninterface B extends A { x: boolean; w: number }\n\
            interface C { m: number }\ninterface C { n: string }\nfunction g(b: B, c: C) {}";
        let declared = declared_of(source, "extends.cts");
        let g = source.find("function g").unwrap() as u32;
        // The child's `x` overrides the parent's; the parent's `z` is inherited.
        assert_eq!(declared.declared_member(g, 0, "x"), Some(bit(V::Boolean)));
        assert_eq!(declared.declared_member(g, 0, "z"), Some(bit(V::String)));
        assert_eq!(declared.declared_member(g, 0, "w"), Some(bit(V::Number)));
        // Two blocks of one interface merge.
        assert_eq!(declared.declared_member(g, 1, "m"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(g, 1, "n"), Some(bit(V::String)));
    }

    #[test]
    fn generic_members_typed_by_a_parameter_are_open() {
        let source = "interface Box<T> { value: T; count: number }\ntype Pair<K> = { key: K; size: string };\nfunction h(b: Box<number>, p: Pair<string>) {}";
        let declared = declared_of(source, "generic.cts");
        let h = source.find("function h").unwrap() as u32;
        assert_eq!(declared.declared_member(h, 0, "value"), None);
        assert_eq!(declared.declared_member(h, 0, "count"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(h, 1, "key"), None);
        assert_eq!(declared.declared_member(h, 1, "size"), Some(bit(V::String)));
    }

    #[test]
    fn alias_and_heritage_cycles_terminate() {
        let source = "type Loop = Loop2;\ntype Loop2 = Loop;\ninterface Ring extends Ring { r: number }\n\
            type Both = Both & { k: string };\nfunction c(v: Loop, r: Ring, b: Both) {}";
        let declared = declared_of(source, "cycles.cts");
        let c = source.find("function c").unwrap() as u32;
        assert_eq!(declared.declared_member(c, 0, "anything"), None);
        assert_eq!(declared.declared_member(c, 1, "r"), Some(bit(V::Number)));
        assert_eq!(declared.declared_member(c, 2, "k"), Some(bit(V::String)));
    }

    #[test]
    fn local_object_annotations_declare_their_members() {
        let source = "interface Point { x: number }\nfunction f() { const p: Point = { x: 1 }; let q: {y: string} = { y: '' }; }\nconst top: Point = { x: 2 };";
        let program = Engine::specialize_typescript(source, "locals.cts").unwrap();
        let declared = program.declared_types.as_deref().expect("a typed program keeps its declarations");
        let f = source.find("function f").unwrap() as u32;
        assert_eq!(declared.declared_local_member(f, "p", "x"), Some(bit(V::Number)));
        assert_eq!(declared.declared_local_member(f, "q", "y"), Some(bit(V::String)));
        assert_eq!(declared.declared_local_member(f, "p", "y"), None);
        assert_eq!(declared.declared_local_member(u32::MAX, "top", "x"), Some(bit(V::Number)));
        // The members survive the binary round trip.
        let bytes: crate::KeptBytes = std::rc::Rc::new(program.encode_trusted());
        let decoded = crate::ResidualProgram::decode_trusted(bytes).unwrap();
        assert_eq!(decoded.declared_types, program.declared_types);
        assert_eq!(decoded.declared_types.as_deref().map(|declared| declared.local_members.len()), Some(3));
    }
}
