//! Decorators, with TypeScript's `experimentalDecorators` meaning (what Angular, NestJS,
//! TypeORM, inversify and MobX code is written against), and `emitDecoratorMetadata`: the
//! declared types of a decorated member become `design:type`, `design:paramtypes` and
//! `design:returntype` metadata, which dependency injection reads. The declarations stay
//! the program's: the metadata is computed from the annotations left on their nodes.
//!
//! A decorated class `C` becomes `let C = class C { … }` followed by the decorations in
//! tsc's order (instance members, static members, then the class), each applied in place
//! with no helper function: decorators evaluate in source order and apply in reverse, a
//! member's returned descriptor is defined, and a class decorator's result replaces `C`.
//! Generated code holds no functions, so a later parse (replay, reparse) admits it the same.
//! The statements are built as AST nodes in `decorators/ast.rs`.

use super::*;

mod ast;

/// One decorated element: where it lives, its decorators, and its metadata.
struct Decoration<'a> {
    is_static: bool,
    /// The property key's name; `None` for the class itself.
    key: Option<String>,
    /// A method or accessor (its descriptor is decorated), not a field.
    has_descriptor: bool,
    decorators: Vec<Expression<'a>>,
    parameters: Vec<(usize, Expression<'a>)>,
    metadata: Vec<(&'static str, Runtime)>,
}

/// A metadata value: what it is at run time.
#[derive(Clone, PartialEq)]
enum Runtime {
    /// A global read by its name: `Number`, `Object`, `Array`.
    Global(&'static str),
    /// `void 0`.
    Void,
    /// A name that is a constructor if it is a function, else `Object`.
    Guarded(Reference),
    /// A qualified type name, whose root may be missing, else `Object`.
    Qualified(Reference),
    /// The parameter types of a method or constructor.
    Array(Vec<Runtime>),
}

/// A name read at run time, then the static members of a qualified type name.
#[derive(Clone, PartialEq)]
struct Reference {
    binding: Binding,
    members: Vec<String>,
}

/// A name in scope, or an import read through its module's namespace object.
#[derive(Clone, PartialEq)]
enum Binding {
    Name(String),
    This,
    /// `ns["name"]`: the imported name, read through a namespace import of its module.
    Imported { namespace: String, name: String },
}

impl<'a> Admit<'a, '_> {
    /// A class declaration with decorators anywhere (class, members, parameters) as `let C =
    /// class C {}` and its decorations; `None` (the class unchanged) when it has none or one
    /// this admission leaves to the decorator check (computed or private keys, no name), which
    /// gets the class back.
    pub(super) fn decorated_class(
        &mut self, mut class: ArenaBox<'a, Class<'a>>, span: Span, how: Export<'_>,
    ) -> Result<(String, Vec<Produced<'a>>), ArenaBox<'a, Class<'a>>> {
        let Some(name) = class.id.as_ref().map(|id| id.name.to_string()) else { return Err(class) };
        if !class_has_decorators(&class) || !class.body.body.iter().all(element_decorators_are_static) {
            return Err(class);
        }
        let decorations = self.decorations(&mut class);
        class.r#type = ClassType::ClassExpression;
        let emit = ast::Emit::new(self.allocator, span);
        let mut statements = vec![emit.class_binding(&name, class, how)];
        statements.extend(decorations.into_iter().map(|decoration| emit.decoration(&name, decoration)));
        if let Export::Namespace(namespace) = how {
            statements.push(emit.publish(namespace, &name));
        }
        Ok((name, statements.into_iter().map(|statement| (statement, false)).collect()))
    }

    /// The class's decorations in tsc's order, taken out of its nodes.
    fn decorations(&mut self, class: &mut Class<'a>) -> Vec<Decoration<'a>> {
        let mut instance = Vec::new();
        let mut statics = Vec::new();
        let mut constructor_parameters = Vec::new();
        let mut constructor_types = None;
        for element in class.body.body.iter_mut() {
            match element {
                ClassElement::MethodDefinition(method) if method.kind.is_constructor() => {
                    constructor_parameters = take_parameter_decorators(&mut method.value.params);
                    constructor_types = Some(self.parameter_types(&method.value.params));
                }
                ClassElement::MethodDefinition(method) => {
                    let parameters = take_parameter_decorators(&mut method.value.params);
                    if method.decorators.is_empty() && parameters.is_empty() { continue; }
                    let decorators = method.decorators.drain(..).map(|decorator| decorator.expression).collect();
                    let metadata = match method.kind {
                        MethodDefinitionKind::Get => vec![("design:type", self.serialize(method.value.return_type.as_deref().map(|t| &t.type_annotation), false))],
                        MethodDefinitionKind::Set => {
                            let types = self.parameter_types(&method.value.params);
                            let first = method.value.params.items.first().and_then(|item| item.type_annotation.as_deref()).map(|t| &t.type_annotation);
                            vec![("design:type", self.serialize(first, false)), ("design:paramtypes", types)]
                        }
                        _ => vec![
                            ("design:type", Runtime::Global("Function")),
                            ("design:paramtypes", self.parameter_types(&method.value.params)),
                            ("design:returntype", self.serialize(method.value.return_type.as_deref().map(|t| &t.type_annotation), true)),
                        ],
                    };
                    let decoration = Decoration {
                        is_static: method.r#static, key: static_key(&method.key), has_descriptor: true,
                        decorators, parameters, metadata,
                    };
                    if method.r#static { statics.push(decoration) } else { instance.push(decoration) }
                }
                ClassElement::PropertyDefinition(property) if !property.decorators.is_empty() => {
                    let decorators = property.decorators.drain(..).map(|decorator| decorator.expression).collect();
                    let metadata = vec![("design:type", self.serialize(property.type_annotation.as_deref().map(|t| &t.type_annotation), false))];
                    let decoration = Decoration {
                        is_static: property.r#static, key: static_key(&property.key), has_descriptor: false,
                        decorators, parameters: Vec::new(), metadata,
                    };
                    if property.r#static { statics.push(decoration) } else { instance.push(decoration) }
                }
                ClassElement::AccessorProperty(accessor) if !accessor.decorators.is_empty() => {
                    let decorators = accessor.decorators.drain(..).map(|decorator| decorator.expression).collect();
                    let metadata = vec![("design:type", self.serialize(accessor.type_annotation.as_deref().map(|t| &t.type_annotation), false))];
                    let decoration = Decoration {
                        is_static: accessor.r#static, key: static_key(&accessor.key), has_descriptor: true,
                        decorators, parameters: Vec::new(), metadata,
                    };
                    if accessor.r#static { statics.push(decoration) } else { instance.push(decoration) }
                }
                _ => {}
            }
        }
        let class_decorators: Vec<_> = class.decorators.drain(..).map(|decorator| decorator.expression).collect();
        if !class_decorators.is_empty() || !constructor_parameters.is_empty() {
            let types = constructor_types.unwrap_or_else(|| Runtime::Array(Vec::new()));
            statics.push(Decoration {
                is_static: true, key: None, has_descriptor: false,
                decorators: class_decorators, parameters: constructor_parameters,
                metadata: vec![("design:paramtypes", types)],
            });
        }
        instance.extend(statics);
        instance
    }

    fn parameter_types(&mut self, params: &FormalParameters<'a>) -> Runtime {
        let types = params.items.iter()
            .map(|item| self.serialize(item.type_annotation.as_deref().map(|t| &t.type_annotation), false))
            .collect();
        Runtime::Array(types)
    }

    /// tsc's runtime serialization of a declared type: the constructor a value of it has,
    /// `Object` where none is known, `void 0` for no value (and an unannotated return).
    fn serialize(&mut self, ty: Option<&TSType<'a>>, is_return: bool) -> Runtime {
        let Some(ty) = ty else { return if is_return { Runtime::Void } else { Runtime::Global("Object") } };
        match ty {
            TSType::TSNumberKeyword(_) => Runtime::Global("Number"),
            TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => Runtime::Global("String"),
            TSType::TSBooleanKeyword(_) => Runtime::Global("Boolean"),
            TSType::TSBigIntKeyword(_) => guarded("BigInt"),
            TSType::TSSymbolKeyword(_) => guarded("Symbol"),
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSNullKeyword(_) | TSType::TSNeverKeyword(_) => Runtime::Void,
            TSType::TSArrayType(_) | TSType::TSTupleType(_) => Runtime::Global("Array"),
            TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => Runtime::Global("Function"),
            TSType::TSLiteralType(literal) => match &literal.literal {
                TSLiteral::NumericLiteral(_) => Runtime::Global("Number"),
                TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => Runtime::Global("String"),
                TSLiteral::BooleanLiteral(_) => Runtime::Global("Boolean"),
                TSLiteral::BigIntLiteral(_) => guarded("BigInt"),
                TSLiteral::UnaryExpression(_) => Runtime::Global("Number"),
            },
            TSType::TSParenthesizedType(inner) => self.serialize(Some(&inner.type_annotation), is_return),
            TSType::TSTypeOperatorType(operator) if operator.operator == TSTypeOperatorOperator::Readonly => {
                self.serialize(Some(&operator.type_annotation), is_return)
            }
            TSType::TSUnionType(union) => {
                let mut serialized: Option<Runtime> = None;
                for member in &union.types {
                    if matches!(member, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSNeverKeyword(_)) { continue; }
                    let runtime = self.serialize(Some(member), is_return);
                    match &serialized {
                        None => serialized = Some(runtime),
                        Some(previous) if *previous == runtime => {}
                        Some(_) => return Runtime::Global("Object"),
                    }
                }
                serialized.unwrap_or(Runtime::Void)
            }
            TSType::TSTypeReference(reference) => self.reference(&reference.type_name),
            _ => Runtime::Global("Object"),
        }
    }

    /// A named type at run time: the value of its name when that is a constructor (a class,
    /// an imported class), else `Object` (an interface, an alias, a missing binding).
    fn reference(&mut self, name: &TSTypeName<'a>) -> Runtime {
        match name {
            TSTypeName::IdentifierReference(identifier) => match identifier.name.as_str() {
                name if self.type_names.contains(name) => Runtime::Global("Object"),
                name if self.imports.contains_key(name) => {
                    let binding = self.metadata_binding(name);
                    Runtime::Guarded(Reference { binding, members: Vec::new() })
                }
                "Array" | "ReadonlyArray" => Runtime::Global("Array"),
                "Promise" | "PromiseLike" => guarded("Promise"),
                name => guarded(name),
            },
            TSTypeName::QualifiedName(qualified) => Runtime::Qualified(self.qualified(qualified)),
            TSTypeName::ThisExpression(_) => Runtime::Global("Object"),
        }
    }

    /// `A.B.C` as its leftmost name (or `this`) and the members after it.
    fn qualified(&mut self, name: &TSQualifiedName<'a>) -> Reference {
        let mut members = vec![name.right.name.to_string()];
        let mut left = &name.left;
        let binding = loop {
            match left {
                TSTypeName::QualifiedName(inner) => {
                    members.push(inner.right.name.to_string());
                    left = &inner.left;
                }
                TSTypeName::IdentifierReference(identifier) => break self.metadata_binding(identifier.name.as_str()),
                TSTypeName::ThisExpression(_) => break Binding::This,
            }
        };
        members.reverse();
        Reference { binding, members }
    }

    /// The run-time binding of `name` for metadata: an imported binding through a namespace
    /// import of its module (a missing export reads `undefined`), else the name.
    fn metadata_binding(&mut self, name: &str) -> Binding {
        let Some((source, imported)) = self.imports.get(name).cloned() else { return Binding::Name(name.to_owned()) };
        let Some(imported) = imported else { return Binding::Name(name.to_owned()) };
        let namespace = match self.metadata_namespaces.iter().find(|(known, _)| *known == source) {
            Some((_, namespace)) => namespace.clone(),
            None => {
                let namespace = format!("$$quench_metadata_{}", self.metadata_namespaces.len());
                self.metadata_namespaces.push((source, namespace.clone()));
                namespace
            }
        };
        Binding::Imported { namespace, name: imported }
    }

    pub(super) fn import_metadata_namespaces(&mut self, body: &mut ArenaVec<'a, Statement<'a>>) {
        if self.metadata_namespaces.is_empty() { return; }
        let imports = ast::metadata_imports(self.allocator, &self.metadata_namespaces);
        let rest = std::mem::replace(body, ArenaVec::new_in(&self.allocator));
        body.extend(imports);
        body.extend(rest);
    }
}

/// A constructor-or-`Object` metadata value of a global name.
fn guarded(name: &str) -> Runtime {
    Runtime::Guarded(Reference { binding: Binding::Name(name.to_owned()), members: Vec::new() })
}

fn class_has_decorators(class: &Class<'_>) -> bool {
    !class.decorators.is_empty() || class.body.body.iter().any(|element| match element {
        ClassElement::MethodDefinition(method) => !method.decorators.is_empty()
            || method.value.params.items.iter().any(|item| !item.decorators.is_empty()),
        ClassElement::PropertyDefinition(property) => !property.decorators.is_empty(),
        ClassElement::AccessorProperty(accessor) => !accessor.decorators.is_empty(),
        _ => false,
    })
}

/// A decorated element's key is a name or literal (a computed or private one stays for the
/// decorator check).
fn element_decorators_are_static(element: &ClassElement<'_>) -> bool {
    let (decorated, key) = match element {
        ClassElement::MethodDefinition(method) => (
            !method.kind.is_constructor() && (!method.decorators.is_empty()
                || method.value.params.items.iter().any(|item| !item.decorators.is_empty())),
            &method.key,
        ),
        ClassElement::PropertyDefinition(property) => (!property.decorators.is_empty(), &property.key),
        ClassElement::AccessorProperty(accessor) => (!accessor.decorators.is_empty(), &accessor.key),
        _ => return true,
    };
    !decorated || static_key(key).is_some()
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::PrivateIdentifier(_) => None,
        key => key.static_name().map(|name| name.to_string()),
    }
}

fn take_parameter_decorators<'a>(params: &mut FormalParameters<'a>) -> Vec<(usize, Expression<'a>)> {
    let mut decorators = Vec::new();
    for (index, parameter) in params.items.iter_mut().enumerate() {
        decorators.extend(parameter.decorators.drain(..).map(|decorator| (index, decorator.expression)));
    }
    decorators
}

/// The value import bindings of `program` by local name: source, and imported name (`None`
/// for a namespace import).
pub(super) fn import_bindings(program: &Program<'_>) -> FxHashMap<String, (String, Option<String>)> {
    let mut bindings = FxHashMap::default();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.import_kind.is_type() { continue; }
        for specifier in import.specifiers.iter().flatten() {
            let imported = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier) if specifier.import_kind.is_type() => continue,
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => Some(specifier.imported.name().to_string()),
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => Some("default".to_owned()),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => None,
            };
            bindings.insert(specifier.local().name.to_string(), (import.source.value.to_string(), imported));
        }
    }
    bindings
}
