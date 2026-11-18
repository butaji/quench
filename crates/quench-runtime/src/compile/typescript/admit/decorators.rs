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

use super::*;

/// One decorated element: where it lives, its decorators, and its metadata.
struct Decoration<'a> {
    is_static: bool,
    /// The property key as a JavaScript literal; `None` for the class itself.
    key: Option<String>,
    /// A method or accessor (its descriptor is decorated), not a field.
    has_descriptor: bool,
    decorators: Vec<Expression<'a>>,
    parameters: Vec<(usize, Expression<'a>)>,
    metadata: Vec<(&'static str, String)>,
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
        let mut values = Vec::new();
        let mut text = String::new();
        let keyword = if matches!(how, Export::Module) { "export let" } else { "let" };
        text.push_str(&format!("{keyword} {name} = $$quench_ts_0;\n"));
        values.push(self.javascript_expression("null", span));
        for decoration in decorations {
            text.push_str(&decoration_text(&name, decoration, &mut values));
        }
        if let Export::Namespace(namespace) = how {
            text.push_str(&format!("{namespace}.{name} = {name};\n"));
        }
        class.r#type = ClassType::ClassExpression;
        values[0] = Expression::ClassExpression(class);
        let mut statements = self.javascript(&text, span);
        fill(&mut statements, values);
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
                            ("design:type", "Function".to_owned()),
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
            let types = constructor_types.unwrap_or_else(|| "[]".to_owned());
            statics.push(Decoration {
                is_static: true, key: None, has_descriptor: false,
                decorators: class_decorators, parameters: constructor_parameters,
                metadata: vec![("design:paramtypes", types)],
            });
        }
        instance.extend(statics);
        instance
    }

    fn parameter_types(&mut self, params: &FormalParameters<'a>) -> String {
        let types: Vec<String> = params.items.iter()
            .map(|item| self.serialize(item.type_annotation.as_deref().map(|t| &t.type_annotation), false))
            .collect();
        format!("[{}]", types.join(", "))
    }

    /// tsc's runtime serialization of a declared type: the constructor a value of it has,
    /// `Object` where none is known, `void 0` for no value (and an unannotated return).
    fn serialize(&mut self, ty: Option<&TSType<'a>>, is_return: bool) -> String {
        let Some(ty) = ty else { return if is_return { "void 0" } else { "Object" }.to_owned() };
        match ty {
            TSType::TSNumberKeyword(_) => "Number".to_owned(),
            TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => "String".to_owned(),
            TSType::TSBooleanKeyword(_) => "Boolean".to_owned(),
            TSType::TSBigIntKeyword(_) => guarded("BigInt"),
            TSType::TSSymbolKeyword(_) => guarded("Symbol"),
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSNullKeyword(_) | TSType::TSNeverKeyword(_) => "void 0".to_owned(),
            TSType::TSArrayType(_) | TSType::TSTupleType(_) => "Array".to_owned(),
            TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => "Function".to_owned(),
            TSType::TSLiteralType(literal) => match &literal.literal {
                TSLiteral::NumericLiteral(_) => "Number".to_owned(),
                TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => "String".to_owned(),
                TSLiteral::BooleanLiteral(_) => "Boolean".to_owned(),
                TSLiteral::BigIntLiteral(_) => guarded("BigInt"),
                TSLiteral::UnaryExpression(_) => "Number".to_owned(),
            },
            TSType::TSParenthesizedType(inner) => self.serialize(Some(&inner.type_annotation), is_return),
            TSType::TSTypeOperatorType(operator) if operator.operator == TSTypeOperatorOperator::Readonly => {
                self.serialize(Some(&operator.type_annotation), is_return)
            }
            TSType::TSUnionType(union) => {
                let mut serialized: Option<String> = None;
                for member in &union.types {
                    if matches!(member, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSNeverKeyword(_)) { continue; }
                    let text = self.serialize(Some(member), is_return);
                    match &serialized {
                        None => serialized = Some(text),
                        Some(previous) if *previous == text => {}
                        Some(_) => return "Object".to_owned(),
                    }
                }
                serialized.unwrap_or_else(|| "void 0".to_owned())
            }
            TSType::TSTypeReference(reference) => self.reference(&reference.type_name),
            _ => "Object".to_owned(),
        }
    }

    /// A named type at run time: the value of its name when that is a constructor (a class,
    /// an imported class), else `Object` (an interface, an alias, a missing binding).
    fn reference(&mut self, name: &TSTypeName<'a>) -> String {
        match name {
            TSTypeName::IdentifierReference(identifier) => match identifier.name.as_str() {
                name if self.type_names.contains(name) => "Object".to_owned(),
                name if self.imports.contains_key(name) => {
                    let binding = self.metadata_binding(name);
                    format!("(typeof {binding} === \"function\" ? {binding} : Object)")
                }
                "Array" | "ReadonlyArray" => "Array".to_owned(),
                "Promise" | "PromiseLike" => guarded("Promise"),
                name => guarded(name),
            },
            TSTypeName::QualifiedName(qualified) => {
                let text = qualified_text(qualified);
                let (root, rest) = text.split_once('.').unwrap_or((&text, ""));
                let binding = self.metadata_binding(root);
                let text = format!("{binding}.{rest}");
                format!("(typeof {binding} !== \"undefined\" && {binding} !== null && typeof {text} === \"function\" ? {text} : Object)")
            }
            TSTypeName::ThisExpression(_) => "Object".to_owned(),
        }
    }
}

