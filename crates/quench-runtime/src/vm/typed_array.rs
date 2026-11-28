use super::*;
#[allow(unused_imports)] use crate::staged::glue_prelude::*;

/// Direct-mapped entries of `TypedArrayGeometryProofs`, by name and prototype (a DataView reads
/// `byteLength` and `byteOffset` through its own): the four geometry names
/// (`length`, `byteLength`, `byteOffset`, `buffer`) a typed array reads through its
/// prototypes' accessors, each under the one prototype chain (a Buffer's or a plain view's)
/// a program mostly reads it from.
const TYPED_ARRAY_GEOMETRY_ENTRIES: usize = 8;

/// That reading `atom` from a typed array whose own shape lacks it, with prototype
/// `prototype`, calls `getter`: every prototype before the holder lacks the name, which the
/// holder keeps as that accessor. Held under the cell of the links up to the holder
/// (`Vm::chain_cell`), which a change to one of them, or its death, retires.
#[derive(Clone, Copy)]
struct TypedArrayGeometryProof {
    /// The receiver's layout id (`Object::layout_id`): its own shape, which proved itself
    /// neither a dictionary nor holding the name, and its prototype, one fact compared once.
    layout: u32,
    atom: Atom,
    cell: crate::heap::ChainCell,
    /// The intrinsic accessor's native (`Vm::pure_native_getter`), fixed with the getter.
    native: Native,
}

/// The proof slot of `atom` read from a receiver of layout `layout`: a lookup and the proof
/// it finds share it.
#[inline(always)]
fn geometry_slot(atom: Atom, layout: u32) -> usize {
    const { assert!(TYPED_ARRAY_GEOMETRY_ENTRIES.is_power_of_two()) };
    ((atom as usize) ^ (layout as usize)) & (TYPED_ARRAY_GEOMETRY_ENTRIES - 1)
}

#[derive(Default)]
pub(super) struct TypedArrayGeometryProofs([std::cell::Cell<Option<TypedArrayGeometryProof>>; TYPED_ARRAY_GEOMETRY_ENTRIES]);

impl Vm {
    /// A read of a geometry name (`AtomClass::VIRTUAL_FIELD`) from typed array, DataView,
    /// Map or Set `object` that runs no guest code: the intrinsic accessor its chain resolves
    /// the name to, as a pure native read (`pure_native_getter_value`). `None` leaves the read to the
    /// general path: an own property of the name, an exotic or dictionary link, a data
    /// property or a script getter on the way.
    pub fn typed_array_geometry_read(&mut self, object: Value, atom: Atom) -> Option<Value> {
        self.typed_array_geometry_read_inline(object, atom)
    }

    /// `typed_array_geometry_read` inlined into its caller: a typed array's `length` site
    /// (`TYPED_LENGTH_SITE`) reads it once per loop test.
    #[inline(always)]
    pub fn typed_array_geometry_read_inline(&mut self, object: Value, atom: Atom) -> Option<Value> {
        let data = self.virtual_field_view(object)?;
        if data.has_side_descriptors() {
            return None;
        }
        let layout = data.layout_id();
        let native = match self.typed_array_geometry.0[geometry_slot(atom, layout)].get() {
            Some(proof) if proof.layout == layout && proof.atom == atom && proof.cell.holds() => proof.native,
            _ => {
                let (shape, prototype) = (data.shape(), data.proto());
                self.prove_geometry_getter(layout, shape, prototype, atom)?
            }
        };
        if native == Native::TypedArrayLengthGetter {
            if let Some(length) = self.typed_cached_length(object) {
                return Some(length);
            }
            // The view's entry is the length proved once: make it here, so the next read of
            // this view hits it rather than walking its buffer again.
            self.fill_typed_view(object);
            if let Some(length) = self.typed_cached_length(object) {
                return Some(length);
            }
        }
        self.pure_native_value(native, object)
    }

    /// The native getter `typed_array_geometry_read` reads `atom` through from a receiver of
    /// layout `layout`, own shape `shape` and prototype `prototype`, proved and recorded under
    /// the cell of its chain to the holder; `None` where the shape is a dictionary or holds the
    /// name, or no intrinsic native accessor answers.
    #[inline(never)]
    fn prove_geometry_getter(&mut self, layout: u32, shape: u32, prototype: Value, atom: Atom) -> Option<Native> {
        if self.shape_is_dictionary(shape) || self.shape_slot(shape, atom).is_some() {
            return None;
        }
        let (getter, depth) = self.typed_array_geometry_getter(prototype, atom)?;
        let native = self.pure_native_getter(getter)?;
        // Every link up to the holder becomes a prototype first, so a later change to one
        // retires the proof's cell; marking one that was no prototype yet changes no layout.
        if let Some(cell) = u8::try_from(depth).ok().and_then(|depth| self.chain_cell(prototype, depth)) {
            self.typed_array_geometry.0[geometry_slot(atom, layout)].set(Some(TypedArrayGeometryProof { layout, atom, cell, native }));
        }
        Some(native)
    }

