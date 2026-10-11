use super::property_key::PropertyKey;
use super::*;
use crate::bytecode::PropertyDefinitionMode;

pub(super) enum TypedArrayIndexKey {
    NotCanonical,
    Invalid,
    Index(usize),
}

#[derive(Clone, Copy)]
pub(super) struct PropertyDescriptorRecord {
    pub(super) value: Option<Value>,
    pub(super) writable: Option<bool>,
    pub(super) enumerable: Option<bool>,
    pub(super) configurable: Option<bool>,
    pub(super) getter: Option<Value>,
    pub(super) setter: Option<Value>,
}

impl PropertyDescriptorRecord {
    pub(super) fn data(value: Value) -> Self {
        Self {
            value: Some(value),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            getter: None,
            setter: None,
        }
    }

    pub(super) fn for_definition(mode: PropertyDefinitionMode, value: Value) -> Self {
        use PropertyDefinitionMode::*;
        match mode {
            Method | ReadonlyMethod => Self {
                writable: Some(mode == Method),
                enumerable: Some(false),
                ..Self::data(value)
            },
            Getter | Setter | EnumerableGetter | EnumerableSetter => Self {
                value: None,
                writable: None,
                enumerable: Some(matches!(mode, EnumerableGetter | EnumerableSetter)),
                configurable: Some(true),
                getter: matches!(mode, Getter | EnumerableGetter).then_some(value),
                setter: matches!(mode, Setter | EnumerableSetter).then_some(value),
            },
        }
    }

    pub(super) fn value(value: Value) -> Self {
        Self {
            value: Some(value),
            writable: None,
            enumerable: None,
            configurable: None,
            getter: None,
            setter: None,
        }
    }

    pub(super) fn compatible_with(
        self,
        current: Option<Self>,
        extensible: bool,
        same_value: impl Fn(Value, Value) -> bool,
    ) -> bool {
        let Some(current) = current else {
            return extensible;
        };
        let attributes = current.fold_attributes(DEFAULT_PROPERTY_ATTRIBUTES, true);
        if attributes.configurable {
            return true;
        }
        let next = self.fold_attributes(attributes, false);
        if self.non_configurable_conflict(attributes, next).is_some()
            || self.changes_non_configurable_accessor(attributes, &same_value)
        {
            return false;
        }
        attributes.accessor
            || attributes.writable
            || self
                .value
                .is_none_or(|value| same_value(value, current.value.unwrap_or(Value::UNDEFINED)))
    }

    pub(super) fn from_attributes(value: Value, attributes: PropertyAttributes) -> Self {
        Self {
            value: (!attributes.accessor).then_some(value),
            writable: (!attributes.accessor).then_some(attributes.writable),
            enumerable: Some(attributes.enumerable),
            configurable: Some(attributes.configurable),
            getter: attributes
                .accessor
                .then_some(attributes.getter.unwrap_or(Value::UNDEFINED)),
            setter: attributes
                .accessor
                .then_some(attributes.setter.unwrap_or(Value::UNDEFINED)),
        }
    }

    pub(super) fn has_accessor_fields(self) -> bool {
        self.getter.is_some() || self.setter.is_some()
    }

