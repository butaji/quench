mod method_name;
pub(super) use method_name::class_method_function_name;

use super::*;
use crate::bytecode::{CALL_INVOKE, ELEMENT_STORE, JUMP_IF_FALSE, OBJECT_FIELD_STORE, REGISTER_MOVE, Traits, the_op};

/// The op that puts a block-scoped binding into its TDZ.
const TDZ_INIT: Op = the_op(Traits::TDZ_INITIALIZE);
/// The op that checks a class `extends` value.
const CLASS_HERITAGE_CHECK: Op = the_op(Traits::CLASS_HERITAGE_CHECK);
/// The op that makes a closure of a function id.
const CLOSURE_MAKER: Op = the_op(Traits::MAKES_CLOSURE);
/// The op that reads a named field.
const FIELD_READ: Op = the_op(Traits::FIELD_LEAF);
/// The op that marks a private name onto a home object.
const PRIVATE_NAME_MARK: Op = the_op(Traits::PRIVATE_NAME_MARK);
/// The op that always jumps.
const JUMP: Op = the_op(Traits::UNCONDITIONAL_JUMP);
/// The op that converts a computed key to a property key.
const PROPERTY_KEY_CONVERT: Op = the_op(Traits::PROPERTY_KEY_CONVERT);
/// The op that sets a function name from a computed key.
const FUNCTION_NAME_KEY_SET: Op = the_op(Traits::FUNCTION_NAME_KEY_SET);
/// The op that defines a private field on an instance.
const PRIVATE_FIELD_DEFINE: Op = the_op(Traits::PRIVATE_FIELD_DEFINE);
/// The op that defines a computed-key field on an instance.
const COMPUTED_FIELD_DEFINE: Op = the_op(Traits::COMPUTED_FIELD_DEFINE);
/// The op that defines a named field on an instance.
const FIELD_DEFINE: Op = the_op(Traits::FIELD_DEFINE);
/// The op that stores a value into a local binding slot.
const LOCAL_BINDING_STORE: Op = the_op(Traits::LOCAL_BINDING_STORE);
/// The op that defines a property from a definition record.
const PROPERTY_RECORD_DEFINE: Op = the_op(Traits::PROPERTY_RECORD_DEFINE);
/// The op that loads `this`.
const THIS_LOAD: Op = the_op(Traits::THIS_LEAF);

/// The rejection for a class field key the compiler does not lower.
const CLASS_FIELD_KEY_UNSUPPORTED: &str = "class field key is unsupported";

impl FunctionCompiler<'_, '_> {
    pub(super) fn class_expression(&mut self, class: &Class<'_>) -> Register {
        self.lower_class(class, false, None)
    }

    pub(super) fn named_class_expression(&mut self, class: &Class<'_>, name: &str) -> Register {
        self.lower_class(class, false, Some(name))
    }

    pub(super) fn class_declaration(&mut self, class: &Class<'_>) {
        self.lower_class(class, true, None);
    }