    /// The getter of the accessor `atom` names first along the ordinary prototypes from
    /// `prototype`, where no earlier link has the name, and the number of links to it.
    fn typed_array_geometry_getter(&self, prototype: Value, atom: Atom) -> Option<(Value, usize)> {
        let key = PropertyKey::string(atom);
        let mut link = prototype;
        for depth in 1.. {
            let data = self.heap.ordinary_view(link)?;
            if data.has_side_descriptors() && self.descriptors.contains_key(&(link, key)) {
                return None;
            }
            // A dictionary link proves the name absent as any shape does: adding it moves
            // the link to a new shape. Its attributes live outside its transitions, so it
            // holds no accessor this proof keeps.
            if let Some(slot) = self.shape_slot(data.shape(), atom) {
                if self.shape_is_dictionary(data.shape()) {
                    return None;
                }
                let attributes = self.slot_attributes(data, slot)?;
                return attributes.accessor.then_some(attributes.getter).flatten().map(|getter| (getter, depth));
            }
            link = data.proto();
        }
        unreachable!("an unbounded walk returns from its loop")
    }

    /// `ArrayBuffer.isView(value)`: whether `value` is a typed array or a DataView.
    pub fn is_array_buffer_view(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::TypedArray { .. } | Cell::DataView { .. }))
    }

    pub(super) fn install_typed_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let uint8_array = self.native_value(Native::Uint8Array);
        let typed_array = self.native_value(Native::TypedArray);
        self.typed_array_protos[TypedArrayKind::Uint8 as usize] = self.object();
        self.typed_array_proto = self.object();
        let (typed_array_proto, function_proto) = (self.typed_array_proto, self.function_proto);
        self.heap.object_mut(self.typed_array_protos[TypedArrayKind::Uint8 as usize])
            .expect("Uint8Array prototype")
            .set_proto(typed_array_proto);
        self.heap.object_mut(typed_array)
            .expect("TypedArray constructor")
            .set_proto(function_proto);
        self.set_named(program, typed_array, "prototype", self.typed_array_proto)?;
        self.set_builtin_value_named(self.typed_array_proto, "constructor", typed_array)?;
        let typed_name = self.heap.alloc_string("TypedArray".into());
        self.set_named(program, typed_array, "name", typed_name)?;
        for (name, configurable) in [("length", true), ("name", true), ("prototype", false)] {
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                typed_array,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        self.heap.object_mut(uint8_array)
            .expect("Uint8Array constructor")
            .set_proto(typed_array);
        self.set_named(program, uint8_array, "prototype", self.typed_array_protos[TypedArrayKind::Uint8 as usize])?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            uint8_array,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(self.typed_array_protos[TypedArrayKind::Uint8 as usize], "constructor", uint8_array)?;
        self.set_builtin_function_name(uint8_array, "Uint8Array")?;
        self.set_named_constant(
            program,
            uint8_array,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.set_named_constant(
            program,
            self.typed_array_protos[TypedArrayKind::Uint8 as usize],
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.install_native_rows(program, self.typed_array_proto, super::native_rows::Holder::TypedArrayPrototype)?;
        self.install_native_rows(program, self.typed_array_protos[TypedArrayKind::Uint8 as usize], super::native_rows::Holder::Uint8ArrayPrototype)?;
        for (name, native) in [
            ("buffer", Native::TypedArrayBufferGetter),
            ("byteLength", Native::TypedArrayByteLengthGetter),
            ("byteOffset", Native::TypedArrayByteOffsetGetter),
            ("length", Native::TypedArrayLengthGetter),
        ] {
            let getter = self.native_with_realm(native, Value::NULL, self.realm.globals);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_named(program, self.typed_array_proto, name, Value::UNDEFINED)?;
            self.set_property_attributes(
                self.typed_array_proto,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        self.global(program, "Uint8Array", uint8_array)?;
        self.install_native_rows(program, uint8_array, super::native_rows::Holder::Uint8ArrayConstructor)?;
        self.realm.intrinsics.builtin_prototypes.insert(
            (self.realm.globals, Native::Uint8Array),
            self.typed_array_protos[TypedArrayKind::Uint8 as usize],
        );
        for &crate::heap::TypedArrayRow { kind, native, name, .. } in &crate::heap::TYPED_ARRAYS {
            if kind != TypedArrayKind::Uint8 {
                self.install_typed_array_kind(program, kind, native, name)?;
            }
        }
        Ok(())
    }

    pub(super) fn install_typed_array_iterator_symbol(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        let iterator = self.well_known_symbols[WellKnownSymbol::Iterator];
        let values_atom = self.intern_atom("values");
        let values = self.get_property(p, self.typed_array_proto, values_atom)?;
        self.set_symbol_property(self.typed_array_proto, iterator, values)?;
        self.set_property_attributes(
            self.typed_array_proto,
            PropertyKey::symbol(iterator),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn typed_array_fill_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        length: usize,
    ) -> Result<Value, JsError> {
        let source = self.heap.root(this);
        let value = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let start = args.get(1).map(|value| self.heap.root(*value));
        let end = args
            .get(2)
            .filter(|value| !value.is_undefined())
            .map(|value| self.heap.root(*value));
        let outcome = (|| {
            let this = self.heap.root_value(source).unwrap();
            let kind = self.typed_array_kind(this).unwrap();
            let value = self.heap.root_value(value).unwrap();
            let value = self.typed_array_convert_value(p, kind, value)?;
            let start = start.and_then(|root| self.heap.root_value(root));
            let start = self.typed_array_relative_index(p, start.as_ref(), length)?;
            let end = match end {
                Some(root) => {
                    let end = self.heap.root_value(root).unwrap();
                    self.typed_array_relative_index(p, Some(&end), length)?
                }
                None => length,
            };
            let this = self.heap.root_value(source).unwrap();
            self.typed_array_validate_current_write(p, this)?;
            let end = end.min(self.typed_array_length(this).unwrap_or_default());
            if !self.typed_array_fill_elements(this, start, end, &value) {
                for index in start..end {
                    self.typed_array_write_element(this, index, &value);
                }
            }
            Ok(this)
        })();
        for root in [Some(source), Some(value), start, end]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn typed_array_set_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        // The immutable-buffer proposal rejects writes before argument effects.
        if matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer {
                immutable: true,
                ..
            })
        ) {
            return Err(self.type_error(p, "typed array backing buffer is immutable".into()));
        }
        let source_root = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let target_root = self.heap.root(this);
        let offset_root = args.get(1).map(|value| self.heap.root(*value));
        let mut object_root = None;
        let outcome = (|| {
            let start = offset_root
                .and_then(|root| self.heap.root_value(root))
                .map(|value| self.to_number(p, value))
                .transpose()?
                .unwrap_or(0.0);
            let start = if start.is_nan() { 0.0 } else { start.trunc() };
            if start < 0.0 {
                return Err(self.range_error(p, "typed array set offset is invalid".into()));
            }
            let this = self.heap.root_value(target_root).unwrap();
            self.typed_array_validate_current_write(p, this)?;
            let current_length = self.typed_array_length(this).unwrap_or_default();
            let source = self.heap.root_value(source_root).unwrap();
            let typed_source = matches!(self.heap.get(source), Some(Cell::TypedArray { .. }));
            let source_object = if typed_source {
                source
            } else {
                self.box_object_or_type_error(p, source)?
            };
            object_root = Some(self.heap.root(source_object));
            let source_length = if typed_source {
                if self.typed_array_out_of_bounds(source) {
                    return Err(self.type_error(p, "typed array source is invalid".into()));
                }
                self.typed_array_length(source).unwrap_or_default()
            } else {
                self.array_like_length(p, source_object)?
            };
            if start.is_infinite() {
                return Err(self.range_error(p, "typed array set offset is invalid".into()));
            }
            let start = start as usize;
            if start > current_length || source_length > current_length - start {
                return Err(self.range_error(p, "typed array set source is too large".into()));
            }
            if typed_source {
                self.typed_array_copy_elements(p, source, this, start, source_length)?;
            } else {
                for index in 0..source_length {
                    let source_object = self.heap.root_value(object_root.unwrap()).unwrap();
                    let value = self.get_index(p, source_object, Value::number(index as f64))?;
                    let this = self.heap.root_value(target_root).unwrap();
                    self.typed_array_set(p, this, start + index, value)?;
                }
            }
            Ok(Value::UNDEFINED)
        })();
        for root in [
            Some(target_root),
            Some(source_root),
            offset_root,
            object_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn typed_array_validate_current_write(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<(), JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let buffer = *buffer;
        if self.typed_array_out_of_bounds(this)
            || self.array_buffer_detached(buffer)
            || matches!(
                self.heap.get(buffer),
                Some(Cell::ArrayBuffer {
                    immutable: true,
                    ..
                })
            )
        {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        }
        Ok(())
    }

    fn typed_array_subarray_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray {
            buffer,
            object,
            length: view_length,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let (buffer, offset, length_tracking, kind) =
            (*buffer, object.offset, view_length.is_tracking(), object.native.kind());
        let length = self.typed_array_length(this).unwrap_or_default();
        let begin = self.typed_array_relative_index(p, args.first(), length)?;
        let end_arg = args.get(1).filter(|value| !value.is_undefined());
        let end = match end_arg {
            Some(value) => self.typed_array_relative_index(p, Some(value), length)?,
            None => length,
        };
        let target_length = end.saturating_sub(begin);
        let target_offset = offset.saturating_add(begin.saturating_mul(kind.width()));
        let mut constructor_args = vec![buffer, Value::number(target_offset as f64)];
        let length_tracks = length_tracking && end_arg.is_none();
        if !length_tracks {
            constructor_args.push(Value::number(target_length as f64));
        }
        self.typed_array_species_create_with_args(p, this, &constructor_args, target_length)
    }

    pub(super) fn typed_array_sort_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let comparator = args.first().copied().filter(|value| !value.is_undefined());
        if let Some(value) = comparator
            && !self.is_function(value)
        {
            return Err(self.type_error(p, "sort comparator is not callable".into()));
        }
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let buffer = *buffer;
        let immutable = matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer {
                immutable: true,
                ..
            })
        );
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer) || immutable {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        }

        let source = self.heap.root(this);
        let comparator = comparator.map(|value| self.heap.root(value));
        let mut values = Vec::new();
        let outcome = (|| {
            let this = self.heap.root_value(source).unwrap();
            let length = self.typed_array_length(this).unwrap_or_default();
            for index in 0..length {
                let this = self.heap.root_value(source).unwrap();
                let value = self
                    .typed_array_get(this, index)
                    .unwrap_or(Value::UNDEFINED);
                values.push(self.heap.root(value));
            }
            self.typed_array_sort_values(p, &mut values, comparator)?;
            for (index, value) in values.iter().enumerate() {
                let this = self.heap.root_value(source).unwrap();
                let value = self.heap.root_value(*value).unwrap();
                self.typed_array_set(p, this, index, value)?;
            }
            Ok(self.heap.root_value(source).unwrap())
        })();
        for value in values {
            self.heap.release_root(value);
        }
        if let Some(root) = comparator {
            self.heap.release_root(root);
        }
        self.heap.release_root(source);
        outcome
    }

    fn typed_array_sort_values(
        &mut self,
        p: &ResidualProgram,
        values: &mut [RootId],
        comparator: Option<RootId>,
    ) -> Result<(), JsError> {
        let Some(comparator) = comparator else {
            return super::sort::try_stable_sort_by(values, |left, right| {
                let left = self.heap.root_value(*left).unwrap();
                let right = self.heap.root_value(*right).unwrap();
                Ok(self.typed_array_sort_compare(left, right))
            });
        };
        let comparator = self.heap.root_value(comparator).unwrap();
        self.with_prepared_callback(comparator, Value::UNDEFINED, |vm, prepared| {
            super::sort::try_stable_sort_by(values, |left, right| {
                let left = vm.heap.root_value(*left).unwrap();
                let right = vm.heap.root_value(*right).unwrap();
                vm.compare_sort_callback(p, prepared, left, right)
            })
        })
    }

    fn new_typed_array_of_kind(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        length: usize,
    ) -> Result<Value, JsError> {
        let name = typed_array_kind_name(kind);
        self.construct_typed_array_native(p, &[Value::number(length as f64)], kind, name)
    }

    fn typed_array_sort_compare(&self, left: Value, right: Value) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) =
            (self.heap.get(left), self.heap.get(right))
        {
            return left.cmp(right);
        }
        let left = left.as_number().unwrap_or(f64::NAN);
        let right = right.as_number().unwrap_or(f64::NAN);
        if left.is_nan() {
            if right.is_nan() {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        } else if right.is_nan() {
            Ordering::Less
        } else if left == 0.0 && right == 0.0 {
            match (left.is_sign_negative(), right.is_sign_negative()) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => Ordering::Equal,
            }
        } else {
            left.partial_cmp(&right).unwrap_or(Ordering::Equal)
        }
    }

    pub(super) fn typed_array_to_string_tag_native(&mut self, this: Value) -> Value {
        let Some(kind) = self.typed_array_kind(this) else {
            return Value::UNDEFINED;
        };
        let name = typed_array_kind_name(kind);
        self.heap.alloc_string(name.into())
    }

    pub(super) fn validate_typed_array_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            let method = super::native_rows::method_name(super::native_rows::Holder::TypedArrayPrototype, native);
            let message = if matches!(native, Native::Uint8ArrayJoin | Native::Uint8ArraySlice) {
                let receiver = self.typed_array_receiver_diagnostic(p, this)?;
                format!(
                    "Method %TypedArray%.prototype.{} called on incompatible receiver {receiver}",
                    method.expect("installed typed method")
                )
            } else if method.is_some() {
                "this is not a typed array.".to_owned()
            } else {
                "typed array receiver is invalid".to_owned()
            };
            return Err(self.type_error(p, message));
        };
        let buffer = *buffer;
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer) {
            let method = super::native_rows::method_name(super::native_rows::Holder::TypedArrayPrototype, native);
            let message = if let Some(method) = method {
                let state = if native == Native::Uint8ArrayKeys {
                    "detached"
                } else {
                    "detached or out-of-bounds"
                };
                format!("Cannot perform %TypedArray%.prototype.{method} on a {state} ArrayBuffer")
            } else {
                "typed array receiver is invalid".to_owned()
            };
            return Err(self.type_error(p, message));
        }
        Ok(buffer)
    }

    fn typed_array_receiver_diagnostic(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<String, JsError> {
        let receiver = if this.is_null() {
            "null".to_owned()
        } else if this.is_undefined() {
            "undefined".to_owned()
        } else if self.is_function(this) {
            self.function_source_value(this)
                .and_then(|source| self.heap.string(source).map(|s| s.host_string().to_owned()))
                .unwrap_or_else(|| "function () { [native code] }".to_owned())
        } else if this.direct_array_index().is_some() {
            "[object Array]".to_owned()
        } else {
            match self.heap.get(this) {
                Some(Cell::DataView { .. }) => "#<DataView>".to_owned(),
                Some(Cell::Date { .. }) => "[object Date]".to_owned(),
                _ if let Some(description) = self.heap.symbol_data(this) => {
                    format!("Symbol({})", description.as_deref().unwrap_or(""))
                }
                Some(Cell::BigInt(_)) => self.to_string(p, this)?,
                _ if self.heap.is_string(this) => self.to_string(p, this)?,
                _ if this.as_number().is_some() || this.as_bool().is_some() => {
                    self.to_string(p, this)?
                }
                _ => self.diagnostic_object_receiver(this),
            }
        };
        Ok(receiver)
    }

    pub(super) fn typed_array_getter_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.typed_array_getter_value(native, this) {
            return Ok(value);
        }
        {
            let property = match native {
                Native::TypedArrayBufferGetter => "buffer",
                Native::TypedArrayByteLengthGetter => "byteLength",
                Native::TypedArrayByteOffsetGetter => "byteOffset",
                Native::TypedArrayLengthGetter => "length",
                _ => unreachable!(),
            };
            let receiver = self.typed_array_receiver_diagnostic(p, this)?;
            Err(self.type_error(p, format!("Method get TypedArray.prototype.{property} called on incompatible receiver {receiver}")))
        }
    }

    /// A `%TypedArray%.prototype` accessor's value for a typed array `this`; `None` for any
    /// other receiver, which the getter rejects.
    pub(super) fn typed_array_getter_value(&self, native: Native, this: Value) -> Option<Value> {
        let Some(Cell::TypedArray {
            buffer,
            object,
            length,
            ..
        }) = self.heap.get(this)
        else {
            return None;
        };
        let (buffer, offset, length, kind) = (*buffer, object.offset, *length, object.native.kind());
        let view_length = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, detached: false, .. }) => length.within(offset, kind.width(), bytes.len()),
            _ => None,
        };
        let out_of_bounds = view_length.is_none();
        let view_length = view_length.unwrap_or(0);
        Some(match native {
            Native::TypedArrayBufferGetter => buffer,
            Native::TypedArrayByteOffsetGetter if !out_of_bounds => {
                Value::number(self.typed_array_byte_offset(this).unwrap_or_default() as f64)
            }
            Native::TypedArrayLengthGetter if !out_of_bounds => Value::length(view_length),
            Native::TypedArrayByteLengthGetter if !out_of_bounds => {
                Value::number((view_length * kind.width()) as f64)
            }
            Native::TypedArrayByteOffsetGetter
            | Native::TypedArrayLengthGetter
            | Native::TypedArrayByteLengthGetter => Value::number(0.0),
            _ => unreachable!(),
        })
    }

    fn typed_array_relative_index(
        &mut self,
        p: &ResidualProgram,
        value: Option<&Value>,
        length: usize,
    ) -> Result<usize, JsError> {
        // An absent argument reads as undefined, whose ToNumber (NaN) resolves to 0.
        value.map_or(Ok(0), |value| self.to_relative_index(p, *value, length))
    }

    pub(super) fn uint32_from_value(number: f64) -> u32 {
        crate::value::number_to_u32(number)
    }
}