    pub(super) fn has_data_fields(self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    pub(super) fn fold_attributes(
        self,
        current: PropertyAttributes,
        is_new: bool,
    ) -> PropertyAttributes {
        let mut next = if is_new {
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            }
        } else {
            current
        };
        if let Some(writable) = self.writable {
            next.writable = writable;
        }
        if let Some(enumerable) = self.enumerable {
            next.enumerable = enumerable;
        }
        if let Some(configurable) = self.configurable {
            next.configurable = configurable;
        }
        if self.has_accessor_fields() {
            next.accessor = true;
            next.writable = false;
            next.getter = self
                .getter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.getter))
                .flatten();
            next.setter = self
                .setter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.setter))
                .flatten();
        } else if self.has_data_fields() {
            next.accessor = false;
            next.getter = None;
            next.setter = None;
        }
        next
    }

    pub(super) fn non_configurable_conflict(
        self,
        current: PropertyAttributes,
        next: PropertyAttributes,
    ) -> Option<DescriptorConflict> {
        if current.configurable {
            return None;
        }
        if next.configurable != current.configurable
            || next.enumerable != current.enumerable
            || next.writable && !current.writable
        {
            return Some(DescriptorConflict::Attributes);
        }
        let changes_kind = (self.has_accessor_fields() || self.has_data_fields())
            && self.has_accessor_fields() != current.accessor;
        changes_kind.then_some(DescriptorConflict::Kind)
    }

    pub(super) fn changes_non_configurable_accessor(
        self,
        current: PropertyAttributes,
        same_value: impl Fn(Value, Value) -> bool,
    ) -> bool {
        !current.configurable
            && current.accessor
            && [(self.getter, current.getter), (self.setter, current.setter)]
                .into_iter()
                .any(|(requested, existing)| {
                    requested.is_some_and(|requested| {
                        !(requested.is_undefined() && existing.is_none())
                            && !existing.is_some_and(|existing| same_value(existing, requested))
                    })
                })
    }
}

#[derive(Clone, Copy)]
pub(super) enum DescriptorConflict {
    Attributes,
    Kind,
}