    fn lower_class(
        &mut self,
        class: &Class<'_>,
        bind_name: bool,
        inferred_name: Option<&str>,
    ) -> Register {
        // Emit before heritage/TDZ evaluation, then fill from the class's allocated
        // locals. This preserves earlier class evaluations before their slots change.
        let first_class_slot = self.locals.len();
        let clone_plan = self.open_class_clone();
        let class_binding = class.id.as_ref().map(|identifier| {
            let source = self.owner.atom(identifier.name.as_str());
            let binding = self.hidden_local(&format!(
                "{}\0quench:class-binding:{}",
                identifier.name.as_str(),
                self.function_id
            ));
            let slot = self.local_slots[&binding];
            self.emit(TDZ_INIT, 0, 0, 0, u32::from(slot));
            self.push_immutable_lexical_bindings(
                FxHashMap::from_iter([(source, binding)]),
                FxHashSet::from_iter([source]),
            );
            binding
        });
        let heritage = class.heritage.as_ref().map(|heritage| {
            let outer_strict = std::mem::replace(&mut self.strict, true);
            let value = self.expression(&heritage.expression);
            self.strict = outer_strict;
            value
        });
        if let Some(heritage) = heritage {
            self.emit(CLASS_HERITAGE_CHECK, heritage, 0, 0, 0);
        }
        let super_atom = heritage.map(|_| {
            let atom = self.hidden_local(&format!("\0quench:class-super:{}", class.span.start));
            let super_binding = self.owner.atom("\0quench:super");
            self.push_lexical_bindings(FxHashMap::from_iter([(super_binding, atom)]));
            atom
        });
        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(field) if field.computed => {
                    self.hidden_local(&computed_field_key_name(field.span.start));
                }
                ClassElement::MethodDefinition(method) if method.computed => {
                    self.hidden_local(&computed_field_key_name(method.span.start));
                }
                ClassElement::AccessorProperty(accessor) if accessor.computed => {
                    self.hidden_local(&computed_field_key_name(accessor.span.start));
                }
                _ => {}
            }
            if let ClassElement::AccessorProperty(accessor) = element {
                self.owner.reserve_auto_accessor_name(accessor.span);
                if !accessor.decorators.is_empty() {
                    self.owner
                        .reject(accessor.span, "decorated class accessors are unsupported");
                }
            }
        }
        let mut method_home_atoms = FxHashMap::default();
        for element in &class.body.body {
            if let ClassElement::MethodDefinition(method) = element {
                let atom = self.hidden_local(&format!("{}{}", crate::bytecode::HOME_BINDING_PREFIX, method.span.start));
                method_home_atoms.insert(method.span.start, atom);
            }
        }
        let fields: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::PropertyDefinition(field) => {
                    Some(ClassField::Property(field.as_ref()))
                }
                ClassElement::AccessorProperty(accessor) => {
                    let backing = self.owner.private_name_atom(accessor.span);
                    Some(ClassField::AutoAccessor { accessor, backing })
                }
                _ => None,
            })
            .collect();
        let instance_fields: Vec<_> = fields
            .iter()
            .copied()
            .filter(|field| !class_field_static(*field))
            .collect();
        let instance_private_method_spans: Vec<_> = class
            .body
            .body
            .iter()
            .flat_map(|element| {
                let (name, backing) = match element {
                    ClassElement::MethodDefinition(method) if !method.r#static => (
                        matches!(&method.key, PropertyKey::PrivateIdentifier(_))
                            .then(|| method.key.span()),
                        None,
                    ),
                    ClassElement::AccessorProperty(accessor) if !accessor.r#static => (
                        matches!(&accessor.key, PropertyKey::PrivateIdentifier(_))
                            .then(|| accessor.key.span()),
                        Some(accessor.span),
                    ),
                    _ => (None, None),
                };
                name.into_iter().chain(backing)
            })
            .collect();
        let mut installed_private_names = FxHashSet::default();
        let instance_private_methods: Vec<_> = instance_private_method_spans
            .iter()
            .map(|span| self.owner.private_name_atom(*span))
            .filter(|atom| installed_private_names.insert(*atom))
            .collect();
        let constructor = class.body.body.iter().find_map(|element| match element {
            ClassElement::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Constructor =>
            {
                Some((method, class.span))
            }
            _ => None,
        });
        let constructor_home_atom = constructor
            .map(|(method, _)| method_home_atoms[&method.span.start])
            .unwrap_or_else(|| {
                self.hidden_local(&format!(
                    "{}implicit-constructor:{}",
                    crate::bytecode::HOME_BINDING_PREFIX,
                    class.span.start
                ))
            });
        let class_home_atom =
            self.hidden_local(&format!("{}class:{}", crate::bytecode::HOME_BINDING_PREFIX, class.span.start));
        let private_bindings: FxHashMap<_, _> = self.class_private_bindings(class, (class_home_atom, constructor_home_atom)).into_iter().collect();
        let has_private_bindings = !private_bindings.is_empty();
        if has_private_bindings {
            self.push_private_bindings(private_bindings);
        }
        for field in &fields {
            if class_field_initializer(*field).is_some() {
                self.hidden_local(&class_field_initializer_name(
                    class_field_span(*field).start,
                ));
            }
        }
        self.fill_class_clone(clone_plan, first_class_slot);
        let scopes = self.capture_scopes();
        for field in &fields {
            let Some(expression) = class_field_initializer(*field) else {
                continue;
            };
            let id = self.owner.compile_function(
                None,
                &[],
                FunctionBody::Expression(expression),
                &scopes,
                Some(self.function_id),
                FunctionOptions {
                    class_initializer: true,
                    non_constructible: true,
                    prototype_unobservable: false,
                    super_home: true,
                    super_home_atom: Some(if class_field_static(*field) {
                        class_home_atom
                    } else {
                        constructor_home_atom
                    }),
                    super_static: class_field_static(*field),
                    strict: true,
                    with_depth: self.with_depth,
                    ..FunctionOptions::default()
                },
            );
            let initializer = self.reg();
            self.emit(CLOSURE_MAKER, initializer, 0, 0, id);
            let atom = self.owner.atom(&class_field_initializer_name(
                class_field_span(*field).start,
            ));
            self.store_atom(atom, initializer);
        }
        let implicit_super = constructor.is_none() && heritage.is_some();
        let constructor_id = constructor
            .map(|(method, source_span)| {
                self.owner.compile_class_method(
                    method,
                    Some(source_span),
                    &scopes,
                    Some(self.function_id),
                    heritage.is_none().then_some(instance_fields.as_slice()),
                    heritage
                        .is_none()
                        .then_some(instance_private_methods.as_slice()),
                    heritage.is_some(),
                    method_home_atoms[&method.span.start],
                    self.with_depth,
                )
            })
            .unwrap_or_else(|| {
                let params = if implicit_super {
                    vec!["\0quench:derived-args".to_owned()]
                } else {
                    vec![]
                };
                self.owner.compile_function(
                    None,
                    &params,
                    FunctionBody::Statements(&[]),
                    &scopes,
                    Some(self.function_id),
                    FunctionOptions {
                        directive_completion: None,
                        defaults: None,
                        source_span: self.owner.source_span(class.span),
                        name_binding: None,
                        async_function: false,
                        generator: false,
                        class_constructor: true,
                        derived_constructor: implicit_super,
                        non_constructible: false,
                        prototype_unobservable: false,
                        class_initializer: false,
                        instance_fields: Some(&instance_fields),
                        instance_private_methods: Some(&instance_private_methods),
                        super_static: false,
                        super_home: true,
                        super_home_atom: Some(constructor_home_atom),
                        rest_override: implicit_super,
                        implicit_body: implicit_super.then_some(super::first_call::ImplicitBody::Super),
                        with_depth: self.with_depth,
                        strict: true,
                    },
                )
            });
        if heritage.is_some()
            && (!instance_fields.is_empty() || !instance_private_methods.is_empty())
        {
            let initializer = self.owner.compile_function(
                None,
                &[],
                FunctionBody::Statements(&[]),
                &scopes,
                Some(self.function_id),
                FunctionOptions {
                    instance_fields: Some(&instance_fields),
                    instance_private_methods: Some(&instance_private_methods),
                    class_initializer: true,
                    non_constructible: true,
                    prototype_unobservable: false,
                    super_home: true,
                    super_home_atom: Some(constructor_home_atom),
                    strict: true,
                    with_depth: self.with_depth,
                    ..FunctionOptions::default()
                },
            );
            // A replay of the function enclosing the class finds the constructor as the load
            // left it.
            if let Some(constructor) = self.owner.functions.get_mut(constructor_id) {
                constructor.set_instance_initializer(initializer);
            }
        }
        let class_value = self.reg();
        self.emit(CLOSURE_MAKER, class_value, 0, 0, constructor_id);
        if let Some(super_atom) = super_atom {
            self.store_atom(super_atom, class_value);
        }
        self.store_atom(class_home_atom, class_value);
        if let Some(name) = class
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str())
            .or(inferred_name)
        {
            let atom = self.owner.atom(name);
            self.emit_function_name(class_value, atom);
        }
        let prototype = self.reg();
        let prototype_atom = self.owner.atom("prototype");
        let prototype_cache = self.owner.cache_site();
        self.emit(
            FIELD_READ,
            prototype,
            FieldBase::register(class_value).0,
            prototype_cache,
            prototype_atom,
        );
        self.define_class_method(prototype, class_value, None, Some("constructor"));
        self.store_atom(constructor_home_atom, prototype);

        // Static methods/accessors exist before static fields execute. Data
        // fields acquire their brand only when their initializer completes;
        // instance members never brand the prototype merely by being declared.
        let mut initialized_static_names = FxHashSet::default();
        for element in &class.body.body {
            let (name, accessor_name) = match element {
                ClassElement::MethodDefinition(method) if method.r#static => (
                    matches!(&method.key, PropertyKey::PrivateIdentifier(_))
                        .then(|| method.key.span()),
                    None,
                ),
                ClassElement::AccessorProperty(accessor) if accessor.r#static => (
                    Some(accessor.span),
                    matches!(&accessor.key, PropertyKey::PrivateIdentifier(_))
                        .then(|| accessor.key.span()),
                ),
                _ => continue,
            };
            for name in name.into_iter().chain(accessor_name) {
                let atom = self.owner.private_name_atom(name);
                if self.owner.private_name_is_overridden(atom)
                    || !initialized_static_names.insert(atom)
                {
                    continue;
                }
                self.emit(PRIVATE_NAME_MARK, 0, class_value, class_value, atom);
            }
        }

        if let Some(base) = heritage {
            let base_prototype = self.reg();
            let null = self.literal(Constant::Null);
            let is_null = self.emit_binary(
                BinaryOperator::StrictEquality as u32,
                Operand::register(base),
                Operand::register(null),
            );
            let get_constructor_prototype = self.emit(JUMP_IF_FALSE, is_null, 0, 0, 0);
            self.emit(REGISTER_MOVE, base_prototype, null, 0, 0);
            let skip_constructor_prototype = self.emit(JUMP, 0, 0, 0, 0);
            self.patch(get_constructor_prototype);
            let prototype_atom = self.owner.atom("prototype");
            let prototype_cache = self.owner.cache_site();
            self.emit(
                FIELD_READ,
                base_prototype,
                FieldBase::register(base).0,
                prototype_cache,
                prototype_atom,
            );
            self.patch(skip_constructor_prototype);
            self.set_prototype(prototype, base_prototype);
            let set_constructor_parent = self.emit(JUMP_IF_FALSE, is_null, 0, 0, 0);
            let skip_constructor_parent = self.emit(JUMP, 0, 0, 0, 0);
            self.patch(set_constructor_parent);
            self.set_prototype(class_value, base);
            self.patch(skip_constructor_parent);
        }

        for element in &class.body.body {
            let (key, span) = match element {
                ClassElement::PropertyDefinition(field) if field.computed => {
                    (field.key.as_expression(), field.span)
                }
                ClassElement::MethodDefinition(method) if method.computed => {
                    (method.key.as_expression(), method.span)
                }
                ClassElement::AccessorProperty(accessor) if accessor.computed => {
                    (accessor.key.as_expression(), accessor.span)
                }
                _ => continue,
            };
            let Some(key) = key else {
                self.owner
                    .reject(span, "computed class element key is unsupported");
                continue;
            };
            let outer_strict = std::mem::replace(&mut self.strict, true);
            let raw_key = self.expression(key);
            self.strict = outer_strict;
            let key_value = self.reg();
            self.emit(PROPERTY_KEY_CONVERT, key_value, raw_key, 0, 0);
            let key_atom = self.owner.atom(&computed_field_key_name(span.start));
            self.store_atom(key_atom, key_value);
        }

        for element in &class.body.body {
            if let ClassElement::AccessorProperty(accessor) = element {
                let target = if accessor.r#static {
                    class_value
                } else {
                    prototype
                };
                let computed_key = if accessor.computed {
                    Some(self.load_name(&computed_field_key_name(accessor.span.start)))
                } else {
                    Self::unit_key(self, &accessor.key)
                };
                let name_text = if computed_key.is_none() {
                    let Some(name) = class_field_storage_name(self.owner, &accessor.key) else {
                        self.owner
                            .reject(accessor.span, "class accessor key is unsupported");
                        continue;
                    };
                    Some(name)
                } else {
                    None
                };
                let Some((getter_id, setter_id)) = self.owner.compile_auto_accessor_methods(
                    accessor,
                    &scopes,
                    Some(self.function_id),
                    class_home_atom,
                    self.with_depth,
                ) else {
                    continue;
                };
                for (function_id, kind, prefix) in [
                    (
                        getter_id,
                        "get",
                        crate::bytecode::FUNCTION_NAME_PREFIX_GETTER,
                    ),
                    (
                        setter_id,
                        "set",
                        crate::bytecode::FUNCTION_NAME_PREFIX_SETTER,
                    ),
                ] {
                    let function = self.reg();
                    self.emit(CLOSURE_MAKER, function, 0, 0, function_id);
                    if let Some(key) = computed_key {
                        self.emit(FUNCTION_NAME_KEY_SET, function, key, 0, prefix);
                    } else if let Some(name) = name_text.as_deref() {
                        let visible_name = match &accessor.key {
                            PropertyKey::PrivateIdentifier(identifier) => {
                                format!("#{}", identifier.name)
                            }
                            _ => name.to_owned(),
                        };
                        let atom = self.owner.atom(&format!("{kind} {visible_name}"));
                        self.emit_function_name(function, atom);
                    }
                    self.define_class_accessor(
                        target,
                        function,
                        computed_key,
                        name_text.as_deref(),
                        kind,
                    );
                }
                continue;
            }
            let ClassElement::MethodDefinition(method) = element else {
                if !matches!(
                    element,
                    ClassElement::PropertyDefinition(_)
                        | ClassElement::AccessorProperty(_)
                        | ClassElement::StaticBlock(_)
                ) {
                    self.owner
                        .reject(element.span(), "class element is unsupported");
                }
                continue;
            };
            let accessor_name = match method.kind {
                MethodDefinitionKind::Get => Some("get"),
                MethodDefinitionKind::Set => Some("set"),
                MethodDefinitionKind::Method => None,
                MethodDefinitionKind::Constructor => None,
            };
            if method.kind == MethodDefinitionKind::Constructor {
                continue;
            }
            if method.kind != MethodDefinitionKind::Method && accessor_name.is_none() {
                self.owner
                    .reject(method.span, "class accessors are unsupported");
                continue;
            }
            let target = if method.r#static {
                class_value
            } else {
                prototype
            };
            let home_atom = method_home_atoms[&method.span.start];
            let computed_key = if method.computed {
                Some(self.load_name(&computed_field_key_name(method.span.start)))
            } else {
                Self::unit_key(self, &method.key)
            };
            let name_text = if computed_key.is_none() {
                let Some(name) = class_method_name(&method.key) else {
                    self.owner
                        .reject(method.span, "class method key is unsupported");
                    continue;
                };
                Some(
                    if let PropertyKey::PrivateIdentifier(identifier) = &method.key {
                        self.owner.private_name_text(identifier.span)
                    } else {
                        name
                    },
                )
            } else {
                None
            };
            self.store_atom(home_atom, target);
            let function_id = self.owner.compile_replayable_class_method(
                method,
                super::replay::ClassMethodContext {
                    element_span: element.span(),
                    home_atom,
                    scopes: scopes.clone(),
                    parent: Some(self.function_id),
                    with_depth: self.with_depth,
                },
            );
            let function = self.reg();
            self.emit(CLOSURE_MAKER, function, 0, 0, function_id);
            if let Some(accessor) = accessor_name {
                if let Some(key) = computed_key {
                    let prefix = match accessor {
                        "get" => crate::bytecode::FUNCTION_NAME_PREFIX_GETTER,
                        "set" => crate::bytecode::FUNCTION_NAME_PREFIX_SETTER,
                        _ => crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
                    };
                    self.emit(FUNCTION_NAME_KEY_SET, function, key, 0, prefix);
                }
                self.define_class_accessor(
                    target,
                    function,
                    computed_key,
                    name_text.as_deref(),
                    accessor,
                );
            } else {
                self.define_class_method(target, function, computed_key, name_text.as_deref());
            }
        }

        if let Some(binding) = class_binding {
            self.initialize_class_binding(binding, class_value);
        }

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) | ClassElement::AccessorProperty(_) => {
                    let field = match element {
                        ClassElement::PropertyDefinition(field) if field.r#static => {
                            ClassField::Property(field)
                        }
                        ClassElement::AccessorProperty(accessor) if accessor.r#static => {
                            let backing = self.owner.private_name_atom(accessor.span);
                            ClassField::AutoAccessor { accessor, backing }
                        }
                        _ => continue,
                    };
                    let span = class_field_span(field);
                    let initializer = class_field_value(field);
                    let key = class_field_key(field);
                    let computed_key = if class_field_computed(field) {
                        Some(self.load_name(&computed_field_key_name(span.start)))
                    } else {
                        Self::unit_key(self, key)
                    };
                    let name = if let ClassField::AutoAccessor { backing, .. } = field {
                        Some(backing)
                    } else if computed_key.is_none() {
                        let Some(name) = class_field_storage_name(self.owner, key) else {
                            self.owner.reject(span, CLASS_FIELD_KEY_UNSUPPORTED);
                            continue;
                        };
                        Some(self.owner.atom(&name))
                    } else {
                        None
                    };
                    let value = if let Some(constant) = class_field_constant(field) {
                        self.literal(constant)
                    } else if initializer.is_some() {
                        let atom = self.owner.atom(&class_field_initializer_name(span.start));
                        let callee = self.load_atom(atom);
                        let value = self.reg();
                        self.emit(CALL_INVOKE, value, callee, class_value, 0);
                        value
                    } else {
                        self.literal(Constant::Undefined)
                    };
                    self.set_class_field_function_name(field, value);
                    if matches!(field, ClassField::Property(field) if matches!(&field.key, PropertyKey::PrivateIdentifier(_)))
                    {
                        self.emit(
                            PRIVATE_FIELD_DEFINE,
                            value,
                            class_value,
                            class_value,
                            name.unwrap(),
                        );
                    } else if let Some(key) =
                        computed_key.filter(|_| matches!(field, ClassField::Property(_)))
                    {
                        self.emit(ELEMENT_STORE, value, class_value, key, 1);
                    } else {
                        let cache = self.owner.cache_site();
                        self.emit(OBJECT_FIELD_STORE, value, class_value, cache, name.unwrap());
                    }
                }
                ClassElement::StaticBlock(block) => {
                    let function_id = self.owner.compile_class_static_block(
                        block,
                        &scopes,
                        Some(self.function_id),
                        class_home_atom,
                        self.with_depth,
                    );
                    let function = self.reg();
                    self.emit(CLOSURE_MAKER, function, 0, 0, function_id);
                    let result = self.reg();
                    self.emit(CALL_INVOKE, result, function, class_value, 0);
                }
                _ => {}
            }
        }
        if has_private_bindings {
            self.pop_lexical_scope();
        }
        if super_atom.is_some() {
            self.pop_lexical_scope();
        }
        if class_binding.is_some() {
            self.pop_lexical_scope();
        }
        if bind_name {
            if let Some(name) = &class.id {
                let atom = self.owner.atom(name.name.as_str());
                self.initialize_atom(atom, class_value);
            } else {
                self.owner
                    .reject(class.span, "class declaration requires a name");
            }
        }
        class_value
    }

    fn initialize_class_binding(&mut self, binding: Atom, value: Register) {
        let slot = self.local_slots[&binding];
        self.emit(LOCAL_BINDING_STORE, value, 0, u16::from(true), u32::from(slot));
    }

    fn define_class_method(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
    ) {
        if let Some(key) = computed_key {
            self.emit(
                FUNCTION_NAME_KEY_SET,
                function,
                key,
                0,
                crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
            );
        }
        let key =
            computed_key.unwrap_or_else(|| self.literal(Constant::String(name.unwrap().into())));
        let mode = if name.is_some_and(|name| name.starts_with("\0quench:private:")) {
            crate::bytecode::PropertyDefinitionMode::ReadonlyMethod
        } else {
            crate::bytecode::PropertyDefinitionMode::Method
        };
        self.emit(PROPERTY_RECORD_DEFINE, function, target, key, mode.word());
    }

    fn define_class_accessor(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
        accessor: &str,
    ) {
        let key = computed_key.unwrap_or_else(|| {
            self.literal(Constant::String(name.expect("named class accessor").into()))
        });
        let mode = match accessor {
            "get" => crate::bytecode::PropertyDefinitionMode::Getter,
            "set" => crate::bytecode::PropertyDefinitionMode::Setter,
            _ => unreachable!("class accessor kind"),
        };
        self.emit(PROPERTY_RECORD_DEFINE, function, target, key, mode.word());
    }

    fn set_prototype(&mut self, target: Register, prototype: Register) {
        // The intrinsic `Object.setPrototypeOf`, whatever `Object` names here.
        let setter = self.load_name(crate::bytecode::INTRINSIC_SET_PROTOTYPE_OF_BINDING);
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(REGISTER_MOVE, target_arg, target, 0, 0);
        let prototype_arg = self.reg();
        self.emit(REGISTER_MOVE, prototype_arg, prototype, 0, 0);
        let result = self.reg();
        self.emit(
            CALL_INVOKE,
            result,
            setter,
            setter,
            crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
        );
    }

    /// The instance elements' initialization: private methods' brands, then each field's define
    /// in order. `fresh_receiver` where the receiver is the one the constructor's own [[Construct]]
    /// just allocated (a base class constructor's), which may hold the leading constant fields
    /// already (`vm::preshaped_receiver`): a run of two or more is then skipped where it does
    /// (`Op::ConstantFieldsHeld`).
    pub(super) fn emit_instance_fields(
        &mut self,
        fields: &[ClassField<'_>],
        private_methods: &[Atom],
        fresh_receiver: bool,
    ) {
        if fields.is_empty() && private_methods.is_empty() {
            return;
        }
        let this = self.reg();
        self.emit(THIS_LOAD, this, 0, 0, 0);
        let private_field = |field: &ClassField<'_>| {
            matches!(
                field,
                ClassField::Property(field)
                    if matches!(&field.key, PropertyKey::PrivateIdentifier(_))
            )
        };
        // Only private elements read the private home object.
        let home = if private_methods.is_empty() && !fields.iter().any(private_field) {
            0
        } else {
            let home_atom = self
                .super_home_atom
                .expect("class constructor carries its private home object");
            let home_name = self.owner.atoms[home_atom as usize].to_string();
            self.load_name(&home_name)
        };
        for atom in private_methods {
            self.emit(PRIVATE_NAME_MARK, 0, this, home, *atom);
        }
        // The leading run of fields whose defines evaluate no code (`bytecode::ConstantField`):
        // nothing before them observes the receiver, so its allocation may hold them already.
        let mut leading = true;
        self.constant_fields.clear();
        // The skip over the run's first `PRESHAPED_FIELDS` defines, where it is taken: the jump to
        // patch, the first define's own cache site, which the test reads (`Op::ConstantFieldsHeld`),
        // and the PC after the last define it covers.
        let mut skip: Option<(usize, u16, u32)> = None;
        for (index, field) in fields.iter().enumerate() {
            let span = class_field_span(*field);
            let initializer = class_field_value(*field);
            let backing = match field {
                ClassField::AutoAccessor { backing, .. } => Some(*backing),
                ClassField::Property(_) => None,
            };
            let computed_key = match field {
                ClassField::Property(field) if field.computed => {
                    Some(self.load_name(&computed_field_key_name(span.start)))
                }
                ClassField::Property(field) => self.unit_key(&field.key),
                ClassField::AutoAccessor { .. } => None,
            };
            let name = match field {
                ClassField::Property(field) if computed_key.is_none() => {
                    match class_field_storage_name(self.owner, &field.key) {
                        Some(name) => Some(self.owner.atom(&name)),
                        None => {
                            self.owner.reject(span, CLASS_FIELD_KEY_UNSUPPORTED);
                            continue;
                        }
                    }
                }
                _ => backing,
            };
            // Whether this field continues the run, decided before its code: a define of a plain
            // name with a constant (`leading_constant_field`).
            let constant = match name {
                Some(atom) if leading && computed_key.is_none() => self.leading_constant_field(*field, atom),
                _ => None,
            };
            if constant.is_none() {
                leading = false;
                self.end_constant_skip(&mut skip);
            } else if self.constant_fields.is_empty() && fresh_receiver {
                skip = self.open_constant_skip(this, fields.get(index + 1).copied());
            }
            let value = if let Some(initializer) = initializer.filter(|initializer| {
                observes_nothing(initializer) || self.initializer_inlines(initializer)
            }) {
                // Evaluating it here, in the constructor, is evaluating it in its initializer.
                self.expression(initializer)
            } else if initializer.is_some() {
                let atom = self.owner.atom(&class_field_initializer_name(span.start));
                let callee = self.load_atom(atom);
                let value = self.reg();
                self.emit(CALL_INVOKE, value, callee, this, 0);
                value
            } else {
                self.literal(Constant::Undefined)
            };
            self.set_class_field_function_name(*field, value);
            let private = matches!(
                field,
                ClassField::Property(field)
                    if matches!(&field.key, PropertyKey::PrivateIdentifier(_))
            );
            if private {
                let field_atom = name.unwrap();
                self.emit(PRIVATE_FIELD_DEFINE, value, this, home, field_atom);
            } else if let Some(key) = computed_key {
                self.emit(COMPUTED_FIELD_DEFINE, value, this, key, 0);
            } else if matches!(field, ClassField::AutoAccessor { .. }) {
                let field_atom = name.unwrap();
                let cache = self.owner.cache_site();
                self.emit(OBJECT_FIELD_STORE, value, this, cache, field_atom);
            } else {
                let field_atom = name.unwrap();
                // The run's first define takes the site its skip's test reads.
                let cache = match skip {
                    Some((_, site, _)) if constant.is_some() && self.constant_fields.is_empty() => site,
                    _ => self.owner.cache_site(),
                };
                self.emit(Op::DefineField, value, this, cache, field_atom);
                if let Some(constant) = constant {
                    self.constant_fields.push(constant);
                    if let Some((_, _, end)) = skip.as_mut() {
                        *end = self.code.len() as u32;
                    }
                    if self.constant_fields.len() == crate::bytecode::PRESHAPED_FIELDS {
                        // A receiver holds no more of the run than this (`vm::preshaped_receiver`).
                        self.end_constant_skip(&mut skip);
                    }
                    continue;
                }
            }
            leading = false;
            self.end_constant_skip(&mut skip);
        }
        self.end_constant_skip(&mut skip);
    }

    /// The skip over a run of constant fields that starts at the field being emitted, whose next
    /// field is `next`, emitted before the field's code where the next may extend the run (a run
    /// of one would not repay the test): the test of the receiver in `this`
    /// (`Op::ConstantFieldsHeld`) through a cache site reserved for the run's first define, and
    /// the jump past the run, patched where the run ends (`end_constant_skip`). `None` where no
    /// unique site is left, whose shared entries would prove nothing of this run.
    fn open_constant_skip(&mut self, this: Register, next: Option<ClassField<'_>>) -> Option<(usize, u16, u32)> {
        if !next.is_some_and(may_extend_constant_run) {
            return None;
        }
        let site = self.owner.cache_site();
        if site == crate::bytecode::SHARED_UNCACHED_SITE {
            return None;
        }
        let held = self.reg();
        self.emit(Op::ConstantFieldsHeld, held, this, site, 0);
        let jump = self.emit(Op::JumpTrue, held, 0, 0, 0);
        Some((jump, site, 0))
    }

    /// Ends the skip over the run of constant fields, if one is open: its jump lands after the
    /// last define the run's test covers.
    fn end_constant_skip(&mut self, skip: &mut Option<(usize, u16, u32)>) {
        if let Some((jump, _, end)) = skip.take() {
            self.patch_instruction(jump, end);
        }
    }

    /// The `ConstantField` a field whose `DefineField` of `atom` the run of leading constant
    /// fields has reached adds to it: a public, non-computed, undecorated property whose key
    /// is neither `__proto__` nor a numeric string and not already in the run, whose
    /// initializer is absent (`undefined`) or folds to a primitive constant other than a
    /// BigInt or a string with lone surrogates (`binding_time::expression`). Evaluating such an
    /// initializer runs no code and its define (CreateDataPropertyOrThrow on the fresh,
    /// extensible receiver) can neither fail nor run code; `None` ends the run.
    fn leading_constant_field(&mut self, field: ClassField<'_>, atom: Atom) -> Option<crate::bytecode::ConstantField> {
        use crate::bytecode::{ConstantField, FieldConstant};
        let ClassField::Property(property) = field else { return None };
        if property.computed || property.r#static || !property.decorators.is_empty() {
            return None;
        }
        if !matches!(property.key, PropertyKey::StaticIdentifier(_) | PropertyKey::StringLiteral(_)) {
            return None;
        }
        let key = &self.owner.atoms[atom as usize];
        if key == "__proto__" || numeric_like_key(key) || self.constant_fields.iter().any(|constant| constant.atom == atom) {
            return None;
        }
        let value = match class_field_value(field) {
            None => FieldConstant::Undefined,
            Some(initializer) => match super::binding_time::expression(initializer)? {
                Constant::Undefined => FieldConstant::Undefined,
                Constant::Null => FieldConstant::Null,
                Constant::Boolean(value) => FieldConstant::Boolean(value),
                Constant::Number(value) => FieldConstant::Number(value),
                Constant::String(text) => FieldConstant::String(self.owner.atom(&text)),
                _ => return None,
            },
        };
        Some(ConstantField { atom, value })
    }

    fn set_class_field_function_name(&mut self, field: ClassField<'_>, value: Register) {
        if !class_field_value(field).is_some_and(Self::anonymous_function_definition) {
            return;
        }
        let key = class_field_key(field);
        let computed_key = if class_field_computed(field) {
            Some(self.load_name(&computed_field_key_name(class_field_span(field).start)))
        } else {
            self.unit_key(key)
        };
        if let Some(key) = computed_key {
            self.emit(
                FUNCTION_NAME_KEY_SET,
                value,
                key,
                0,
                crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
            );
        } else {
            let name = match key {
                PropertyKey::PrivateIdentifier(identifier) => Some(format!("#{}", identifier.name)),
                _ => class_method_name(key),
            };
            if let Some(name) = name {
                let atom = self.owner.atom(&name);
                self.emit_function_name(value, atom);
            }
        }
    }

    fn unit_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        let PropertyKey::StringLiteral(value) = key else {
            return None;
        };
        match super::string::constant(value) {
            Constant::StringUnits(units) => Some(self.literal(Constant::StringUnits(units))),
            _ => None,
        }
    }
}