/// The entry of each typed array method, typed array iterator and typed array constructor, by
/// its native. The base64 methods are the last group, in `typed_array_base64`.
pub(super) fn entry(native: Native) -> Option<native_table::NativeFn> {
    match native {
        Native::Uint8ArraySet => Some(uint8_array_set),
        Native::Uint8ArraySubarray => Some(uint8_array_subarray),
        Native::Uint8ArrayReverse => Some(uint8_array_reverse),
        Native::Uint8ArrayFill => Some(uint8_array_fill),
        Native::Uint8ArrayCopyWithin => Some(uint8_array_copy_within),
        Native::Uint8ArraySlice => Some(uint8_array_slice),
        Native::Uint8ArrayIncludes | Native::Uint8ArrayIndexOf => Some(uint8_array_search),
        Native::Uint8ArrayJoin | Native::Uint8ArrayToString => Some(uint8_array_join),
        Native::TypedArrayForEach => Some(typed_array_for_each),
        Native::TypedArrayMap => Some(typed_array_map),
        Native::TypedArrayFilter => Some(typed_array_filter),
        Native::TypedArraySome => Some(typed_array_some),
        Native::TypedArrayEvery => Some(typed_array_every),
        Native::TypedArrayFind => Some(typed_array_find),
        Native::TypedArrayFindIndex => Some(typed_array_find_index),
        Native::TypedArrayFindLast => Some(typed_array_find_last),
        Native::TypedArrayFindLastIndex => Some(typed_array_find_last_index),
        Native::TypedArrayReduce => Some(typed_array_reduce),
        Native::TypedArrayReduceRight => Some(typed_array_reduce_right),
        Native::TypedArrayLastIndexOf => Some(typed_array_last_index_of),
        Native::TypedArraySort => Some(typed_array_sort),
        Native::TypedArrayAt => Some(typed_array_at),
        Native::TypedArrayWith => Some(typed_array_with),
        Native::TypedArrayToReversed => Some(typed_array_to_reversed),
        Native::TypedArrayToSorted => Some(typed_array_to_sorted),
        Native::TypedArrayToLocaleString => Some(typed_array_to_locale_string),
        Native::ArrayBufferIsView => Some(array_buffer_is_view),
        Native::Uint8ArrayKeys | Native::Uint8ArrayValues | Native::Uint8ArrayEntries => Some(typed_array_iterator),
        Native::TypedArray | Native::Uint8Array | Native::Uint8ClampedArray | Native::Uint16Array | Native::Uint32Array | Native::Int8Array | Native::Int16Array | Native::Int32Array | Native::BigInt64Array | Native::BigUint64Array | Native::Float16Array | Native::Float32Array | Native::Float64Array => Some(typed_array_constructor_called),
        _ => super::typed_array_base64::entry(native),
    }
}

