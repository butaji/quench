use super::property_key::PropertyKey;
use super::*;
use crate::bytecode::PropertyDefinitionMode;

/// The fields of a property descriptor object (ECMA-262 PropertyDescriptor's six).
const DESCRIPTOR_FIELDS: usize = 6;

/// Where an ordinary descriptor object of `shape`, whose prototype has `prototype_shape` and no
/// prototype of its own, finds each `descriptor_field_atoms` field: an own data property's slot,
/// or `None` where neither shape names the field. Shapes are immutable and hold their
/// attributes (dictionary shapes are left out), so the layout holds for every such object.
#[derive(Clone, Copy)]
pub(in crate::vm) struct DescriptorLayout {
    shape: u32,
    prototype_shape: u32,
    slots: [Option<u32>; DESCRIPTOR_FIELDS],
}

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

    /// Whether this descriptor gives every field of its kind other than the shared
    /// `[[Enumerable]]` and `[[Configurable]]`, so the property it defines keeps nothing of
    /// the current one but those.
    pub(super) fn replaces_kind_fields(self) -> bool {
        (self.value.is_some() && self.writable.is_some())
            || (self.getter.is_some() && self.setter.is_some())
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
    /// ToPropertyDescriptor's fields' keys, in the order it reads them.
    fn descriptor_field_atoms(&self) -> [Atom; DESCRIPTOR_FIELDS] {
        let atoms = self.runtime_atoms;
        [
            atoms.enumerable,
            atoms.configurable,
            atoms.value,
            atoms.writable,
            atoms.get,
            atoms.set,
        ]
    }

    pub(super) fn to_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
    ) -> Result<PropertyDescriptorRecord, JsError> {
        if !self.is_object_like(descriptor) {
            return Err(self.type_error(p, "property descriptor is not an object".into()));
        }
        if let Some(fields) = self.laid_out_descriptor_fields(descriptor) {
            return self.descriptor_record(p, fields);
        }
        let keys = self.descriptor_field_atoms();
        // A descriptor whose fields, present or not, are all found without running code (an
        // object literal, nearly always) is read without rooting: nothing allocates.
        let mut plain = [None; DESCRIPTOR_FIELDS];
        if keys.iter().zip(&mut plain).all(|(&key, field)| {
            self.plain_property_lookup(descriptor, PropertyKey::string(key))
                .map(|found| *field = found)
                .is_some()
        }) {
            return self.descriptor_record(p, plain);
        }
        let descriptor_root = self.heap.root(descriptor);
        let mut field_roots = [None; DESCRIPTOR_FIELDS];
        let result = (|| {
            for (index, key) in keys.into_iter().enumerate() {
                let descriptor = self.heap.root_value(descriptor_root).unwrap();
                let field = self.descriptor_field(p, descriptor, key)?;
                if key == self.runtime_atoms.get {
                    // The getter is checked before `set` is read, which may run code.
                    self.checked_accessor(p, field)?;
                }
                field_roots[index] = field.map(|value| self.heap.root(value));
            }
            let fields =
                field_roots.map(|root| root.map(|root| self.heap.root_value(root).unwrap()));
            self.descriptor_record(p, fields)
        })();
        for root in [Some(descriptor_root)]
            .into_iter()
            .chain(field_roots)
            .flatten()
        {
            self.heap.release_root(root);
        }
        result
    }

    /// The fields of `descriptor` read through its shape's `DescriptorLayout`, or `None` when
    /// it is not an ordinary object whose prototype ends the chain, or its shapes have none.
    fn laid_out_descriptor_fields(
        &mut self,
        descriptor: Value,
    ) -> Option<[Option<Value>; DESCRIPTOR_FIELDS]> {
        let plain = |object: &Object| {
            !object.is_module_namespace()
                && object.deferred_module().is_none()
                && !object.has_side_descriptors()
        };
        let data = self
            .heap
            .ordinary_object(descriptor)
            .filter(|data| plain(data))?;
        let shape = data.shape();
        let prototype = self
            .heap
            .ordinary_object(data.proto())
            .filter(|data| plain(data))?;
        if !prototype.proto().is_null() {
            return None;
        }
        let prototype_shape = prototype.shape();
        let layout = match self.descriptor_layout {
            Some(layout) if layout.shape == shape && layout.prototype_shape == prototype_shape => {
                layout
            }
            _ => {
                let layout = self.descriptor_layout_of(descriptor, shape, prototype_shape)?;
                self.descriptor_layout = Some(layout);
                layout
            }
        };
        let data = self.heap.ordinary_object(descriptor)?;
        let mut fields = [None; DESCRIPTOR_FIELDS];
        for (field, slot) in fields.iter_mut().zip(layout.slots) {
            if let Some(slot) = slot {
                *field = Some(self.heap.property_get(data, slot as usize)?);
            }
        }
        Some(fields)
    }

    /// The `DescriptorLayout` of `descriptor`'s shape: `None` when a shape is a dictionary,
    /// a field is an own accessor, or the prototype names a field.
    fn descriptor_layout_of(
        &self,
        descriptor: Value,
        shape: u32,
        prototype_shape: u32,
    ) -> Option<DescriptorLayout> {
        if self.shape_is_dictionary(shape) || self.shape_is_dictionary(prototype_shape) {
            return None;
        }
        let data = self.heap.ordinary_object(descriptor)?;
        let mut slots = [None; DESCRIPTOR_FIELDS];
        for (slot, atom) in slots.iter_mut().zip(self.descriptor_field_atoms()) {
            let key = PropertyKey::string(atom);
            match self.property_shape_slot(shape, key) {
                Some(own) => {
                    if self.shape_may_have_accessors(shape)
                        && self.slot_attributes(data, own)?.accessor
                    {
                        return None;
                    }
                    *slot = Some(u32::try_from(own).ok()?);
                }
                None if self.property_shape_slot(prototype_shape, key).is_some() => return None,
                None => {}
            }
        }
        Some(DescriptorLayout {
            shape,
            prototype_shape,
            slots,
        })
    }

    /// An accessor field that is absent, undefined or callable.
    fn checked_accessor(
        &mut self,
        p: &ResidualProgram,
        field: Option<Value>,
    ) -> Result<(), JsError> {
        match field {
            Some(accessor) if !accessor.is_undefined() && !self.is_function(accessor) => {
                Err(self.type_error(p, "Accessor descriptor must be callable".into()))
            }
            _ => Ok(()),
        }
    }

    /// ToPropertyDescriptor's record of the fields it read, in `descriptor_field_atoms` order.
    fn descriptor_record(
        &mut self,
        p: &ResidualProgram,
        [enumerable, configurable, value, writable, getter, setter]: [Option<Value>;
            DESCRIPTOR_FIELDS],
    ) -> Result<PropertyDescriptorRecord, JsError> {
        self.checked_accessor(p, getter)?;
        self.checked_accessor(p, setter)?;
        let [writable, enumerable, configurable] =
            [writable, enumerable, configurable].map(|field| field.map(|value| self.truthy(value)));
        let record = PropertyDescriptorRecord {
            value,
            writable,
            enumerable,
            configurable,
            getter,
            setter,
        };
        if record.has_accessor_fields() && record.has_data_fields() {
            return Err(self.type_error(
                p,
                "Property descriptor cannot mix accessor and data fields".into(),
            ));
        }
        Ok(record)
    }

    pub(super) fn own_descriptor_record(&mut self, descriptor: Value) -> PropertyDescriptorRecord {
        let atoms = &self.runtime_atoms;
        let fields = [
            atoms.value,
            atoms.writable,
            atoms.enumerable,
            atoms.configurable,
            atoms.get,
            atoms.set,
        ]
        .map(|atom| self.own_property(descriptor, atom));
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

    /// FromPropertyDescriptor of a property's complete descriptor: a data or an accessor
    /// descriptor's four fields, in `from_property_descriptor`'s order, made in one allocation
    /// of their literal shape, as every one of them has the same keys.
    fn property_descriptor_object(
        &mut self,
        value: Value,
        attributes: PropertyAttributes,
    ) -> Result<Value, JsError> {
        let [enumerable, configurable] =
            [attributes.enumerable, attributes.configurable].map(Self::integrity_bool);
        let atoms = self.runtime_atoms;
        let (keys, values) = if attributes.accessor {
            (
                [atoms.get, atoms.set, atoms.enumerable, atoms.configurable],
                [
                    attributes.getter.unwrap_or(Value::UNDEFINED),
                    attributes.setter.unwrap_or(Value::UNDEFINED),
                    enumerable,
                    configurable,
                ],
            )
        } else {
            (
                [
                    atoms.value,
                    atoms.writable,
                    atoms.enumerable,
                    atoms.configurable,
                ],
                [
                    value,
                    Self::integrity_bool(attributes.writable),
                    enumerable,
                    configurable,
                ],
            )
        };
        let shape = match self.descriptor_shapes[usize::from(attributes.accessor)] {
            Some(shape) => shape,
            None => {
                let shape = self.literal_shape(&keys);
                self.descriptor_shapes[usize::from(attributes.accessor)] = Some(shape);
                shape
            }
        };
        let prototype = self.realm_object_prototype(self.realm.globals);
        Ok(self
            .heap
            .alloc_object_with_properties(prototype, shape, &values))
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
                if self.is_host_private_symbol(key) {
                    return Ok(Value::UNDEFINED);
                }
                return self.proxy_own_property_descriptor(p, target, key);
            }
            match self.ordinary_get_own_property(p, target, key)? {
                Some((value, attributes)) => self.property_descriptor_object(value, attributes),
                None => Ok(Value::UNDEFINED),
            }
        })();
        for root in [Some(source), Some(input), key_root].into_iter().flatten() {
            self.heap.release_root(root);
        }
        outcome
    }

    /// `target.[[GetOwnProperty]](key)` as a complete descriptor record, as a Proxy trap's
    /// invariant checks read it: a Proxy target answers through its own trap, any other
    /// target through `ordinary_get_own_property`, which makes no descriptor object. `key` is
    /// a property key; the caller keeps both rooted.
    pub(super) fn own_property_descriptor_record(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
    ) -> Result<Option<PropertyDescriptorRecord>, JsError> {
        if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
            let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
            return Ok((!descriptor.is_undefined()).then(|| self.own_descriptor_record(descriptor)));
        }
        Ok(self
            .ordinary_get_own_property(p, target, key)?
            .map(|(value, attributes)| {
                if attributes.accessor {
                    PropertyDescriptorRecord {
                        value: None,
                        writable: None,
                        enumerable: Some(attributes.enumerable),
                        configurable: Some(attributes.configurable),
                        getter: Some(attributes.getter.unwrap_or(Value::UNDEFINED)),
                        setter: Some(attributes.setter.unwrap_or(Value::UNDEFINED)),
                    }
                } else {
                    PropertyDescriptorRecord {
                        value: Some(value),
                        writable: Some(attributes.writable),
                        enumerable: Some(attributes.enumerable),
                        configurable: Some(attributes.configurable),
                        getter: None,
                        setter: None,
                    }
                }
            }))
    }

    /// [[GetOwnProperty]] of a non-Proxy object for a property key, as its value and
    /// attributes. A lazy data property reports its data descriptor, so it materializes first;
    /// its attributes alone (as enumeration reads them, `ordinary_own_property`) need no value.
    /// The caller keeps `target` and `key` rooted.
    pub(super) fn ordinary_get_own_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
    ) -> Result<Option<(Value, PropertyAttributes)>, JsError> {
        if let (Some(atom), Some(name)) =
            (self.string_key_atom(key), self.heap.string(key).cloned())
        {
            // A lazy data property is an accessor whose getter is its member: only a property
            // found as one needs its group materialized and the lookup made again.
            let found = self.ordinary_own_string_property(p, target, &name, atom)?;
            let lazy = found.is_some_and(|(_, attributes)| {
                attributes.accessor
                    && attributes
                        .getter
                        .is_some_and(|getter| self.lazy_member(getter).is_some())
            });
            if !lazy {
                return Ok(found);
            }
            self.materialize_lazy_data_property(p, target, PropertyKey::string(atom))?;
            return self.ordinary_own_string_property(p, target, &name, atom);
        }
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            self.materialize_lazy_data_property(p, target, PropertyKey::symbol(key))?;
        }
        self.ordinary_own_property(p, target, key)
    }

    /// An own property of a non-Proxy object as its value and attributes,
    /// without allocating a descriptor object. `key` is a property key.
    pub(super) fn ordinary_own_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
    ) -> Result<Option<(Value, PropertyAttributes)>, JsError> {
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            let Some(value) = self.symbol_property(target, key) else {
                return Ok(None);
            };
            let attributes = self
                .property_attributes(target, PropertyKey::symbol(key))
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            return Ok(Some((value, attributes)));
        }
        let (Some(atom), Some(key)) = (self.string_key_atom(key), self.heap.string(key).cloned())
        else {
            unreachable!("ToPropertyKey returns a string or symbol")
        };
        self.ordinary_own_string_property(p, target, &key, atom)
    }

    /// `ordinary_own_property` of a string key, interned as `atom`.
    fn ordinary_own_string_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: &crate::vm::wtf16::JsString,
        atom: Atom,
    ) -> Result<Option<(Value, PropertyAttributes)>, JsError> {
        self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;
        if atom == self.length_atom
            && let Some(length) = self.own_array_length(target)
        {
            let length_attributes = self
                .property_attributes(target, PropertyKey::string(self.length_atom))
                .expect("array length has attributes");
            return Ok(Some((
                Value::number(length as f64),
                PropertyAttributes {
                    enumerable: false,
                    ..length_attributes
                },
            )));
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
                    return Ok(Some((value, DEFAULT_PROPERTY_ATTRIBUTES)));
                }
                TypedArrayIndexKey::Invalid | TypedArrayIndexKey::Index(_) => {
                    return Ok(None);
                }
                TypedArrayIndexKey::NotCanonical => {}
            }
        }
        if let Some(index) =
            super::object_static::array_index(key.host_string()).map(|index| index as usize)
        {
            let attributes = self
                .property_attributes(target, PropertyKey::string(atom))
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            if attributes.accessor {
                return Ok(Some((Value::UNDEFINED, attributes)));
            }
            let value = self
                .heap
                .array_elements(target)
                .and_then(|elements| {
                    elements
                        .get(index)
                        .copied()
                        .filter(|value| !value.is_deleted())
                })
                .or_else(|| {
                    self.heap
                        .sparse_get(target, index)
                        .filter(|value| !value.is_deleted())
                });
            if let Some(value) = value {
                return Ok(Some((value, attributes)));
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
            return Ok(None);
        };
        let attributes = self
            .property_attributes(target, PropertyKey::string(atom))
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let writable = self
            .object_data(target)
            .is_some_and(Object::is_module_namespace)
            || attributes.writable;
        Ok(Some((
            value,
            PropertyAttributes {
                writable,
                ..attributes
            },
        )))
    }

    /// `hasOwnProperty` or `propertyIsEnumerable` (`native`) of a string key on an object
    /// whose own lookup runs no guest code: neither a Proxy (traps) nor a module namespace
    /// (a deferred one evaluates its module; a binding may be uninitialized). ToPropertyKey
    /// of a string is the string, so the answer is the general path's
    /// (`own_property_enumerable`); `None` leaves the call to it.
    pub(super) fn plain_own_property_query(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        key: Value,
    ) -> Option<Value> {
        let found = self.plain_own_attributes(p, receiver, key)?;
        let holds = found.is_some_and(|attributes| {
            native == Native::ObjectPrototypeHasOwnProperty || attributes.enumerable
        });
        Some(if holds { Value::TRUE } else { Value::FALSE })
    }

    /// The attributes of `object`'s own property of the string `key`, `Some(None)` when it
    /// has none, read without guest code: `object` is neither a Proxy nor a module namespace
    /// (`ordinary_own_string_property`). `None` for any other object or key.
    fn plain_own_attributes(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Option<Option<PropertyAttributes>> {
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. }))
            || !self
                .object_data(object)
                .is_some_and(|data| !data.is_module_namespace() && data.deferred_module().is_none())
        {
            return None;
        }
        let text = self.heap.string(key)?.clone();
        let atom = self.string_key_atom(key)?;
        let found = self
            .ordinary_own_string_property(p, object, &text, atom)
            .ok()?;
        Some(found.map(|(_, attributes)| attributes))
    }

    /// `object_for_in_key_is_enumerable` of a string key along a chain whose own lookups run
    /// no guest code (`plain_own_attributes`): the nearest own property's enumerability, or
    /// false past the chain's end. `None` leaves the walk, a Proxy's or primitive's
    /// included, to the general path.
    pub(super) fn plain_for_in_key_is_enumerable(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        key: Value,
    ) -> Option<Value> {
        let mut current = source;
        loop {
            if let Some(attributes) = self.plain_own_attributes(p, current, key)? {
                return Some(if attributes.enumerable {
                    Value::TRUE
                } else {
                    Value::FALSE
                });
            }
            current = self.object_data(current)?.proto();
            if current.is_null() {
                return Some(Value::FALSE);
            }
        }
    }

    /// Whether `key` is an own enumerable property of `target`; `None` when it
    /// is not an own property. Proxies answer through their traps.
    pub(super) fn own_property_enumerable(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
    ) -> Result<Option<bool>, JsError> {
        // A primitive's own properties are its wrapper's, as GetOwnProperty of ToObject sees
        // them: a string's indices and `length`.
        let target = self.box_object(target)?;
        if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
            let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
            return Ok((!descriptor.is_undefined())
                .then(|| self.descriptor_flag(descriptor, self.runtime_atoms.enumerable)));
        }
        // A deferred module namespace evaluates its module on first access.
        let roots = [self.heap.root(target), self.heap.root(key)];
        let found = self.ordinary_own_property(p, target, key);
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        Ok(found?.map(|(_, attributes)| attributes.enumerable))
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
                if !self.descriptor_flag(current, self.runtime_atoms.configurable) {
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

    pub(super) fn descriptor_flag(&mut self, descriptor: Value, atom: Atom) -> bool {
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