fn class_field_key<'a>(field: ClassField<'a>) -> &'a PropertyKey<'a> {
    match field {
        ClassField::Property(field) => &field.key,
        ClassField::AutoAccessor { accessor, .. } => &accessor.key,
    }
}

fn class_field_static(field: ClassField<'_>) -> bool {
    match field {
        ClassField::Property(field) => field.r#static,
        ClassField::AutoAccessor { accessor, .. } => accessor.r#static,
    }
}

fn class_field_computed(field: ClassField<'_>) -> bool {
    match field {
        ClassField::Property(field) => field.computed,
        ClassField::AutoAccessor { accessor, .. } => accessor.computed,
    }
}

fn class_field_span(field: ClassField<'_>) -> Span {
    match field {
        ClassField::Property(field) => field.span,
        ClassField::AutoAccessor { accessor, .. } => accessor.span,
    }
}

fn class_field_initializer_name(start: u32) -> String {
    format!("\0quench:field-initializer:{start}")
}

/// The identifiers a function initializer references, and whether it reads `new.target` or
/// calls `eval` directly, either of which ties it to its own activation.
#[derive(Default)]
struct InitializerReferences<'a> {
    names: Vec<&'a str>,
    activation_bound: bool,
}

impl<'a> oxc_ast_visit::Visit<'a> for InitializerReferences<'a> {
    fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.push(identifier.name.as_str());
    }

    fn visit_new_target(&mut self, _: &oxc_ast::ast::NewTarget) {
        self.activation_bound = true;
    }

    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if super::early::is_direct_eval_call(call) {
            self.activation_bound = true;
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }
}