/// The live typed array a modern method reads: its element kind and length.
fn typed_array_live_view(vm: &mut Vm, p: &ResidualProgram, this: Value) -> Result<(TypedArrayKind, usize), JsError> {
    let Some(Cell::TypedArray { buffer, object, .. }) = vm.heap.get(this) else {
        return Err(vm.type_error(p, "typed array receiver is invalid".into()));
    };
    let (buffer, kind) = (*buffer, object.native.kind());
    if vm.typed_array_out_of_bounds(this) || vm.array_buffer_detached(buffer) {
        return Err(vm.type_error(p, "typed array receiver is invalid".into()));
    }
    let length = vm.typed_array_length(this).unwrap_or_default();
    Ok((kind, length))
}

/// The validated receiver of a typed array method: its backing buffer and length.
fn typed_array_validated_view(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value) -> Result<(Value, usize), JsError> {
    let buffer = vm.validate_typed_array_method(p, native, this)?;
    let length = vm.typed_array_length(this).unwrap_or_default();
    Ok((buffer, length))
}

/// The validated receiver of a method that writes: its length, once its backing buffer is mutable.
fn typed_array_mutable_view(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value) -> Result<usize, JsError> {
    let (buffer, length) = typed_array_validated_view(vm, p, native, this)?;
    if matches!(vm.heap.get(buffer), Some(Cell::ArrayBuffer { immutable: true, .. })) {
        return Err(vm.type_error(p, "typed array backing buffer is immutable".into()));
    }
    Ok(length)
}

