//! Declared function signatures, local bindings and parameter shapes: what a TypeScript annotation
//! says a function returns, takes as its rest parameter or as `this`, what each annotated local
//! holds, and what each parameter's type literal or top-level class type declares
//! (`DeclaredTypes::signatures`, `DeclaredTypes::locals`, `DeclaredTypes::members`,
//! `DeclaredTypes::parameter_classes`). A declaration is a seed, not a proof (TypeScript is
//! unsound), so a tier checks it once where a value enters and trusts it.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use rustc_hash::FxHashMap;

use crate::bytecode::{DeclaredSignature, DeclaredTypes};
use crate::compile::class::class_method_source_span;
use crate::typed_ir::language_bit;
use super::{DECLARED_ANY, declared_language};

/// The function a local is declared in when it is top-level code of a script or module.
const TOP_LEVEL: u32 = u32::MAX;

/// Record each declared signature and annotated local of `program`, keyed by the source span
/// start each function's bytecode header begins at. Run before admission, so the annotations
/// are still in place.
pub(super) fn collect<'a>(program: &Program<'a>, declarations: &mut DeclaredTypes) {
    let mut collect = Collect {
        text: program.source_text,
        declarations,
        function: Some(TOP_LEVEL),
        class: TOP_LEVEL,
        method: None,
        loop_head: false,
        classes: top_level_classes(program),
    };
    collect.visit_program(program);
}

/// The top-level classes a parameter annotation may name, by name, with each class's span start
/// (the key `DeclaredField::class_start` uses). A name counts only when one top-level declaration
/// (a class, an interface, a type alias or an enum) declares it, so the annotation denotes that class.
fn top_level_classes(program: &Program<'_>) -> FxHashMap<String, u32> {
    let mut declared: FxHashMap<String, u32> = FxHashMap::default();
    let mut classes: FxHashMap<String, u32> = FxHashMap::default();
    let mut note = |id: &BindingIdentifier<'_>, class: Option<u32>| {
        let name = id.name.as_str().to_string();
        *declared.entry(name.clone()).or_default() += 1;
        if let Some(start) = class {
            classes.insert(name, start);
        }
    };
    for statement in &program.body {
        match statement {
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id { note(id, Some(class.span.start)) }
            }
            Statement::TSInterfaceDeclaration(declaration) => note(&declaration.id, None),
            Statement::TSTypeAliasDeclaration(declaration) => note(&declaration.id, None),
            Statement::TSEnumDeclaration(declaration) => note(&declaration.id, None),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = &class.id { note(id, Some(class.span.start)) }
                }
                Declaration::TSInterfaceDeclaration(declaration) => note(&declaration.id, None),
                Declaration::TSTypeAliasDeclaration(declaration) => note(&declaration.id, None),
                Declaration::TSEnumDeclaration(declaration) => note(&declaration.id, None),
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration {
                    if let Some(id) = &class.id { note(id, Some(class.span.start)) }
                }
            }
            _ => {}
        }
    }
    classes.retain(|name, _| declared.get(name) == Some(&1));
    classes
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
    /// The top-level classes a parameter annotation may name (`top_level_classes`), by name.
    classes: FxHashMap<String, u32>,
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
            let returns = returns_of(function.r#async || function.generator, function.return_type.as_deref());
            let this = this_of(function.this_param.as_deref());
            self.record(start, returns, &function.params, function.type_parameters.as_deref(), this);
        }
        let outer = std::mem::replace(&mut self.function, key);
        walk::walk_function(self, function, flags);
        self.function = outer;
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let start = arrow.span.start;
        let returns = returns_of(arrow.r#async, arrow.return_type.as_deref());
        self.record(start, returns, &arrow.params, arrow.type_parameters.as_deref(), DECLARED_ANY);
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
    fn record(&mut self, start: u32, returns: u16, params: &FormalParameters<'_>,
        type_parameters: Option<&TSTypeParameterDeclaration<'_>>, this: u16) {
        let rest = rest_of(params);
        if returns != DECLARED_ANY || rest != DECLARED_ANY || this != DECLARED_ANY {
            self.declarations.declare_signature(DeclaredSignature { start, returns, rest, this });
        }
        self.parameters(start, params, type_parameters);
    }

    /// Record the declared shape of each parameter with a plain identifier pattern and a type
    /// annotation: the properties of a type literal, or the top-level class a type reference names.
    /// A type parameter of the function that has the class's name shadows it, so it records nothing.
    fn parameters(&mut self, function: u32, params: &FormalParameters<'_>,
        type_parameters: Option<&TSTypeParameterDeclaration<'_>>) {
        for (index, parameter) in params.items.iter().enumerate() {
            let (Ok(index), Some(annotation)) = (u16::try_from(index), &parameter.type_annotation) else { continue };
            if parameter.pattern.get_identifier_name().is_none() {
                continue;
            }
            match &annotation.type_annotation {
                TSType::TSTypeLiteral(literal) => {
                    for member in &literal.members {
                        self.member(function, index, member);
                    }
                }
                TSType::TSTypeReference(reference) => {
                    let TSTypeName::IdentifierReference(name) = &reference.type_name else { continue };
                    let name = name.name.as_str();
                    if shadowed(type_parameters, name) {
                        continue;
                    }
                    if let Some(&class) = self.classes.get(name) {
                        self.declarations.declare_parameter_class(function, index, class);
                    }
                }
                _ => {}
            }
        }
    }

    /// A property of a parameter's type literal with a static identifier key and a type annotation.
    /// An optional property admits `undefined`, as an optional class field does.
    fn member(&mut self, function: u32, parameter: u16, member: &TSSignature<'_>) {
        let TSSignature::TSPropertySignature(property) = member else { return };
        let PropertyKey::StaticIdentifier(identifier) = &property.key else { return };
        let Some(annotation) = &property.type_annotation else { return };
        let mut languages = declared_language(&annotation.type_annotation);
        if property.optional {
            languages |= language_bit(crate::ValueType::Undefined);
        }
        if languages != DECLARED_ANY {
            self.declarations.declare_member(function, parameter, identifier.name.as_str(), languages);
        }
    }

    /// Record a local binding of a plain identifier with a type annotation, in the innermost function.
    fn local(&mut self, name: Option<&str>, annotation: Option<&TSTypeAnnotation<'_>>) {
        let (Some(function), Some(name), Some(annotation)) = (self.function, name, annotation) else { return };
        let languages = declared_language(&annotation.type_annotation);
        if languages != DECLARED_ANY {
            self.declarations.declare_local(function, name, languages);
        }
    }
}