impl FunctionCompiler<'_, '_> {
    /// Whether a field initializer evaluates the same in the constructor as in its
    /// initializer function. Both run with the same receiver and home object inside the same
    /// enclosing scopes, so its names resolve the same unless the constructor itself binds
    /// one, and the constructor runs no code of its own before its fields. `new.target` and
    /// direct `eval` see the activation; an initializer can name neither `arguments` nor
    /// `await` or `yield` (early errors).
    fn initializer_inlines(&mut self, initializer: &Expression<'_>) -> bool {
        use oxc_ast_visit::Visit;
        let mut references = InitializerReferences::default();
        references.visit_expression(initializer);
        // A name a function initializer binds as a parameter resolves inside it, to that
        // parameter or an inner shadow, never to the constructor's binding.
        let params = match initializer {
            Expression::ArrowFunctionExpression(arrow) => Some(&arrow.params),
            Expression::FunctionExpression(function) => Some(&function.params),
            _ => None,
        };
        let own = params
            .into_iter()
            .flat_map(|params| {
                params
                    .items
                    .iter()
                    .flat_map(|param| param.pattern.get_binding_identifiers())
                    .chain(params.rest.iter().flat_map(|rest| rest.rest.argument.get_binding_identifiers()))
            })
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>();
        !references.activation_bound
            && references.names.iter().filter(|name| !own.contains(name)).all(|name| {
                let atom = self.owner.atom(name);
                !self.binds_own_name(atom)
            })
    }
}