fn uint8_array_set(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_set_native(p, this, args)
}

fn uint8_array_subarray(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_subarray_native(p, this, args)
}

fn uint8_array_reverse(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    let length = typed_array_mutable_view(vm, p, native, this)?;
    for index in 0..length / 2 {
        let other = length - index - 1;
        let left = vm
            .typed_array_get(this, index)
            .unwrap_or(Value::UNDEFINED);
        let right = vm
            .typed_array_get(this, other)
            .unwrap_or(Value::UNDEFINED);
        vm.typed_array_set(p, this, index, right)?;
        vm.typed_array_set(p, this, other, left)?;
    }
    Ok(this)
}

fn uint8_array_fill(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let length = typed_array_mutable_view(vm, p, native, this)?;
    vm.typed_array_fill_native(p, this, args, length)
}

fn uint8_array_copy_within(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let length = typed_array_mutable_view(vm, p, native, this)?;
    let target = vm.typed_array_relative_index(p, args.first(), length)?;
    let start = vm.typed_array_relative_index(p, args.get(1), length)?;
    let end_arg = args.get(2).filter(|value| !value.is_undefined());
    let end = match end_arg {
        Some(value) => vm.typed_array_relative_index(p, Some(value), length)?,
        None => length,
    };
    let count = end.saturating_sub(start).min(length - target);
    if count > 0 {
        vm.typed_array_validate_current_write(p, this)?;
        let current_length = vm.typed_array_length(this).unwrap_or_default();
        let count = count
            .min(current_length.saturating_sub(target))
            .min(current_length.saturating_sub(start));
        let (buffer, offset, width) = match vm.heap.get(this) {
            Some(Cell::TypedArray { buffer, object, .. }) => (*buffer, object.offset, object.native.kind().width()),
            _ => unreachable!("typed array receiver was validated"),
        };
        let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get_mut(buffer) else {
            unreachable!("typed array backing buffer was validated");
        };
        let from = offset + start * width;
        let to = offset + target * width;
        // Copy backing bytes directly to preserve NaN payloads and overlap.
        if count > 0 {
            bytes.make_mut().copy_within(from..from + count * width, to);
        }
    }
    Ok(this)
}

