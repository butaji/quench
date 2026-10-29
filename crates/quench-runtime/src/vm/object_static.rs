use super::object_descriptors::PropertyDescriptorRecord;
use super::object_keys::EnumerableOwnPropertyKind;
use super::object_symbols::PropertyCopyKind;
use super::property_key::PropertyKey;
use super::*;

/// The own keys a for-in last listed from a receiver's shape (`Vm::own_for_in_keys`), with
/// the receiver, that shape and the collection epoch: while the receiver keeps the shape,
/// in the same epoch (which no reused handle or compacted shape shares), each listed key is
/// still an own enumerable property (`Vm::for_in_own_key_listed`).
pub(super) struct ForInOwnKeys {
    object: Value,
    shape: u32,
    epoch: u64,
    keys: Vec<Value>,
}

impl<H: Host> Vm<H> {
    /// `Object.create(proto)` before its properties argument: an empty ordinary object
    /// inheriting from `proto`, which runs no guest code; `None` when `proto` is neither an
    /// object nor `null`.
    pub(super) fn object_create(&mut self, proto: Value) -> Option<Value> {
        if !proto.is_null() && self.object_data(proto).is_none() && !self.is_wasm_gc_object(proto) {
            return None;
        }
        Some(self.heap.alloc(Cell::Object(Self::empty_object(proto))))
    }