fn guarded(name: &str) -> String {
    format!("(typeof {name} === \"function\" ? {name} : Object)")
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
        key => key.static_name().map(|name| enums::quote(&name)),
    }
}

fn take_parameter_decorators<'a>(params: &mut FormalParameters<'a>) -> Vec<(usize, Expression<'a>)> {
    let mut decorators = Vec::new();
    for (index, parameter) in params.items.iter_mut().enumerate() {
        decorators.extend(parameter.decorators.drain(..).map(|decorator| (index, decorator.expression)));
    }
    decorators
}

/// One decoration as statements: decorators evaluated in source order (member decorators,
/// then parameter decorators), then applied in reverse with the metadata first.
fn decoration_text<'a>(class: &str, decoration: Decoration<'a>, values: &mut Vec<Expression<'a>>) -> String {
    let mut placeholder = |expression: Expression<'a>| {
        values.push(expression);
        format!("$$quench_ts_{}", values.len() - 1)
    };
    let decorators: Vec<String> = decoration.decorators.into_iter().map(&mut placeholder).collect();
    let parameters: Vec<(usize, String)> = decoration.parameters.into_iter()
        .map(|(index, expression)| (index, placeholder(expression))).collect();
    let mut text = String::from("{\n");
    let mut temporaries: Vec<String> = Vec::new();
    for (index, value) in decorators.iter().chain(parameters.iter().map(|(_, value)| value)).enumerate() {
        temporaries.push(format!("$$quench_d{index} = {value}"));
    }
    let metadata = |target: &str, key: &str, result: &str| -> String {
        let applied: String = decoration.metadata.iter().rev().map(|(name, value)| match result {
            "" => format!("Reflect.metadata(\"{name}\", {value})({target}, {key}); "),
            result => format!("{result} = Reflect.metadata(\"{name}\", {value})({target}, {key}, {result}) || {result}; "),
        }).collect();
        format!("if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") {{ {applied}}}\n")
    };
    match &decoration.key {
        None => {
            if !temporaries.is_empty() { text.push_str(&format!("const {};\n", temporaries.join(", "))); }
            text.push_str(&metadata(class, "void 0", class).replace(&format!("({class}, void 0, {class})"), &format!("({class})")));
            for (offset, (index, _)) in parameters.iter().enumerate().rev() {
                text.push_str(&format!("$$quench_d{}({class}, void 0, {index});\n", decorators.len() + offset));
            }
            for index in (0..decorators.len()).rev() {
                text.push_str(&format!("{class} = $$quench_d{index}({class}) || {class};\n"));
            }
        }
        Some(key) => {
            let target = if decoration.is_static { class.to_owned() } else { format!("{class}.prototype") };
            temporaries.insert(0, format!("$$quench_t = {target}"));
            text.push_str(&format!("const {};\n", temporaries.join(", ")));
            let descriptor = if decoration.has_descriptor { format!("Object.getOwnPropertyDescriptor($$quench_t, {key})") } else { "void 0".to_owned() };
            text.push_str(&format!("let $$quench_r = {descriptor};\n"));
            text.push_str(&metadata("$$quench_t", key, "$$quench_r"));
            for (offset, (index, _)) in parameters.iter().enumerate().rev() {
                text.push_str(&format!("$$quench_d{}($$quench_t, {key}, {index});\n", decorators.len() + offset));
            }
            for index in (0..decorators.len()).rev() {
                text.push_str(&format!("$$quench_r = $$quench_d{index}($$quench_t, {key}, $$quench_r) || $$quench_r;\n"));
            }
            text.push_str(&format!("$$quench_r && Object.defineProperty($$quench_t, {key}, $$quench_r);\n"));
        }
    }
    text.push_str("}\n");
    text
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

impl<'a> Admit<'a, '_> {
    /// The run-time expression of `name` for metadata: an imported binding through a
    /// namespace import of its module (a missing export reads `undefined`), else the name.
    fn metadata_binding(&mut self, name: &str) -> String {
        let Some((source, imported)) = self.imports.get(name).cloned() else { return name.to_owned() };
        let Some(imported) = imported else { return name.to_owned() };
        let namespace = match self.metadata_namespaces.iter().find(|(known, _)| *known == source) {
            Some((_, namespace)) => namespace.clone(),
            None => {
                let namespace = format!("$$quench_metadata_{}", self.metadata_namespaces.len());
                self.metadata_namespaces.push((source, namespace.clone()));
                namespace
            }
        };
        format!("{namespace}[{}]", enums::quote(&imported))
    }

    pub(super) fn import_metadata_namespaces(&mut self, body: &mut ArenaVec<'a, Statement<'a>>) {
        if self.metadata_namespaces.is_empty() { return; }
        let text: String = self.metadata_namespaces.iter()
            .map(|(source, namespace)| format!("import * as {namespace} from {};\n", enums::quote(source))).collect();
        let imports = self.javascript(&text, Span::empty(0));
        let rest = std::mem::replace(body, ArenaVec::new_in(&self.allocator));
        body.extend(imports);
        body.extend(rest);
    }
}