fn uint8_array_slice(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (buffer, length) = typed_array_validated_view(vm, p, native, this)?;
    let begin = vm.typed_array_relative_index(p, args.first(), length)?;
    let end_arg = args.get(1).filter(|value| !value.is_undefined());
    let end = vm.typed_array_relative_index(p, end_arg, length)?;
    let end = if end_arg.is_none() { length } else { end };
    let start = begin.min(end);
    let count = end.saturating_sub(begin);
    let target = vm.typed_array_species_create_for_writing(p, this, count)?;
    let target_buffer = match vm.heap.get(target) {
        Some(Cell::TypedArray { buffer, .. }) => *buffer,
        _ => unreachable!("species result was validated as a typed array"),
    };
    let current_length = vm.typed_array_length(this).unwrap_or_default();
    if count > 0
        && (vm.typed_array_out_of_bounds(this) || vm.array_buffer_detached(buffer))
    {
        return Err(vm.type_error(p, "typed array receiver is invalid".into()));
    }
    let copy_count = count.min(current_length.saturating_sub(start));
    let source_kind = vm.typed_array_kind(this).unwrap();
    if copy_count > 0 && vm.typed_array_kind(target) == Some(source_kind) {
        let source_offset =
            vm.typed_array_byte_offset(this).unwrap() + start * source_kind.width();
        let target_offset = vm.typed_array_byte_offset(target).unwrap();
        let byte_count = copy_count * source_kind.width();
        if buffer == target_buffer {
            let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get_mut(buffer)
            else {
                unreachable!("validated typed array backing buffer");
            };
            let mut bytes = bytes.make_mut();
            // Slice copies forward even when species returns an overlapping view.
            for index in 0..byte_count {
                bytes[target_offset + index] = bytes[source_offset + index];
            }
        } else {
            let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get(buffer) else {
                unreachable!("validated typed array backing buffer");
            };
            let source_bytes = bytes.read()[source_offset..source_offset + byte_count].to_vec();
            let Some(Cell::ArrayBuffer { bytes, .. }) =
                vm.heap.get_mut(target_buffer)
            else {
                unreachable!("validated species backing buffer");
            };
            bytes.make_mut()[target_offset..target_offset + byte_count]
                .copy_from_slice(&source_bytes);
        }
    } else {
        for index in 0..copy_count {
            let value = vm
                .typed_array_get(this, start + index)
                .unwrap_or(Value::UNDEFINED);
            vm.typed_array_set(p, target, index, value)?;
        }
    }
    Ok(target)
}