/// Whether a field initializer reads nothing its scope or receiver could change: a static
/// constant (`binding_time::expression`), or an array or object literal of static constants
/// with plain keys. Each evaluation still makes a fresh array or object.
fn observes_nothing(initializer: &Expression<'_>) -> bool {
    let constant = |value: &Expression<'_>| binding_time::expression(value).is_some();
    match initializer {
        Expression::ParenthesizedExpression(inner) => observes_nothing(&inner.expression),
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            element.as_expression().is_some_and(constant)
        }),
        Expression::ObjectExpression(object) => object.properties.iter().all(|property| {
            matches!(
                property,
                ObjectPropertyKind::ObjectProperty(property)
                    if property.kind == PropertyKind::Init
                        && !property.computed
                        && !property.method
                        && !property.shorthand
                        && constant(&property.value)
            )
        }),
        value => constant(value),
    }
}

pub(super) fn class_field_value<'a>(field: ClassField<'a>) -> Option<&'a Expression<'a>> {
    match field {
        ClassField::Property(field) => field.value.as_ref(),
        ClassField::AutoAccessor { accessor, .. } => accessor.value.as_ref(),
    }
}

/// The value of a field whose initializer is a constant. Evaluating one observes nothing an
/// initializer function would provide (its receiver, home object or new.target) and names
/// no function, so the definition stores the constant itself.
fn class_field_constant(field: ClassField<'_>) -> Option<Constant> {
    class_field_value(field).and_then(super::binding_time::expression)
}