impl<H: Host> Vm<H> {
    pub(super) fn to_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
    ) -> Result<PropertyDescriptorRecord, JsError> {
        if !self.is_object_like(descriptor) {
            return Err(self.type_error(p, "property descriptor is not an object".into()));
        }
        let descriptor_root = self.heap.root(descriptor);
        let mut value_root = None;
        let mut getter_root = None;
        let mut setter_root = None;
        let result = (|| {
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let enumerable = self
                .descriptor_field(p, descriptor, "enumerable")?
                .map(|value| self.truthy(value));
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let configurable = self
                .descriptor_field(p, descriptor, "configurable")?
                .map(|value| self.truthy(value));
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let value = self.descriptor_field(p, descriptor, "value")?;
            value_root = value.map(|value| self.heap.root(value));
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let writable = self
                .descriptor_field(p, descriptor, "writable")?
                .map(|value| self.truthy(value));
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let getter = self.descriptor_field(p, descriptor, "get")?;
            getter_root = getter.map(|value| self.heap.root(value));
            if let Some(root) = getter_root {
                let getter = self.heap.root_value(root).unwrap();
                if !getter.is_undefined() && !self.is_function(getter) {
                    return Err(self.type_error(p, "Accessor descriptor must be callable".into()));
                }
            }
            let descriptor = self.heap.root_value(descriptor_root).unwrap();
            let setter = self.descriptor_field(p, descriptor, "set")?;
            setter_root = setter.map(|value| self.heap.root(value));
            if let Some(root) = setter_root {
                let setter = self.heap.root_value(root).unwrap();
                if !setter.is_undefined() && !self.is_function(setter) {
                    return Err(self.type_error(p, "Accessor descriptor must be callable".into()));
                }
            }
            let record = PropertyDescriptorRecord {
                value: value_root.map(|root| self.heap.root_value(root).unwrap()),
                writable,
                enumerable,
                configurable,
                getter: getter_root.map(|root| self.heap.root_value(root).unwrap()),
                setter: setter_root.map(|root| self.heap.root_value(root).unwrap()),
            };
            if record.has_accessor_fields() && record.has_data_fields() {
                return Err(self.type_error(
                    p,
                    "Property descriptor cannot mix accessor and data fields".into(),
                ));
            }
            Ok(record)
        })();
        for root in [Some(descriptor_root), value_root, getter_root, setter_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        result
    }

    pub(super) fn own_descriptor_record(&mut self, descriptor: Value) -> PropertyDescriptorRecord {
        let fields = [
            "value",
            "writable",
            "enumerable",
            "configurable",
            "get",
            "set",
        ]
        .map(|name| {
            let atom = self.intern_atom(name);
            self.own_property(descriptor, atom)
        });
        let [value, writable, enumerable, configurable, getter, setter] = fields;
        PropertyDescriptorRecord {
            value,
            writable: writable.map(|v| self.truthy(v)),
            enumerable: enumerable.map(|v| self.truthy(v)),
            configurable: configurable.map(|v| self.truthy(v)),
            getter,
            setter,
        }
    }

    pub(super) fn complete_property_descriptor(
        &mut self,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<Value, JsError> {
        let accessor = descriptor.has_accessor_fields();
        self.from_property_descriptor(PropertyDescriptorRecord {
            value: (!accessor).then_some(descriptor.value.unwrap_or(Value::UNDEFINED)),
            writable: (!accessor).then_some(descriptor.writable.unwrap_or(false)),
            enumerable: Some(descriptor.enumerable.unwrap_or(false)),
            configurable: Some(descriptor.configurable.unwrap_or(false)),
            getter: accessor.then_some(descriptor.getter.unwrap_or(Value::UNDEFINED)),
            setter: accessor.then_some(descriptor.setter.unwrap_or(Value::UNDEFINED)),
        })
    }

    pub(super) fn from_property_descriptor(
        &mut self,
        record: PropertyDescriptorRecord,
    ) -> Result<Value, JsError> {
        let record =
            super::property_definition::RootedPropertyDescriptor::new(&mut self.heap, record);
        let prototype = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, Native::Object))
            .copied()
            .unwrap_or(self.object_proto);
        let descriptor = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let descriptor = self.heap.root(descriptor);
        let outcome = (|| {
            let fields = record.resolve(&self.heap);
            for (name, field) in [
                ("value", fields.value),
                ("writable", fields.writable.map(Self::integrity_bool)),
                ("get", fields.getter),
                ("set", fields.setter),
                ("enumerable", fields.enumerable.map(Self::integrity_bool)),
                (
                    "configurable",
                    fields.configurable.map(Self::integrity_bool),
                ),
            ] {
                if let Some(field) = field {
                    let atom = self.intern_atom(name);
                    let object = self.heap.root_value(descriptor).unwrap();
                    self.set_property(object, atom, field)?;
                }
            }
            Ok(self.heap.root_value(descriptor).unwrap())
        })();
        self.heap.release_root(descriptor);
        record.release(&mut self.heap);
        outcome
    }

    fn own_data_descriptor(
        &mut self,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<Value, JsError> {
        self.property_descriptor_object(
            value,
            PropertyAttributes {
                writable,
                enumerable,
                configurable,
                accessor: false,
                getter: None,
                setter: None,
            },
        )
    }

    fn property_descriptor_object(
        &mut self,
        value: Value,
        attributes: PropertyAttributes,
    ) -> Result<Value, JsError> {
        self.from_property_descriptor(PropertyDescriptorRecord::from_attributes(value, attributes))
    }

    pub(super) fn typed_array_index_key(key: &str) -> TypedArrayIndexKey {
        if key == "-0" {
            return TypedArrayIndexKey::Invalid;
        }
        let number = match key {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => match key.parse::<f64>() {
                Ok(number) => number,
                Err(_) => return TypedArrayIndexKey::NotCanonical,
            },
        };
        if super::number::number_to_decimal(number) != key {
            return TypedArrayIndexKey::NotCanonical;
        }
        if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > MAX_SAFE_INTEGER
        {
            return TypedArrayIndexKey::Invalid;
        }
        TypedArrayIndexKey::Index(number as usize)
    }

    pub(super) fn define_typed_array_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        index: usize,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let invalid_kind = descriptor.has_accessor_fields();
        let invalid_attributes = descriptor.writable == Some(false)
            || descriptor.enumerable == Some(false)
            || descriptor.configurable == Some(false);
        if invalid_kind || invalid_attributes {
            return Ok(false);
        }
        if self
            .typed_array_length(target)
            .is_none_or(|length| index >= length)
        {
            return Ok(false);
        }
        if let Some(value) = descriptor.value
            && !self.typed_array_set(p, target, index, value)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn object_get_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target =
            self.box_object_or_type_error(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let source = self.heap.root(target);
        let input = self
            .heap
            .root(args.get(1).copied().unwrap_or(Value::UNDEFINED));
        let mut key_root = None;
        let outcome = (|| {
            let key = self.to_property_key(p, self.heap.root_value(input).unwrap())?;
            let key = self.heap.root(key);
            key_root = Some(key);
            let target = self.heap.root_value(source).unwrap();
            let key = self.heap.root_value(key).unwrap();
            if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
                return self.proxy_own_property_descriptor(p, target, key);
            }
            if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
                let Some(value) = self.symbol_property(target, key) else {
                    return Ok(Value::UNDEFINED);
                };
                let attributes = self
                    .property_attributes(target, PropertyKey::symbol(key))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                return self.property_descriptor_object(value, attributes);
            }
            let Some(Cell::String(key)) = self.heap.get(key).cloned() else {
                unreachable!("ToPropertyKey returns a string or symbol")
            };
            let atom = self.intern_js_atom(&key);
            self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;
            let target = self.heap.root_value(source).unwrap();
            if atom == self.length_atom
                && let Some(length) = self.own_array_length(target)
            {
                let length_attributes = self
                    .property_attributes(target, PropertyKey::string(self.length_atom))
                    .expect("array length has attributes");
                return self.property_descriptor_object(
                    Value::number(length as f64),
                    PropertyAttributes {
                        enumerable: false,
                        ..length_attributes
                    },
                );
            }
            if matches!(self.heap.get(target), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(key.host_string()) {
                    TypedArrayIndexKey::Index(index)
                        if self
                            .typed_array_length(target)
                            .is_some_and(|length| index < length) =>
                    {
                        let value = self
                            .typed_array_get(target, index)
                            .unwrap_or(Value::UNDEFINED);
                        return self.own_data_descriptor(value, true, true, true);
                    }
                    TypedArrayIndexKey::Invalid | TypedArrayIndexKey::Index(_) => {
                        return Ok(Value::UNDEFINED);
                    }
                    TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if let Some(index) =
                super::object_static::array_index(key.host_string()).map(|index| index as usize)
            {
                let atom = self.intern_js_atom(&key);
                let attributes = self
                    .property_attributes(target, PropertyKey::string(atom))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                if attributes.accessor {
                    return self.property_descriptor_object(Value::UNDEFINED, attributes);
                }
                let value = match self.heap.get(target) {
                    Some(cell @ Cell::Array { .. }) => cell
                        .array_elements()
                        .get(index)
                        .copied()
                        .filter(|value| !value.is_deleted()),
                    _ => None,
                }
                .or_else(|| {
                    self.heap
                        .sparse_get(target, index)
                        .filter(|value| !value.is_deleted())
                });
                if let Some(value) = value {
                    return self.property_descriptor_object(value, attributes);
                }
            }
            let value = if self
                .object_data(target)
                .is_some_and(Object::is_module_namespace)
            {
                self.module_namespace_value(p, target, atom)?
            } else {
                self.own_property(target, atom)
            };
            let Some(value) = value else {
                return Ok(Value::UNDEFINED);
            };
            let attributes = self
                .property_attributes(target, PropertyKey::string(atom))
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            let writable = self
                .object_data(target)
                .is_some_and(Object::is_module_namespace)
                || attributes.writable;
            self.property_descriptor_object(
                value,
                PropertyAttributes {
                    writable,
                    ..attributes
                },
            )
        })();
        for root in [Some(source), Some(input), key_root].into_iter().flatten() {
            self.heap.release_root(root);
        }
        outcome
    }

    fn proxy_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let Some((target, handler)) = self.proxy_parts(proxy) else {
            unreachable!("Proxy descriptor dispatch")
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let proxy = self.heap.root(proxy);
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let key = self.heap.root(key);
        let mut result_root = None;
        let mut target_descriptor_root = None;
        let outcome = (|| {
            let atom = self.intern_atom("getOwnPropertyDescriptor");
            let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), atom)?;
            if trap.is_null() || trap.is_undefined() {
                return self.object_get_own_property_descriptor(
                    p,
                    &[
                        self.heap.root_value(target).unwrap(),
                        self.heap.root_value(key).unwrap(),
                    ],
                );
            }
            if !self.is_function(trap) {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap is not callable".into(),
                ));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                ],
            )?;
            if !result.is_undefined() && !self.is_object_like(result) {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap must return an object or undefined".into(),
                ));
            }
            let result = self.heap.root(result);
            result_root = Some(result);
            let current = self.object_get_own_property_descriptor(
                p,
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                ],
            )?;
            let current = self.heap.root(current);
            target_descriptor_root = Some(current);
            if self.heap.root_value(result).unwrap().is_undefined() {
                let current = self.heap.root_value(current).unwrap();
                if current.is_undefined() {
                    return Ok(Value::UNDEFINED);
                }
                if !self.descriptor_flag(current, "configurable") {
                    return Err(self.type_error(
                        p,
                        "proxy getOwnPropertyDescriptor trap cannot hide a target property".into(),
                    ));
                }
                let extensible =
                    self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
                if !self.truthy(extensible) {
                    return Err(self.type_error(
                        p,
                        "proxy getOwnPropertyDescriptor trap cannot hide a target property".into(),
                    ));
                }
                return Ok(Value::UNDEFINED);
            }
            let extensible =
                self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
            let extensible = self.truthy(extensible);
            let record = self.to_property_descriptor(p, self.heap.root_value(result).unwrap())?;
            let normalized = self.complete_property_descriptor(record)?;
            let record = self.own_descriptor_record(normalized);
            let current = self.heap.root_value(current).unwrap();
            let current = (!current.is_undefined()).then(|| self.own_descriptor_record(current));
            if !record.compatible_with(current, extensible, |left, right| {
                self.same_value(left, right)
            }) || (record.configurable == Some(false)
                && current.is_none_or(|current| {
                    current.configurable == Some(true)
                        || (record.writable == Some(false) && current.writable == Some(true))
                }))
            {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap returned an incompatible descriptor"
                        .into(),
                ));
            }
            Ok(normalized)
        })();
        for root in [
            Some(proxy),
            Some(target),
            Some(handler),
            Some(key),
            result_root,
            target_descriptor_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn descriptor_flag(&mut self, descriptor: Value, name: &str) -> bool {
        let atom = self.intern_atom(name);
        self.own_property(descriptor, atom)
            .is_some_and(|value| self.truthy(value))
    }

    pub(super) fn object_get_own_property_descriptors(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source =
            self.box_object_or_type_error(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let source = self.heap.root(source);
        let result = self.object();
        let result = self.heap.root(result);
        let mut keys = Vec::new();
        let outcome = (|| {
            keys = self
                .object_own_key_values(p, self.heap.root_value(source).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for key in &keys {
                let descriptor = self.object_get_own_property_descriptor(
                    p,
                    &[
                        self.heap.root_value(source).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    ],
                )?;
                if descriptor.is_undefined() {
                    continue;
                }
                self.define_property_or_throw(
                    p,
                    self.heap.root_value(result).unwrap(),
                    self.heap.root_value(*key).unwrap(),
                    PropertyDescriptorRecord::data(descriptor),
                )?;
            }
            Ok(self.heap.root_value(result).unwrap())
        })();
        for root in keys.into_iter().chain([source, result]) {
            self.heap.release_root(root);
        }
        outcome
    }
}