fn uint8_array_search(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (buffer, length) = typed_array_validated_view(vm, p, native, this)?;
    if length == 0 {
        return Ok(if native == Native::Uint8ArrayIncludes {
            Value::FALSE
        } else {
            Value::number(-1.0)
        });
    }
    let search = args.first().copied().unwrap_or(Value::UNDEFINED);
    let from = vm.typed_array_relative_index(p, args.get(1), length)?;
    let invalid_view =
        vm.typed_array_out_of_bounds(this) || vm.array_buffer_detached(buffer);
    if native == Native::Uint8ArrayIndexOf && invalid_view {
        return Ok(Value::number(-1.0));
    }
    let current_length = if native == Native::Uint8ArrayIncludes {
        length
    } else {
        vm.typed_array_length(this)
            .unwrap_or_default()
            .min(length)
    };
    // A Number search over a Number kind is the staged search over the elements
    // (`typed_array_find_number`); any other search takes the general comparison.
    let found = match search.as_number() {
        Some(number) if vm.typed_array_kind(this).is_some_and(|kind| !typed_array_kind_is_bigint(kind)) => {
            vm.typed_array_find_number(this, from, current_length, number, native == Native::Uint8ArrayIncludes)
        }
        _ => (from..current_length).find(|index| {
            let Some(value) = vm.typed_array_get(this, *index) else {
                return false;
            };
            if native == Native::Uint8ArrayIncludes {
                vm.same_value_zero(value, search)
            } else {
                vm.strict_equal(value, search)
            }
        }),
    };
    if native == Native::Uint8ArrayIncludes {
        Ok(if found.is_some() {
            Value::TRUE
        } else {
            Value::FALSE
        })
    } else {
        Ok(Value::number(found.map_or(-1.0, |index| index as f64)))
    }
}

fn uint8_array_join(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (_, length) = typed_array_validated_view(vm, p, native, this)?;
    let separator = if native == Native::Uint8ArrayToString {
        ",".to_owned()
    } else {
        match args.first().copied() {
            Some(value) if !value.is_undefined() => vm.to_string(p, value)?,
            None => ",".to_owned(),
            Some(_) => ",".to_owned(),
        }
    };
    let mut result = String::new();
    for index in 0..length {
        if index > 0 {
            result.push_str(&separator);
        }
        let value = vm
            .typed_array_get(this, index)
            .unwrap_or(Value::UNDEFINED);
        if value.is_null() || value.is_undefined() {
            continue;
        }
        result.push_str(&vm.to_string(p, value)?);
    }
    Ok(vm.heap.alloc_string(result.into()))
}

fn typed_array_for_each(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayForEach, this, args)
}

fn typed_array_map(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayMap, this, args)
}

fn typed_array_filter(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayFilter, this, args)
}

fn typed_array_some(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArraySome, this, args)
}