/// Whether a type parameter of the function (`type_parameters`) has the name `name`, so that the
/// name denotes it rather than a top-level class.
fn shadowed(type_parameters: Option<&TSTypeParameterDeclaration<'_>>, name: &str) -> bool {
    type_parameters.is_some_and(|parameters| parameters.params.iter().any(|parameter| parameter.name.name.as_str() == name))
}

/// The return type's languages. An `async` function returns a Promise and a generator returns
/// an iterator: both are objects whatever the annotation names.
fn returns_of(object: bool, annotation: Option<&TSTypeAnnotation<'_>>) -> u16 {
    if object {
        language_bit(crate::ValueType::Object)
    } else {
        annotation.map_or(DECLARED_ANY, |annotation| declared_language(&annotation.type_annotation))
    }
}

/// The `this` parameter's languages. A named type stays open (`DECLARED_ANY`): it may be an
/// alias of a primitive or a type parameter, which no syntax tells apart from a class.
fn this_of(parameter: Option<&TSThisParameter<'_>>) -> u16 {
    parameter.and_then(|parameter| parameter.type_annotation.as_deref())
        .map_or(DECLARED_ANY, |annotation| declared_language(&annotation.type_annotation))
}

/// The languages of an annotated rest parameter's elements: `...xs: T[]` and `...xs: Array<T>`
/// hold `T`s.
fn rest_of(params: &FormalParameters<'_>) -> u16 {
    let Some(annotation) = params.rest.as_ref().and_then(|rest| rest.type_annotation.as_deref()) else {
        return DECLARED_ANY;
    };
    match &annotation.type_annotation {
        TSType::TSArrayType(array) => declared_language(&array.element_type),
        TSType::TSTypeReference(reference) => match (&reference.type_name, reference.type_arguments.as_deref()) {
            (TSTypeName::IdentifierReference(name), Some(arguments))
                if name.name.as_str() == "Array" && arguments.params.len() == 1 =>
            {
                declared_language(&arguments.params[0])
            }
            _ => DECLARED_ANY,
        },
        _ => DECLARED_ANY,
    }
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
}