/// The initializer expression a field evaluates in a function of its own: any but a constant.
fn class_field_initializer<'a>(field: ClassField<'a>) -> Option<&'a Expression<'a>> {
    class_field_value(field).filter(|_| class_field_constant(field).is_none())
}

impl Compiler<'_> {
    fn compile_auto_accessor_methods(
        &mut self,
        accessor: &AccessorProperty<'_>,
        scopes: &super::capture_scope::ScopeChain,
        parent: Option<u32>,
        home_atom: Atom,
        with_depth: u16,
    ) -> Option<(u32, u32)> {
        let backing_id = self.private_name_ids[&(accessor.span.start, accessor.span.end)];
        let prefix = " ".repeat(self.text.len().saturating_add(1));
        let source = format!(
            "{prefix}class __AutoAccessor {{ #backing; get __get() {{ return this.#backing; }} set __set(value) {{ this.#backing = value; }} }}"
        );
        let allocator = Allocator::with_capacity(source.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &source, SourceType::script()).parse();
        if parsed.stack_exhausted {
            self.errors.push(Diagnostic::stack_exhausted(self.source));
            return None;
        }
        if !parsed.diagnostics.is_empty() {
            self.reject(accessor.span, "could not lower class auto-accessor methods");
            return None;
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        let generated_private_names = private_name_ids(&semantic.semantic);
        for span in generated_private_names.keys() {
            self.private_name_ids.insert(*span, backing_id);
            self.private_name_labels.insert(*span, String::new());
        }
        let Statement::ClassDeclaration(class) = &parsed.program.body[0] else {
            self.reject(accessor.span, "auto-accessor lowering produced no class");
            return None;
        };
        let methods: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::MethodDefinition(method)
                    if matches!(
                        method.kind,
                        MethodDefinitionKind::Get | MethodDefinitionKind::Set
                    ) =>
                {
                    Some(method.as_ref())
                }
                _ => None,
            })
            .collect();
        if methods.len() != 2 || !semantic.diagnostics.is_empty() {
            self.reject(accessor.span, "auto-accessor method lowering is invalid");
            return None;
        }
        let getter = self.compile_class_method(
            methods[0], None, scopes, parent, None, None, false, home_atom, with_depth,
        );
        let setter = self.compile_class_method(
            methods[1], None, scopes, parent, None, None, false, home_atom, with_depth,
        );
        // Generated parser names are scaffolding; the actual property key owns
        // the accessor's name, including computed and private keys.
        for accessor in [getter, setter] {
            if let Some(accessor) = self.functions.get_mut(accessor) {
                accessor.name = None;
            }
        }
        Some((getter, setter))
    }

    pub(super) fn compile_class_method(
        &mut self,
        method: &MethodDefinition<'_>,
        source_span: Option<Span>,
        scopes: &super::capture_scope::ScopeChain,
        parent: Option<u32>,
        instance_fields: Option<&[ClassField<'_>]>,
        instance_private_methods: Option<&[Atom]>,
        derived_constructor: bool,
        home_atom: Atom,
        with_depth: u16,
    ) -> u32 {
        if lazy_bodies::is_released_method(method) {
            let place = lazy_bodies::MethodPlace { source_span, scopes, parent, home_atom, with_depth };
            return self.compile_released_method(method, place);
        }
        let params = FunctionCompiler::params_from_formals(&method.value.params, self);
        let body = method
            .value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = class_method_function_name(method);
        self.compile_function(
            name.as_deref(),
            &params,
            FunctionBody::Statements(body),
            scopes,
            parent,
            FunctionOptions {
                directive_completion: None,
                defaults: Some(&method.value.params),
                source_span: source_span.and_then(|span| {
                    class_method_source_span(self.text, span, method.key.span().start)
                        .and_then(|span| self.source_span(span))
                }),
                name_binding: None,
                async_function: method.value.r#async,
                generator: method.value.generator,
                class_constructor: method.kind == MethodDefinitionKind::Constructor,
                derived_constructor: method.kind == MethodDefinitionKind::Constructor
                    && derived_constructor,
                non_constructible: method.kind != MethodDefinitionKind::Constructor,
                prototype_unobservable: false,
                class_initializer: false,
                instance_fields,
                instance_private_methods,
                super_static: method.r#static,
                super_home: true,
                super_home_atom: Some(home_atom),
                rest_override: false,
                implicit_body: None,
                with_depth,
                strict: true,
            },
        )
    }

    fn compile_class_static_block(
        &mut self,
        block: &StaticBlock<'_>,
        scopes: &super::capture_scope::ScopeChain,
        parent: Option<u32>,
        home_atom: Atom,
        with_depth: u16,
    ) -> u32 {
        self.compile_function(
            None,
            &[],
            FunctionBody::Statements(&block.body),
            scopes,
            parent,
            FunctionOptions {
                directive_completion: None,
                defaults: None,
                source_span: None,
                name_binding: None,
                async_function: false,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
                prototype_unobservable: false,
                class_initializer: true,
                instance_fields: None,
                instance_private_methods: None,
                super_static: true,
                super_home: true,
                super_home_atom: Some(home_atom),
                rest_override: false,
                implicit_body: None,
                with_depth,
                strict: true,
            },
        )
    }
}