fn typed_array_every(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayEvery, this, args)
}

fn typed_array_find(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayFind, this, args)
}

fn typed_array_find_index(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayFindIndex, this, args)
}

fn typed_array_find_last(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayFindLast, this, args)
}

fn typed_array_find_last_index(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayFindLastIndex, this, args)
}

fn typed_array_reduce(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayReduce, this, args)
}

fn typed_array_reduce_right(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_callback_native(p, Native::ArrayReduceRight, this, args)
}

fn typed_array_last_index_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_last_index_of_native(p, this, args)
}

fn typed_array_sort(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.typed_array_sort_native(p, this, args)
}

fn typed_array_at(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (_, length) = typed_array_live_view(vm, p, this)?;
    let index = vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let index = crate::units::to_integer_or_infinity(index);
    Ok(crate::staged::typed_elements::element_at(index, length)
        .and_then(|position| vm.typed_array_get(this, position))
        .unwrap_or(Value::UNDEFINED))
}

fn typed_array_with(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (kind, length) = typed_array_live_view(vm, p, this)?;
    let index = vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let index = crate::units::to_integer_or_infinity(index);
    let relative = if index < 0.0 {
        length as f64 + index
    } else {
        index
    };
    let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    let converted = if typed_array_kind_is_bigint(kind)
    {
        let value = vm.to_bigint(p, value)?;
        vm.heap.alloc(Cell::bigint(value))
    } else {
        Value::number(vm.to_number(p, value)?)
    };
    let current_length = vm.typed_array_length(this).unwrap_or_default();
    if relative < 0.0 || relative >= current_length as f64 || !relative.is_finite() {
        return Err(vm.range_error(p, "typed array index is out of range".into()));
    }
    let values = (0..length)
        .map(|item| {
            if item == relative as usize {
                converted
            } else {
                vm.typed_array_get(this, item).unwrap_or(Value::UNDEFINED)
            }
        })
        .collect::<Vec<_>>();
    let target = vm.new_typed_array_of_kind(p, kind, length)?;
    for (item, value) in values.into_iter().enumerate() {
        vm.typed_array_set(p, target, item, value)?;
    }
    Ok(target)
}

fn typed_array_to_reversed(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    typed_array_copy_ordered(vm, p, this, args, true)
}

fn typed_array_to_sorted(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    typed_array_copy_ordered(vm, p, this, args, false)
}

fn typed_array_copy_ordered(vm: &mut Vm, p: &ResidualProgram, this: Value, args: &[Value], reversed: bool) -> Result<Value, JsError> {
    let (kind, length) = typed_array_live_view(vm, p, this)?;
    let comparator = args.first().copied().filter(|value| !value.is_undefined());
    if !reversed
        && let Some(value) = comparator
        && !vm.is_function(value)
    {
        return Err(vm.type_error(p, "sort comparator is not callable".into()));
    }
    let source = vm.heap.root(this);
    let comparator = if !reversed {
        comparator.map(|value| vm.heap.root(value))
    } else {
        None
    };
    let mut values = Vec::new();
    let mut target = None;
    let outcome = (|| {
        for index in 0..length {
            let this = vm.heap.root_value(source).unwrap();
            let value = vm
                .typed_array_get(this, index)
                .unwrap_or(Value::UNDEFINED);
            values.push(vm.heap.root(value));
        }
        if reversed {
            values.reverse();
        } else {
            vm.typed_array_sort_values(p, &mut values, comparator)?;
        }
        let result = vm.new_typed_array_of_kind(p, kind, length)?;
        let result = vm.heap.root(result);
        target = Some(result);
        for (index, value) in values.iter().enumerate() {
            let target = vm.heap.root_value(result).unwrap();
            let value = vm.heap.root_value(*value).unwrap();
            vm.typed_array_set(p, target, index, value)?;
        }
        Ok(vm.heap.root_value(result).unwrap())
    })();
    if let Some(target) = target {
        vm.heap.release_root(target);
    }
    for value in values {
        vm.heap.release_root(value);
    }
    if let Some(root) = comparator {
        vm.heap.release_root(root);
    }
    vm.heap.release_root(source);
    outcome
}

fn typed_array_to_locale_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let Some(Cell::TypedArray { buffer, .. }) = vm.heap.get(this) else {
        return Err(vm.type_error(p, "typed array receiver is invalid".into()));
    };
    if vm.typed_array_out_of_bounds(this) || vm.array_buffer_detached(*buffer) {
        return Err(vm.type_error(p, "typed array receiver is invalid".into()));
    }
    let length = vm.typed_array_length(this).unwrap_or_default();
    vm.array_to_locale_string_with_length(p, this, length, args)
}

fn array_buffer_is_view(vm: &mut Vm, _p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let value = args.first().copied().unwrap_or(Value::UNDEFINED);
    Ok(Value::boolean(vm.is_array_buffer_view(value)))
}

fn typed_array_iterator(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_iterator_native(p, native, this)
}

fn typed_array_constructor_called(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Err(vm.type_error(p, "typed array constructor requires new".into()))
}
