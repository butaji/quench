use super::native_rows::Holder;
use super::object_descriptors::PropertyDescriptorRecord;
use super::object_keys::EnumerableOwnPropertyKind;
use super::object_symbols::PropertyCopyKind;
use super::property_key::PropertyKey;
use super::*;
mod for_in_keys;
pub(super) use for_in_keys::ForInMemo;

impl Vm {
    /// `Object.create(proto)` before its properties argument: an empty ordinary object
    /// inheriting from `proto`, which runs no guest code; `None` when `proto` is neither an
    /// object nor `null`.
    pub fn object_create(&mut self, proto: Value) -> Option<Value> {
        if !proto.is_null() && self.heap.object(proto).is_none() && !self.is_wasm_gc_object(proto) {
            return None;
        }
        Some(self.heap.alloc_object(Self::empty_object(proto)))
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
            PropertyKey::Private(..) => return false,
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

    pub fn define_class_field(
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
                PropertyKey::Private(..) => {
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
        if let Some((
            crate::BuiltinBrand::NumberObject | crate::BuiltinBrand::BigIntObject,
            value,
        )) = self.boxed_primitive_data(this)
        {
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

    pub fn object_prototype_to_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let proxy_array =
            matches!(self.heap.get(value), Some(Cell::Proxy { .. })) && self.is_array(p, value)?;
        let boxed_brand = self.boxed_primitive_data(value).and_then(|(brand, _)| {
            use crate::BuiltinBrand as B;
            match brand {
                B::StringObject => Some("String"),
                B::BooleanObject => Some("Boolean"),
                B::NumberObject => Some("Number"),
                _ => None,
            }
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
            // Object.prototype.toString's builtinTag, a separate concept from `Cell::class_name`
            // (BigInt and Symbol read as Object here). The spec's builtinTag names only Date,
            // RegExp and Error among these cells; the buffer, view and weak rows are extra.
            match self.heap.get(value) {
                Some(Cell::Proxy { .. }) if proxy_array => "Array",
                Some(Cell::Proxy { .. }) if self.is_function(value) => "Function",
                _ if self.heap.is_string(value)  => "String",
                Some(Cell::BigInt(_)) => "Object",
                _ if self.heap.is_symbol(value) => "Object",
                Some(Cell::Date { .. }) => "Date",
                Some(Cell::RegExp { .. }) => "RegExp",
                Some(Cell::ArrayBuffer { .. }) => "ArrayBuffer",
                Some(Cell::DataView { .. }) => "DataView",
                Some(Cell::TypedArray { object, .. }) => object.native.kind().name(),
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
        let tag = tag.and_then(|tag| self.heap.string(tag).map(|text| text.to_string()));
        Ok(self.builtin_brand_string(tag.as_deref().unwrap_or(brand)))
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
        // The plain body answers when every copy is an ordinary data copy; it declines before
        // any guest code could observe a difference, so only the spec slow tail is left below.
        // A fast call that already declined (`declined_fast`) is not tried again.
        if self.declined_fast.take() != Some(Native::ObjectAssign)
            && let Some(target) = self.plain_object_assign(args)
        {
            return Ok(target);
        }
        // The arguments and the target stay reachable on the call roots while sources' getters
        // and the target's setters run.
        self.with_call_roots(args.iter().copied(), |vm| {
            let target = vm.box_object_or_type_error(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            vm.with_call_roots([target], |vm| {
                for &source in args.iter().skip(1) {
                    vm.copy_enumerable_properties(p, target, source, &[], PropertyCopyKind::Set)?;
                }
                Ok(target)
            })
        })
    }

    /// The for-in key snapshot of `object` (`Vm::for_in_start`): every enumerable string key
    /// along its prototype chain, each once, in EnumerateObjectProperties order. A chain of
    /// ordinary objects is listed by `shape_for_in_keys` in one walk. Otherwise this walk takes
    /// the chain object by object: an ordinary object's names come from its shape through the
    /// same per-object step (`for_in_ordinary_names`), an exotic one (Proxy, array, typed array,
    /// String wrapper, module namespace) through its own key and [[GetOwnProperty]] reads.
    pub(super) fn object_for_in_keys(&mut self, p: &ResidualProgram, object: Value) -> Result<Value, JsError> {
        if let Some(keys) = self.shape_for_in_keys(object) {
            return Ok(keys);
        }
        let mut current = object;
        let mut visited_objects = FxHashMap::default();
        // `JsString` caches its flattened units in a `OnceCell`; `Hash` and `Eq` read only the units.
        #[allow(clippy::mutable_key_type)]
        let mut visited_names = FxHashSet::default();
        let mut keys = Vec::new();
        let mut own_keys = Vec::new();
        let outcome = (|| {
            while !current.is_null() && !visited_objects.contains_key(&current) {
                let current_root = self.heap.root(current);
                visited_objects.insert(current, current_root);
                if self.shape_lists_enumerable_keys(self.heap.root_value(current_root).unwrap()) {
                    // No guest code runs for an ordinary object, so its names are held unrooted
                    // until each is rooted as a key.
                    for (atom, enumerable) in self.for_in_ordinary_names(self.heap.root_value(current_root).unwrap()) {
                        let name = self.atom_string(atom);
                        let Some(text) = self.heap.string(name) else {
                            continue;
                        };
                        let text = text.clone();
                        if visited_names.contains(&text) {
                            continue;
                        }
                        visited_names.insert(text);
                        if enumerable {
                            keys.push(self.heap.root(name));
                        }
                    }
                } else {
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
                }
                current =
                    self.object_get_prototype_of(p, self.heap.root_value(current_root).unwrap())?;
            }
            let elements = keys
                .iter()
                .map(|key| self.heap.root_value(*key).unwrap())
                .collect::<Vec<_>>();
            Ok(self.alloc_for_in_key_array(elements.into(), self.array_proto))
        })();
        for root in visited_objects.into_values().chain(own_keys).chain(keys) {
            self.heap.release_root(root);
        }
        outcome
    }

    /// The own string names of ordinary `object` as atoms with their enumerability, in
    /// OrdinaryOwnPropertyKeys order: the per-object step of a for-in walk (`shape_for_in_keys`,
    /// `object_for_in_keys`), read from the shape's slots alone.
    fn for_in_ordinary_names(&self, object: Value) -> Vec<(Atom, bool)> {
        self.own_named_slots(object)
            .into_iter()
            .map(|(atom, slot)| (atom, self.own_slot_enumerable(object, atom, slot)))
            .collect()
    }

    /// The enumerability of `object`'s own key `atom` at `slot`, read from the slot's attributes.
    /// A slot without attributes of its own is a side descriptor, or enumerable by default.
    pub(super) fn own_slot_enumerable(&self, object: Value, atom: Atom, slot: usize) -> bool {
        let Some(data) = self.heap.object(object) else {
            return true;
        };
        match self.slot_attributes(data, slot) {
            Some(attributes) => attributes.enumerable,
            None => self
                .side_descriptor(object, PropertyKey::string(atom))
                .is_none_or(|attributes| attributes.enumerable),
        }
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

    pub(super) fn install_object_prototype_methods(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        self.install_rows_in_realm(program, prototype, Holder::ObjectPrototype, realm)?;
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
        _program: &ProgramRecord,
        object: Value,
        name: &str,
        native: Native,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        let function = self.realm_native_value_optional(native, realm);
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    /// Runs an object static through its table entry. Callers outside the table (the in-place
    /// fast path, embedding, tests) reach the statics through here.
    pub fn call_object_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.call_native_dispatch(p, native, Value::UNDEFINED, args)
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
            let result = self.heap.alloc_object(Self::empty_object(prototype));
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
                    let key = if self.heap.is_symbol(property_key) {
                        PropertyKey::symbol(property_key)
                    } else if let Some(atom) = self.string_key_atom(property_key) {
                        PropertyKey::string(atom)
                    } else {
                        unreachable!("ToPropertyKey returns a string or symbol")
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
        if let Some(same) = crate::staged::builtins::concrete_same_number(left, right) {
            return same;
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
            .filter_map(|(key, slot)| match key {
                PropertyKey::String(atom) => Some((atom, slot as usize)),
                PropertyKey::Symbol(_) | PropertyKey::Private(..) => None,
            })
            .collect::<Vec<_>>();
        if data.is_module_namespace() {
            entries.sort_by(|(left, _), (right, _)| self.atom_value(*left).order(&self.atom_value(*right)));
            return entries;
        }
        // Shape entries are already in insertion order; only integer indices reorder. Each
        // key's index is read once: indices ascend ahead of the rest, which keep their order.
        if !entries
            .iter()
            .any(|(atom, _)| self.atom_class(*atom).contains(AtomClass::ARRAY_INDEX))
        {
            return entries;
        }
        entries.sort_by_cached_key(|(atom, _)| {
            let index = array_index(self.atom_name(*atom));
            (index.is_none(), index)
        });
        entries
    }

    pub fn box_object(&mut self, value: Value) -> Result<Value, JsError> {
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
            PropertyKey::Private(..) => None,
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
        let host_private = key
            .symbol_value()
            .is_some_and(|symbol| self.is_host_private_symbol(symbol));
        let extensible =
            host_private || self.heap.object(target).is_some_and(|object| object.is_extensible());
        if !descriptor.compatible_with(current_record, extensible, |a, b| self.same_value(a, b)) {
            return Ok(false);
        }
        if current.configurable
            && (descriptor_value.is_some() || descriptor_accessor || attributes != current)
        {
            self.reconfigure_function_name(target, key);
        }
        if descriptor_accessor {
            if is_new
                && !matches!(key, PropertyKey::Private(..))
                && let Some(shape) = self.heap.object(target).map(|object| object.shape())
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
            && !matches!(key, PropertyKey::Private(..))
            && let Some(data) = DataAttributes::of(attributes)
            && self.heap.object(target).is_some()
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

/// The array index `name` holds in canonical decimal form: the property keys that name
/// array elements (an integer below 2^32 - 1, without leading zeros).
pub fn array_index(name: &str) -> Option<u32> {
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

/// The entry of `Object` and the `Object.*` statics, each a standalone function. `Object.keys`,
/// `Object.values` and `Object.entries` share one.
pub(super) fn entry(native: Native) -> Option<native_table::NativeFn> {
    let entry: native_table::NativeFn = match native {
        Native::Object => static_object,
        Native::ObjectGetOwnPropertyNames => static_own_property_names,
        Native::ObjectGetOwnPropertySymbols => static_own_property_symbols,
        Native::ObjectGetOwnPropertyDescriptor => static_own_property_descriptor,
        Native::ObjectGetOwnPropertyDescriptors => static_own_property_descriptors,
        Native::ObjectDefineProperty => static_define_property,
        Native::ObjectDefineProperties => static_define_properties,
        Native::ObjectKeys | Native::ObjectValues | Native::ObjectEntries => static_enumerable_own_properties,
        Native::ObjectFromEntries => static_from_entries,
        Native::ObjectGroupBy => static_group_by,
        Native::ObjectIs => static_is,
        Native::ObjectCreate => static_create,
        Native::ObjectAssign => static_assign,
        Native::ObjectGetPrototypeOf => static_get_prototype_of,
        Native::ObjectSetPrototypeOf => static_set_prototype_of,
        Native::ObjectHasOwn => static_has_own,
        Native::ObjectPreventExtensions => static_prevent_extensions,
        Native::ObjectIsExtensible => static_is_extensible,
        Native::ObjectSeal => static_seal,
        Native::ObjectIsSealed => static_is_sealed,
        Native::ObjectFreeze => static_freeze,
        Native::ObjectIsFrozen => static_is_frozen,
        _ => return None,
    };
    Some(entry)
}

fn first_argument(args: &[Value]) -> Value {
    args.first().copied().unwrap_or(Value::UNDEFINED)
}

fn static_object(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let value = first_argument(args);
    if value.is_null() || value.is_undefined() {
        Ok(vm.object())
    } else if matches!(vm.heap.get(value), Some(Cell::BigInt(_))) {
        vm.box_bigint_object(p, value)
    } else {
        vm.box_object(value)
    }
}

fn static_own_property_names(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_names(p, first_argument(args))
}

fn static_own_property_symbols(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_symbols(p, first_argument(args))
}

fn static_own_property_descriptor(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_get_own_property_descriptor(p, args)
}

fn static_own_property_descriptors(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_get_own_property_descriptors(p, args)
}

fn static_define_property(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_define_property(p, args)
}

fn static_define_properties(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_define_properties(p, args)
}

fn static_enumerable_own_properties(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let kind = EnumerableOwnPropertyKind::of(native).expect("Object.keys, values and entries list their kind");
    let object = first_argument(args);
    if vm.declined_fast.take() != Some(native)
        && vm.is_object_like(object)
        && let Some(listed) = vm.plain_enumerable_own_properties(object, kind)
    {
        return Ok(listed);
    }
    vm.enumerable_own_properties(p, object, kind)
}

fn static_from_entries(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_from_entries(p, first_argument(args))
}

fn static_group_by(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.group_by(p, args, GroupByKind::Object)
}

fn static_is(vm: &mut Vm, _p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let left = first_argument(args);
    let right = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    Ok(if vm.same_value(left, right) { Value::TRUE } else { Value::FALSE })
}

fn static_create(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let proto = first_argument(args);
    let Some(object) = vm.object_create(proto) else {
        return Err(JsError("Object prototype is not an object".into()));
    };
    if let Some(descriptors) = args.get(1).copied()
        && !descriptors.is_undefined()
    {
        return vm.object_define_properties(p, &[object, descriptors]);
    }
    Ok(object)
}

fn static_assign(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_assign(p, args)
}

fn static_get_prototype_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_get_prototype_of(p, first_argument(args))
}

fn static_set_prototype_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_set_prototype_of(p, first_argument(args), args.get(1).copied().unwrap_or(Value::UNDEFINED))
}

fn static_has_own(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let target = first_argument(args);
    if target.is_null() || target.is_undefined() {
        return Err(vm.type_error(p, "Object.hasOwn target is nullish".into()));
    }
    let target = vm.box_object(target)?;
    let target_root = vm.heap.root(target);
    let key = vm.to_property_key(p, args.get(1).copied().unwrap_or(Value::UNDEFINED));
    let target = vm.heap.root_value(target_root).unwrap();
    vm.heap.release_root(target_root);
    let found = vm.own_property_enumerable(p, target, key?)?.is_some();
    Ok(if found { Value::TRUE } else { Value::FALSE })
}

fn static_prevent_extensions(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_prevent_extensions(p, args)
}

fn static_is_extensible(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_is_extensible(p, args)
}

fn static_seal(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_set_integrity(p, args, false)
}

fn static_is_sealed(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_is_integrity_level(p, args, false)
}

fn static_freeze(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_set_integrity(p, args, true)
}

fn static_is_frozen(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.object_is_integrity_level(p, args, true)
}