    pub(super) fn intrinsic_builtin_prototype(&mut self, native: Native) -> Value {
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, native))
            .copied()
            .unwrap_or_else(|| {
                let constructor = self.native_value(native);
                let atom = self.prototype_atom();
                self.own_property(constructor, atom)
                    .unwrap_or(self.object_proto)
            })
    }

    pub(super) fn object_prototype_define_accessor(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let receiver = self.heap.root(receiver);
        let key_input = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let accessor = self
            .heap
            .root(args.get(1).copied().unwrap_or(Value::UNDEFINED));
        let mut target_root = None;
        let outcome = (|| {
            let value = self.heap.root_value(receiver).unwrap();
            let target = self.box_object_or_type_error(p, value)?;
            let target = self.heap.root(target);
            target_root = Some(target);
            if !self.is_function(self.heap.root_value(accessor).unwrap()) {
                return Err(self.type_error(p, "accessor is not callable".into()));
            }
            let input = self.heap.root_value(key_input).unwrap();
            let key = self.to_property_key(p, input)?;
            let function = self.heap.root_value(accessor).unwrap();
            let (getter, setter) = match native {
                Native::ObjectPrototypeDefineGetter => (Some(function), None),
                Native::ObjectPrototypeDefineSetter => (None, Some(function)),
                _ => unreachable!("legacy accessor definition kind"),
            };
            let descriptor = PropertyDescriptorRecord {
                value: None,
                writable: None,
                enumerable: Some(true),
                configurable: Some(true),
                getter,
                setter,
            };
            let target = self.heap.root_value(target).unwrap();
            self.define_property_or_throw(p, target, key, descriptor)?;
            Ok(Value::UNDEFINED)
        })();
        for root in [Some(receiver), Some(key_input), Some(accessor), target_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

/// CreateDataPropertyOrThrow of a class field adds an ordinary data property and nothing
    /// else when the target is an extensible ordinary object (not a Proxy, array, arguments
    /// or other exotic object) that lacks the key and the key is no array index.
    pub(super) fn class_field_is_plain_add(&self, target: Value, key: PropertyKey) -> bool {
        let index_key = match key {
            PropertyKey::String(atom) => self.atom_class(atom).contains(AtomClass::ARRAY_INDEX),
            PropertyKey::Symbol(_) => false,
            PropertyKey::Private(_) => return false,
        };
        !index_key
            && self
                .class_field_define_target(target)
                .is_some_and(|shape| self.property_shape_slot(shape, key).is_none())
            && !self.descriptors.contains_key(&(target, key))
    }

    /// The shape of `target` when it is an extensible ordinary object whose own properties
    /// are all shape properties: wrapper, error and arguments objects carry exotic or virtual
    /// own properties. A non-index descriptor kept outside a shape belongs to one of those
    /// exotic objects, and recording one clears every field cache.
    pub(super) fn class_field_define_target(&self, target: Value) -> Option<u32> {
        let object = self.heap.ordinary_object(target)?;
        (object.is_extensible()
            && !object.is_arguments_object()
            && object.boxed_primitive().is_none()
            && object.stack_data().is_none()
            && !object.has_error_data())
        .then(|| object.shape())
    }

    pub(super) fn define_class_field(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<(), JsError> {
        if self.class_field_is_plain_add(target, key) {
            return self.create_shape_data_property(target, key, value, DEFAULT_DATA_ATTRIBUTES);
        }
        let target = self.heap.root(target);
        let value = self.heap.root(value);
        let outcome = (|| {
            let key = match key {
                PropertyKey::String(atom) => self.heap.alloc_string(self.atom_value(atom)),
                PropertyKey::Symbol(symbol) => symbol,
                PropertyKey::Private(_) => {
                    return Err(JsError::validation(
                        "private names are not public class-field keys".into(),
                    ));
                }
            };
            let target = self.heap.root_value(target).unwrap();
            let value = self.heap.root_value(value).unwrap();
            self.define_property_or_throw(p, target, key, PropertyDescriptorRecord::data(value))
        })();
        self.heap.release_root(value);
        self.heap.release_root(target);
        outcome
    }

    pub(super) fn object_prototype_to_locale_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, this)?;
        if this.as_number().is_some() || matches!(self.heap.get(this), Some(Cell::BigInt(_))) {
            let text = self.to_string(p, this)?;
            return Ok(self.heap.alloc_string(text.into()));
        }
        if let Some((crate::BuiltinBrand::NumberObject | crate::BuiltinBrand::BigIntObject, value)) = self.boxed_primitive_data(this) {
            let text = self.to_string(p, value)?;
            return Ok(self.heap.alloc_string(text.into()));
        }
        let to_string = self.intern_atom("toString");
        let method = self.get_property(p, this, to_string)?;
        if !self.is_function(method) {
            return Err(self.type_error(p, "toString is not callable".into()));
        }
        self.call_value(p, method, this, &[])
    }

    pub(super) fn object_prototype_to_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let proxy_array =
            matches!(self.heap.get(value), Some(Cell::Proxy { .. })) && self.is_array(p, value)?;
        let boxed_brand = self.boxed_primitive_data(value).and_then(|(brand, _)| {
            use crate::BuiltinBrand as B;
            match brand { B::StringObject => Some("String"), B::BooleanObject => Some("Boolean"), B::NumberObject => Some("Number"), _ => None }
        });
        let brand = if value.is_undefined() || value.is_deleted() {
            "Undefined"
        } else if value.is_null() {
            "Null"
        } else if value.as_bool().is_some() {
            "Boolean"
        } else if value.as_number().is_some() {
            "Number"
        } else if self.error_is_error(value) {
            "Error"
        } else if let Some(brand) = boxed_brand {
            brand
        } else if let Some(object) = self.heap.array(value) {
            if object.is_arguments_object() {
                "Arguments"
            } else {
                "Array"
            }
        } else if self.heap.is_function_value(value) {
            "Function"
        } else {
            match self.heap.get(value) {
                Some(Cell::Proxy { .. }) if proxy_array => "Array",
                Some(Cell::Proxy { .. }) if self.is_function(value) => "Function",
                _ if self.heap.is_string(value)  => "String",
                Some(Cell::BigInt(_) | Cell::Symbol(_)) => "Object",
                Some(Cell::Date { .. }) => "Date",
                Some(Cell::RegExp { .. }) => "RegExp",
                Some(Cell::ArrayBuffer { .. }) => "ArrayBuffer",
                Some(Cell::DataView { .. }) => "DataView",
                Some(Cell::TypedArray { kind, .. }) => match kind {
                    TypedArrayKind::Uint8 => "Uint8Array",
                    TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
                    TypedArrayKind::Uint16 => "Uint16Array",
                    TypedArrayKind::Uint32 => "Uint32Array",
                    TypedArrayKind::Int8 => "Int8Array",
                    TypedArrayKind::Int16 => "Int16Array",
                    TypedArrayKind::Int32 => "Int32Array",
                    TypedArrayKind::BigInt64 => "BigInt64Array",
                    TypedArrayKind::BigUint64 => "BigUint64Array",
                    TypedArrayKind::Float16 => "Float16Array",
                    TypedArrayKind::Float32 => "Float32Array",
                    TypedArrayKind::Float64 => "Float64Array",
                },
                Some(Cell::WeakRef { .. }) => "WeakRef",
                Some(Cell::FinalizationRegistry { .. }) => "FinalizationRegistry",
                Some(Cell::Error(_)) => "Error",
                _ => "Object",
            }
        };
        let tag = if value.is_null() || value.is_undefined() {
            None
        } else {
            self.well_known_symbols
                .get(WellKnownSymbol::ToStringTag)
                .map(|symbol| self.get_index(p, value, symbol))
                .transpose()?
        };
        let brand = tag
            .and_then(|tag| self.heap.string(tag).map(|text| text.to_string()))
            .unwrap_or_else(|| brand.to_owned());
        Ok(self
            .heap
            .alloc_string(format!("[object {brand}]").into()))
    }

    pub(super) fn descriptor_field(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
        atom: Atom,
    ) -> Result<Option<Value>, JsError> {
        if let Some(found) = self.plain_property_lookup(descriptor, PropertyKey::string(atom)) {
            return Ok(found);
        }
        let descriptor_root = self.heap.root(descriptor);
        let result = (|| {
            let descriptor = self.heap.root_value(descriptor_root).unwrap_or(descriptor);
            let key = self.atom_string(atom);
            let descriptor = self.heap.root_value(descriptor_root).unwrap_or(descriptor);
            if !self.has_property(p, descriptor, key)? {
                return Ok(None);
            }
            let descriptor = self.heap.root_value(descriptor_root).unwrap_or(descriptor);
            Ok(Some(self.get_property(p, descriptor, atom)?))
        })();
        self.heap.release_root(descriptor_root);
        result
    }

    pub(super) fn object_assign(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let args = args
            .iter()
            .map(|value| self.heap.root(*value))
            .collect::<Vec<_>>();
        let mut target_root = None;
        let outcome = (|| {
            let target = args
                .first()
                .map(|root| self.heap.root_value(*root).unwrap())
                .unwrap_or(Value::UNDEFINED);
            let target = self.box_object_or_type_error(p, target)?;
            let target = self.heap.root(target);
            target_root = Some(target);
            for source in args.iter().skip(1) {
                self.copy_enumerable_properties(
                    p,
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(*source).unwrap(),
                    &[],
                    PropertyCopyKind::Set,
                )?;
            }
            Ok(self.heap.root_value(target).unwrap())
        })();
        for root in args.into_iter().chain(target_root) {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn object_for_in_keys(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    Vec::new(),
                ),
            }));
        }
        let mut current = self.box_object(source)?;
        if let Some(keys) = self.shape_for_in_keys(current) {
            return Ok(self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    keys,
                ),
            }));
        }
        let mut visited_objects = FxHashMap::default();
        let mut visited_names = FxHashSet::default();
        let mut keys = Vec::new();
        let mut own_keys = Vec::new();
        let outcome = (|| {
            while !current.is_null() && !visited_objects.contains_key(&current) {
                let current_root = self.heap.root(current);
                visited_objects.insert(current, current_root);
                own_keys = self
                    .object_own_key_values(p, self.heap.root_value(current_root).unwrap())?
                    .into_iter()
                    .map(|key| self.heap.root(key))
                    .collect();
                for key in &own_keys {
                    let Some(name) = self.heap.string(self.heap.root_value(*key).unwrap())
                    else {
                        continue;
                    };
                    let name = name.clone();
                    if visited_names.contains(&name) {
                        continue;
                    }
                    let Some(enumerable) = self.own_property_enumerable(
                        p,
                        self.heap.root_value(current_root).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    )?
                    else {
                        continue;
                    };
                    visited_names.insert(name);
                    if enumerable {
                        keys.push(self.heap.root(self.heap.root_value(*key).unwrap()));
                    }
                }
                for root in own_keys.drain(..) {
                    self.heap.release_root(root);
                }
                current =
                    self.object_get_prototype_of(p, self.heap.root_value(current_root).unwrap())?;
            }
            let elements = keys
                .iter()
                .map(|key| self.heap.root_value(*key).unwrap())
                .collect::<Vec<_>>();
            Ok(self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    elements,
                ),
            }))
        })();
        for root in visited_objects.into_values().chain(own_keys).chain(keys) {
            self.heap.release_root(root);
        }
        outcome
    }

    /// for-in keys of an object whose prototype chain holds only objects with
    /// shape-held keys: no Proxy, array, typed array or module namespace. The
    /// keys and their order are those of the generic walk; prototypes after
    /// the last one with an enumerable key are skipped, since they can neither
    /// add a key nor shadow one. `None` sends other chains to the generic walk.
    fn shape_for_in_keys(&mut self, receiver: Value) -> Option<Vec<Value>> {
        // The common chain: the receiver's memoized enumerable keys, and no prototype with an
        // enumerable key, so no key shadows another and the order is the receiver's own.
        if let Some(keys) = self.own_for_in_keys(receiver) {
            return Some(keys);
        }
        let mut chain = Vec::new();
        let mut current = receiver;
        while !current.is_null() {
            let plain = !matches!(self.heap.get(current), Some(Cell::Proxy { .. }))
                && self.heap.array(current).is_none()
                && !matches!(self.heap.get(current), Some(Cell::TypedArray { .. }));
            let object = self.object_data(current).filter(|_| plain)?;
            if object.is_module_namespace() {
                return None;
            }
            let prototype = object.proto();
            let keys = self
                .own_named_keys(current)
                .into_iter()
                .map(|atom| {
                    let enumerable = self
                        .property_attributes(current, PropertyKey::string(atom))
                        .is_none_or(|attributes| attributes.enumerable);
                    (atom, enumerable)
                })
                .collect::<Vec<_>>();
            chain.push(keys);
            current = prototype;
        }
        let Some(last) = chain
            .iter()
            .rposition(|keys| keys.iter().any(|(_, enumerable)| *enumerable))
        else {
            return Some(Vec::new());
        };
        let mut visited = FxHashSet::default();
        let mut result = Vec::new();
        for keys in &chain[..=last] {
            for &(atom, enumerable) in keys {
                if last > 0 && !visited.insert(atom) {
                    continue;
                }
                if enumerable {
                    let name = self.atom_value(atom);
                    result.push(self.intern_dynamic_value(name));
                }
            }
        }
        Some(result)
    }

    /// `shape_for_in_keys` of a receiver none of whose prototypes has an enumerable string
    /// key, as most have not (class methods are not enumerable): a prototype then neither
    /// adds a key nor shadows one the receiver lists first, so the keys are the receiver's
    /// own enumerable ones, read from the shape memo (`shape_enumerable_slots`). `None`
    /// leaves any other chain to the walk.
    fn own_for_in_keys(&mut self, receiver: Value) -> Option<Vec<Value>> {
        let slots = self.shape_enumerable_slots(receiver)?;
        let mut prototype = self.object_data(receiver)?.proto();
        while !prototype.is_null() {
            if !self.shape_enumerable_slots(prototype)?.is_empty() {
                return None;
            }
            prototype = self.object_data(prototype)?.proto();
        }
        let keys: Vec<Value> = slots.iter().map(|&(atom, _)| self.atom_string(atom)).collect();
        let shape = self.object_data(receiver)?.shape();
        self.for_in_own_keys = Some(ForInOwnKeys {
            object: receiver,
            shape,
            epoch: self.heap.collection_epoch(),
            keys: keys.clone(),
        });
        Some(keys)
    }

    /// Whether `key` is one of the own enumerable keys a for-in over `source` listed from
    /// its shape (`ForInOwnKeys`) while `source` keeps that shape and stores no attributes
    /// beside it: then it is still an own enumerable property, as the loop's check asks,
    /// without a lookup. Any other key or object is left to the lookup.
    pub(super) fn for_in_own_key_listed(&self, source: Value, key: Value) -> bool {
        self.for_in_own_keys.as_ref().is_some_and(|listed| {
            listed.object == source
                && listed.epoch == self.heap.collection_epoch()
                && self
                    .object_data(source)
                    .is_some_and(|data| data.shape() == listed.shape && !data.has_side_descriptors())
                && listed.keys.contains(&key)
        })
    }

    pub(super) fn object_for_in_key_is_enumerable(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(false);
        }
        if self.for_in_own_key_listed(source, key) {
            return Ok(true);
        }
        let mut current = self.box_object(source)?;
        if !matches!(self.heap.get(current), Some(Cell::Proxy { .. }))
            && let Some(enumerable) = self.own_property_enumerable(p, current, key)?
        {
            return Ok(enumerable);
        }
        let key = self.heap.root(key);
        let mut visited_objects = FxHashMap::default();
        let outcome = (|| {
            while !current.is_null() && !visited_objects.contains_key(&current) {
                let current_root = self.heap.root(current);
                visited_objects.insert(current, current_root);
                if let Some(enumerable) = self.own_property_enumerable(
                    p,
                    self.heap.root_value(current_root).unwrap(),
                    self.heap.root_value(key).unwrap(),
                )? {
                    return Ok(enumerable);
                }
                current =
                    self.object_get_prototype_of(p, self.heap.root_value(current_root).unwrap())?;
            }
            Ok(false)
        })();
        for root in visited_objects.into_values().chain([key]) {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn install_object_extra(
        &mut self,
        program: &ResidualProgram,
        object: Value,
    ) -> Result<(), JsError> {
        self.install_object_prototype_methods(program, self.object_proto, None)?;
        for (name, native) in [
            ("getOwnPropertyNames", Native::ObjectGetOwnPropertyNames),
            (
                "getOwnPropertyDescriptor",
                Native::ObjectGetOwnPropertyDescriptor,
            ),
            ("getOwnPropertySymbols", Native::ObjectGetOwnPropertySymbols),
            (
                "getOwnPropertyDescriptors",
                Native::ObjectGetOwnPropertyDescriptors,
            ),
            ("defineProperty", Native::ObjectDefineProperty),
            ("defineProperties", Native::ObjectDefineProperties),
            ("values", Native::ObjectValues),
            ("entries", Native::ObjectEntries),
            ("fromEntries", Native::ObjectFromEntries),
            ("is", Native::ObjectIs),
        ] {
            self.set_builtin_named(program, object, name, native)?;
        }
        Ok(())
    }

    pub(super) fn install_object_prototype_methods(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("toLocaleString", Native::ObjectPrototypeToLocaleString),
            ("toString", Native::ObjectPrototypeToString),
            ("valueOf", Native::ObjectPrototypeValueOf),
            ("hasOwnProperty", Native::ObjectPrototypeHasOwnProperty),
            (
                "propertyIsEnumerable",
                Native::ObjectPrototypePropertyIsEnumerable,
            ),
            ("__lookupGetter__", Native::ObjectPrototypeLookupGetter),
            ("__lookupSetter__", Native::ObjectPrototypeLookupSetter),
            ("__defineGetter__", Native::ObjectPrototypeDefineGetter),
            ("__defineSetter__", Native::ObjectPrototypeDefineSetter),
            ("isPrototypeOf", Native::ObjectPrototypeIsPrototypeOf),
        ] {
            self.set_realm_builtin_named(program, prototype, name, native, realm)?;
        }
        let prototype_atom = self.intern_atom("__proto__");
        let getter = self.realm_native_value_optional(Native::ObjectPrototypeProtoGetter, realm);
        let setter = self.realm_native_value_optional(Native::ObjectPrototypeProtoSetter, realm);
        self.set_builtin_function_name(getter, "get __proto__")?;
        self.set_builtin_function_name(setter, "set __proto__")?;
        self.set_property_with_attributes(
            prototype,
            prototype_atom,
            Value::UNDEFINED,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter: Some(setter),
            },
        )?;
        Ok(())
    }

    pub(super) fn realm_native_value(
        &mut self,
        native: Native,
        realm: Value,
        current_realm: bool,
    ) -> Value {
        if current_realm {
            self.native_value(native)
        } else {
            self.native_with_realm(native, Value::NULL, realm)
        }
    }

    fn realm_native_value_optional(&mut self, native: Native, realm: Option<Value>) -> Value {
        match realm {
            Some(realm) => self.native_with_realm(native, Value::NULL, realm),
            None => self.native_value(native),
        }
    }

    pub(super) fn set_realm_builtin_named(
        &mut self,
        _program: &ResidualProgram,
        object: Value,
        name: &str,
        native: Native,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        let function = self.realm_native_value_optional(native, realm);
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn call_object_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Object => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if value.is_null() || value.is_undefined() {
                    Ok(self.object())
                } else if matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
                    self.box_bigint_object(p, value)
                } else {
                    self.box_object(value)
                }
            }
            Native::ForInKeys => {
                self.object_for_in_keys(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ForInKeyIsEnumerable => Ok(
                if self.object_for_in_key_is_enumerable(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                )? {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::ObjectGetOwnPropertyNames => {
                self.object_names(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertySymbols => {
                self.object_symbols(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyDescriptor => {
                self.object_get_own_property_descriptor(p, args)
            }
            Native::ObjectGetOwnPropertyDescriptors => {
                self.object_get_own_property_descriptors(p, args)
            }
            Native::ObjectDefineProperty => self.object_define_property(p, args),
            Native::ObjectDefineProperties => self.object_define_properties(p, args),
            Native::ObjectKeys | Native::ObjectValues | Native::ObjectEntries => {
                let kind = match native {
                    Native::ObjectKeys => EnumerableOwnPropertyKind::Key,
                    Native::ObjectValues => EnumerableOwnPropertyKind::Value,
                    Native::ObjectEntries => EnumerableOwnPropertyKind::KeyValue,
                    _ => unreachable!("enumeration native selected by dispatch"),
                };
                self.enumerable_own_properties(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                    kind,
                )
            }
            Native::ObjectFromEntries => {
                self.object_from_entries(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGroupBy => self.group_by(p, args, GroupByKind::Object),
            Native::ObjectIs => {
                let left = args.first().copied().unwrap_or(Value::UNDEFINED);
                let right = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.same_value(left, right) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ObjectCreate => {
                let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(object) = self.object_create(proto) else {
                    return Err(JsError("Object prototype is not an object".into()));
                };
                if let Some(descriptors) = args.get(1).copied()
                    && !descriptors.is_undefined()
                {
                    return self.object_define_properties(p, &[object, descriptors]);
                }
                Ok(object)
            }
            Native::ObjectAssign => self.object_assign(p, args),
            Native::ObjectGetPrototypeOf => {
                self.object_get_prototype_of(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectSetPrototypeOf => self.object_set_prototype_of(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::ObjectHasOwn => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if target.is_null() || target.is_undefined() {
                    return Err(self.type_error(p, "Object.hasOwn target is nullish".into()));
                }
                let target = self.box_object(target)?;
                let target_root = self.heap.root(target);
                let key = self.to_property_key(p, args.get(1).copied().unwrap_or(Value::UNDEFINED));
                let target = self.heap.root_value(target_root).unwrap();
                self.heap.release_root(target_root);
                let found = self.own_property_enumerable(p, target, key?)?.is_some();
                Ok(if found { Value::TRUE } else { Value::FALSE })
            }
            Native::ObjectPreventExtensions => self.object_prevent_extensions(p, args),
            Native::ObjectIsExtensible => self.object_is_extensible(p, args),
            Native::ObjectSeal => self.object_set_integrity(p, args, false),
            Native::ObjectIsSealed => self.object_is_integrity_level(p, args, false),
            Native::ObjectFreeze => self.object_set_integrity(p, args, true),
            Native::ObjectIsFrozen => self.object_is_integrity_level(p, args, true),
            _ => Err(JsError("invalid object native".into())),
        }
    }

    pub(super) fn object_from_entries(
        &mut self,
        p: &ResidualProgram,
        iterable: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, iterable)?;
        let iterator = self.get_iterator(p, iterable)?;
        let iterator = self.heap.root(iterator);
        let mut next_root = None;
        let mut result_root = None;
        let outcome = (|| {
            let next_atom = self.runtime_atoms.next;
            let receiver = self.heap.root_value(iterator).unwrap();
            let next = self.get_property(p, receiver, next_atom)?;
            let next = self.heap.root(next);
            next_root = Some(next);
            let prototype = self
                .realm
                .intrinsics
                .builtin_prototypes
                .get(&(self.realm.globals, Native::Object))
                .copied()
                .unwrap_or(self.object_proto);
            let result = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
            let result = self.heap.root(result);
            result_root = Some(result);
            loop {
                let Some(entry) = self.rooted_iterator_step_value(p, iterator, next)? else {
                    return Ok(self.heap.root_value(result).unwrap());
                };
                let entry = self.heap.root(entry);
                let mut key_root = None;
                let mut value_root = None;
                let install = (|| {
                    let object = self.heap.root_value(entry).unwrap();
                    if !self.is_object_like(object) {
                        return Err(
                            self.type_error(p, "Object.fromEntries entry is not an object".into())
                        );
                    }
                    let key = self.get_index(p, object, Value::number(0.0))?;
                    let key = self.heap.root(key);
                    key_root = Some(key);
                    let object = self.heap.root_value(entry).unwrap();
                    let value = self.get_index(p, object, Value::number(1.0))?;
                    let value = self.heap.root(value);
                    value_root = Some(value);
                    let raw_key = self.heap.root_value(key).unwrap();
                    let property_key = self.to_property_key(p, raw_key)?;
                    self.heap.update_root(key, property_key);
                    let key = match self.heap.get(property_key).cloned() {
                        Some(Cell::Symbol(_)) => PropertyKey::symbol(property_key),
                        _ if let Some(atom) = self.string_key_atom(property_key) => PropertyKey::string(atom),
                        _ => unreachable!("ToPropertyKey returns a string or symbol"),
                    };
                    let object = self.heap.root_value(result).unwrap();
                    let value = self.heap.root_value(value).unwrap();
                    // The unexposed ordinary result owns only default data properties.
                    self.set_shape_property(object, key, value)
                })();
                let install = install.map_err(|error| {
                    let receiver = self.heap.root_value(iterator).unwrap();
                    self.iterator_abrupt(p, receiver, error)
                });
                for root in [Some(entry), key_root, value_root].into_iter().flatten() {
                    self.heap.release_root(root);
                }
                install?;
            }
        })();
        for root in [Some(iterator), next_root, result_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn same_value(&self, left: Value, right: Value) -> bool {
        if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
            return (a.is_nan() && b.is_nan())
                || (a == b && (a != 0.0 || a.is_sign_negative() == b.is_sign_negative()));
        }
        if let (Some(a), Some(b)) = (self.heap.string(left), self.heap.string(right)) {
            return a == b;
        }
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => a == b,
            _ => left == right,
        }
    }

    pub(super) fn ordered_shape(&self, data: &Object) -> Vec<(Atom, usize)> {
        let mut entries = self
            .shape_entries(data.shape())
            .iter()
            .filter_map(|(key, slot)| match *key {
                PropertyKey::String(atom) => Some((atom, *slot as usize)),
                PropertyKey::Symbol(_) | PropertyKey::Private(_) => None,
            })
            .collect::<Vec<_>>();
        // Shape entries are already in insertion order; only integer indices and module
        // namespaces (code-unit order) reorder.
        if !data.is_module_namespace()
            && !entries
                .iter()
                .any(|(atom, _)| self.atom_class(*atom).contains(AtomClass::ARRAY_INDEX))
        {
            return entries;
        }
        entries.sort_by(|(left, _), (right, _)| {
            if data.is_module_namespace() {
                return self
                    .atom_value(*left)
                    .units()
                    .cmp(&self.atom_value(*right).units());
            }
            match (
                array_index(self.atom_name(*left)),
                array_index(self.atom_name(*right)),
            ) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        entries
    }

    pub(super) fn box_object(&mut self, value: Value) -> Result<Value, JsError> {
        if self.is_object_like(value) {
            return Ok(value);
        }
        if value.is_null() || value.is_undefined() {
            return Err(JsError("cannot convert nullish value to object".into()));
        }
        self.box_primitive_object(value)
    }

    pub(super) fn box_object_or_type_error(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, value)?;
        self.box_object(value)
    }

    pub(super) fn object_define_property(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.define_property_from_descriptor(p, args, PropertyDefinitionKind::Object)
    }

    pub(super) fn define_ordinary_property_key(
        &mut self,
        target: Value,
        key: PropertyKey,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let existing = match key {
            PropertyKey::String(atom) => self.own_property(target, atom),
            PropertyKey::Symbol(symbol) => self.symbol_property(target, symbol),
            PropertyKey::Private(_) => None,
        };
        let current = self
            .property_attributes(target, key)
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let is_new = existing.is_none();
        let attributes = descriptor.fold_attributes(current, is_new);
        let descriptor_value = descriptor.value;
        let descriptor_accessor = descriptor.has_accessor_fields();
        let descriptor_data = descriptor.has_data_fields();
        let current_record = (!is_new).then(|| {
            PropertyDescriptorRecord::from_attributes(existing.unwrap_or(Value::UNDEFINED), current)
        });
        let host_private = key.symbol_value().is_some_and(|symbol| self.is_host_private_symbol(symbol));
        let extensible = host_private || self.object_data(target).is_some_and(Object::is_extensible);
        if !descriptor.compatible_with(current_record, extensible, |a, b| self.same_value(a, b)) {
            return Ok(false);
        }
        if current.configurable && (descriptor_value.is_some() || descriptor_accessor || attributes != current) {
            self.reconfigure_function_name(target, key);
        }
        if descriptor_accessor {
            if is_new
                && !matches!(key, PropertyKey::Private(_))
                && let Some(shape) = self.object_data(target).map(Object::shape)
                && self.property_shape_slot(shape, key).is_none()
            {
                self.create_shape_accessor_property(target, key, attributes)?;
                return Ok(true);
            }
            if is_new {
                self.set_shape_property(target, key, Value::UNDEFINED)?;
            }
            self.set_property_attributes(target, key, attributes);
            return Ok(true);
        }
        let value = descriptor_value.or(existing).unwrap_or(Value::UNDEFINED);
        // A new data property is added with its attributes in one shape transition, as
        // `set_shape_property` and `set_property_attributes` would leave it in two.
        if is_new
            && !matches!(key, PropertyKey::Private(_))
            && let Some(data) = DataAttributes::of(attributes)
            && self.object_data(target).is_some()
        {
            self.create_shape_data_property(target, key, value, data)?;
            return Ok(true);
        }
        if is_new || descriptor_value.is_some() && (current.writable || current.configurable) {
            if (current.accessor || !current.writable && current.configurable) && descriptor_data {
                self.remove_property_attributes(target, key);
            }
            self.set_shape_property(target, key, value)?;
        }
        self.set_property_attributes(target, key, attributes);
        Ok(true)
    }
}

pub(super) fn array_index(name: &str) -> Option<u32> {
    if name.is_empty() || name != "0" && name.starts_with('0') {
        return None;
    }
    let mut index = 0_u32;
    for byte in name.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        index = index.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
    }
    (index < u32::MAX).then_some(index)
}