pub(super) fn class_method_source_span(mut text: &str, span: Span, key_start: u32) -> Option<Span> {
    let mut start = span.start as usize;
    let key_start = key_start as usize;
    if start > key_start || key_start > text.len() {
        return None;
    }
    text = text.get(start..key_start)?;
    let leading = skip_source_trivia(text, 0);
    start += leading;
    text = text.get(leading..)?;
    if text.starts_with("static")
        && text
            .as_bytes()
            .get("static".len())
            .is_none_or(|byte| !is_identifier_continue(*byte))
    {
        let after_static = "static".len();
        start += skip_source_trivia(text, after_static);
    }
    Some(Span::new(u32::try_from(start).ok()?, span.end))
}

fn skip_source_trivia(text: &str, mut offset: usize) -> usize {
    const COMMENT_PREFIX_LEN: usize = 2;
    const BLOCK_COMMENT_SUFFIX_LEN: usize = 2;
    let bytes = text.as_bytes();
    loop {
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            offset += 1;
        }
        match (bytes.get(offset), bytes.get(offset + 1)) {
            (Some(b'/'), Some(b'*')) => {
                let Some(end) = text
                    .get(offset + COMMENT_PREFIX_LEN..)
                    .and_then(|tail| tail.find("*/"))
                else {
                    return offset;
                };
                offset += end + COMMENT_PREFIX_LEN + BLOCK_COMMENT_SUFFIX_LEN;
            }
            (Some(b'/'), Some(b'/')) => {
                let end = text
                    .get(offset + COMMENT_PREFIX_LEN..)
                    .and_then(|tail| tail.find(['\n', '\r']))
                    .map_or(text.len(), |end| offset + COMMENT_PREFIX_LEN + end);
                offset = end;
            }
            _ => return offset,
        }
    }
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

fn class_method_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(value) if super::string::is_well_formed(value) => Some(value.value.to_string()),
        PropertyKey::NumericLiteral(value) => Some(crate::number_to_string::format(value.value)),
        PropertyKey::BigIntLiteral(value) => Some(value.value.to_string()),
        _ => None,
    }
}

fn class_field_storage_name(compiler: &mut Compiler<'_>, key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::PrivateIdentifier(identifier) => {
            Some(compiler.private_name_text(identifier.span))
        }
        _ => class_method_name(key),
    }
}

/// Whether `field` may extend a run of leading constant fields (`leading_constant_field`), judged
/// by its syntax alone: a public, non-computed, undecorated property of a plain key whose
/// initializer is absent or folds to a constant. Only whether the run's skip is emitted rests on
/// it; what the run holds is `leading_constant_field`'s, as each field is reached.
fn may_extend_constant_run(field: ClassField<'_>) -> bool {
    let ClassField::Property(property) = field else { return false };
    !property.computed
        && !property.r#static
        && property.decorators.is_empty()
        && matches!(property.key, PropertyKey::StaticIdentifier(_) | PropertyKey::StringLiteral(_))
        && property.value.as_ref().is_none_or(|value| binding_time::expression(value).is_some())
}

/// Whether a field key could name an array index or another canonical numeric string, which
/// a receiver's layout of constant fields leaves out: every one starts with a digit or `-`,
/// or is `Infinity` or `NaN`.
fn numeric_like_key(key: &str) -> bool {
    key.starts_with(|first: char| first.is_ascii_digit() || first == '-') || key == "Infinity" || key == "NaN"
}

pub(super) fn computed_field_key_name(start: u32) -> String {
    format!("\0quench:computed-field-key:{start}")
}
