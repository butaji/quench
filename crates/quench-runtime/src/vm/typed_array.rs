use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::typed_array_install::{TYPED_ARRAY_BASE_METHODS, TYPED_ARRAY_CALLBACK_METHODS};
use super::typed_array_install::{UINT8_ARRAY_PROTOTYPE_METHODS, UINT8_ARRAY_STATIC_METHODS};
use super::*;

/// Direct-mapped entries of `TypedArrayGeometryProofs`, by name: the four geometry names
/// (`length`, `byteLength`, `byteOffset`, `buffer`) a typed array reads through its
/// prototypes' accessors, each under the one prototype chain (a Buffer's or a plain view's)
/// a program mostly reads it from.
const TYPED_ARRAY_GEOMETRY_ENTRIES: usize = 8;

/// That reading `atom` from a typed array whose own shape lacks it, with prototype
/// `prototype`, calls `getter`: every prototype before the holder lacks the name, which the
/// holder keeps as that accessor. Proved under one `prototype_layout_epoch`, which any
/// prototype change or collection advances.
#[derive(Clone, Copy)]
struct TypedArrayGeometryProof {
    prototype: Value,
    atom: Atom,
    epoch: u64,
    getter: Value,
}

#[derive(Default)]
pub(super) struct TypedArrayGeometryProofs([std::cell::Cell<Option<TypedArrayGeometryProof>>; TYPED_ARRAY_GEOMETRY_ENTRIES]);

impl<H: Host> Vm<H> {
    /// A read of a geometry name (`AtomClass::VIRTUAL_FIELD`) from typed array `object`
    /// that runs no guest code: the intrinsic accessor its chain resolves the name to, as
    /// a pure native read (`pure_native_getter_value`). `None` leaves the read to the
    /// general path: an own property of the name, an exotic or dictionary link, a data
    /// property or a script getter on the way.
    pub(super) fn typed_array_geometry_read(&mut self, object: Value, atom: Atom) -> Option<Value> {
        let Some(Cell::TypedArray { object: view, .. }) = self.heap.get(object) else {
            return None;
        };
        let data = &view.object;
        if data.has_side_descriptors()
            || self.shape_is_dictionary(data.shape())
            || self.shape_slot(data.shape(), atom).is_some()
        {
            return None;
        }
        let prototype = data.proto();
        let epoch = crate::heap::prototype_layout_epoch();
        let index = atom as usize % TYPED_ARRAY_GEOMETRY_ENTRIES;
        let getter = match self.typed_array_geometry.0[index].get() {
            Some(proof) if proof.prototype == prototype && proof.atom == atom && proof.epoch == epoch => proof.getter,
            _ => {
                let (getter, depth) = self.typed_array_geometry_getter(prototype, atom)?;
                // Every link up to the holder becomes a prototype first, so a later change to
                // one advances the epoch and retires the proof.
                let mut link = prototype;
                for _ in 0..depth {
                    self.heap.mark_prototype(link);
                    link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
                }
                if crate::heap::prototype_layout_epoch() == epoch {
                    self.typed_array_geometry.0[index].set(Some(TypedArrayGeometryProof { prototype, atom, epoch, getter }));
                }
                getter
            }
        };
        self.pure_native_getter_value(getter, object)
    }

    /// The getter of the accessor `atom` names first along the ordinary prototypes from
    /// `prototype`, where no earlier link has the name, and the number of links to it.
    fn typed_array_geometry_getter(&self, prototype: Value, atom: Atom) -> Option<(Value, usize)> {
        let key = PropertyKey::string(atom);
        let mut link = prototype;
        for depth in 1.. {
            let data = self.heap.ordinary_object(link)?;
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
    pub(super) fn is_array_buffer_view(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::TypedArray { .. } | Cell::DataView { .. }))
    }

    pub(super) fn maybe_call_typed_array_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if native.is_typed_array_method() {
            if native.is_uint8_array_base64_method() {
                return Some(self.typed_array_base64_native(p, native, this, args));
            }
            Some(self.typed_array_native(p, native, this, args))
        } else if native.is_typed_array_iterator() {
            Some(self.array_iterator_native(p, native, this))
        } else {
            None
        }
    }
    pub(super) fn install_typed_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.runtime_atoms.value;
        self.runtime_atoms.done;
        let uint8_array = self.native_value(Native::Uint8Array);
        let typed_array = self.native_value(Native::TypedArray);
        self.uint8_array_proto = self.object();
        self.typed_array_proto = self.object();
        let (typed_array_proto, function_proto) = (self.typed_array_proto, self.function_proto);
        self.object_data_mut(self.uint8_array_proto)
            .expect("Uint8Array prototype")
            .set_proto(typed_array_proto);
        self.object_data_mut(typed_array)
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
        self.object_data_mut(uint8_array)
            .expect("Uint8Array constructor")
            .set_proto(typed_array);
        self.set_named(program, uint8_array, "prototype", self.uint8_array_proto)?;
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
        self.set_builtin_value_named(self.uint8_array_proto, "constructor", uint8_array)?;
        self.set_builtin_function_name(uint8_array, "Uint8Array")?;
        self.set_named_constant(
            program,
            uint8_array,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.set_named_constant(
            program,
            self.uint8_array_proto,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        for &(name, native) in TYPED_ARRAY_BASE_METHODS {
            self.set_builtin_named(program, self.typed_array_proto, name, native)?;
        }
        for &(name, native) in UINT8_ARRAY_PROTOTYPE_METHODS {
            self.set_builtin_named(program, self.uint8_array_proto, name, native)?;
        }
        for &(name, native, _) in TYPED_ARRAY_CALLBACK_METHODS {
            let method = self.native_with_realm(native, Value::NULL, self.realm.globals);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(self.typed_array_proto, name, method)?;
        }
        let to_string_atom = self.intern_atom("toString");
        let to_string = self.get_property(program, self.array_proto, to_string_atom)?;
        self.set_builtin_value_named(self.typed_array_proto, "toString", to_string)?;
        let last_index_of = self.native_with_realm(
            Native::TypedArrayLastIndexOf,
            Value::NULL,
            self.realm.globals,
        );
        self.set_builtin_function_name(last_index_of, "lastIndexOf")?;
        self.set_builtin_value_named(self.typed_array_proto, "lastIndexOf", last_index_of)?;
        let sort = self.native_with_realm(Native::TypedArraySort, Value::NULL, self.realm.globals);
        self.set_builtin_function_name(sort, "sort")?;
        self.set_builtin_value_named(self.typed_array_proto, "sort", sort)?;
        for (name, native) in [
            ("at", Native::TypedArrayAt),
            ("toReversed", Native::TypedArrayToReversed),
            ("toSorted", Native::TypedArrayToSorted),
            ("with", Native::TypedArrayWith),
            ("toLocaleString", Native::TypedArrayToLocaleString),
        ] {
            let method = self.native_with_realm(native, Value::NULL, self.realm.globals);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(self.typed_array_proto, name, method)?;
        }
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
        for &(name, native) in UINT8_ARRAY_STATIC_METHODS {
            self.set_builtin_named(program, uint8_array, name, native)?;
        }
        self.realm.intrinsics.builtin_prototypes.insert(
            (self.realm.globals, Native::Uint8Array),
            self.uint8_array_proto,
        );
        for &(kind, native, name) in TYPED_ARRAY_INSTALLS {
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
    pub(super) fn typed_array_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TypedArrayLastIndexOf {
            return self.typed_array_last_index_of_native(p, this, args);
        }
        if native == Native::TypedArraySort {
            return self.typed_array_sort_native(p, this, args);
        }
        if native == Native::TypedArrayToLocaleString {
            let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
                return Err(self.type_error(p, "typed array receiver is invalid".into()));
            };
            if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
                return Err(self.type_error(p, "typed array receiver is invalid".into()));
            }
            let length = self.typed_array_length(this).unwrap_or_default();
            return self.array_to_locale_string_with_length(p, this, length, args);
        }
        if native == Native::Uint8ArraySet {
            return self.typed_array_set_native(p, this, args);
        }
        if native == Native::Uint8ArraySubarray {
            return self.typed_array_subarray_native(p, this, args);
        }
        if matches!(
            native,
            Native::TypedArrayAt
                | Native::TypedArrayToReversed
                | Native::TypedArrayToSorted
                | Native::TypedArrayWith
        ) {
            return self.typed_array_modern_native(p, native, this, args);
        }
        if let Some(array_native) =
            TYPED_ARRAY_CALLBACK_METHODS
                .iter()
                .find_map(|(_, typed_native, array_native)| {
                    (*typed_native == native).then_some(*array_native)
                })
        {
            return self.typed_array_callback_native(p, array_native, this, args);
        }
        if native == Native::ArrayBufferIsView {
            let value = args.first().copied().unwrap_or(Value::UNDEFINED);
            return Ok(Value::boolean(self.is_array_buffer_view(value)));
        }
        let buffer = self.validate_typed_array_method(p, native, this)?;
        let length = self.typed_array_length(this).unwrap_or_default();
        if matches!(
            native,
            Native::Uint8ArrayReverse | Native::Uint8ArrayFill | Native::Uint8ArrayCopyWithin
        ) && matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer {
                immutable: true,
                ..
            })
        ) {
            return Err(self.type_error(p, "typed array backing buffer is immutable".into()));
        }
        match native {
            Native::Uint8ArrayReverse => {
                for index in 0..length / 2 {
                    let other = length - index - 1;
                    let left = self
                        .typed_array_get(this, index)
                        .unwrap_or(Value::UNDEFINED);
                    let right = self
                        .typed_array_get(this, other)
                        .unwrap_or(Value::UNDEFINED);
                    self.typed_array_set(p, this, index, right)?;
                    self.typed_array_set(p, this, other, left)?;
                }
                Ok(this)
            }
            Native::Uint8ArrayFill => self.typed_array_fill_native(p, this, args, length),
            Native::Uint8ArrayCopyWithin => {
                let target = self.typed_array_relative_index(p, args.first(), length)?;
                let start = self.typed_array_relative_index(p, args.get(1), length)?;
                let end_arg = args.get(2).filter(|value| !value.is_undefined());
                let end = match end_arg {
                    Some(value) => self.typed_array_relative_index(p, Some(value), length)?,
                    None => length,
                };
                let count = end.saturating_sub(start).min(length - target);
                if count > 0 {
                    self.typed_array_validate_current_write(p, this)?;
                    let current_length = self.typed_array_length(this).unwrap_or_default();
                    let count = count
                        .min(current_length.saturating_sub(target))
                        .min(current_length.saturating_sub(start));
                    let (buffer, offset, width) = match self.heap.get(this) {
                        Some(Cell::TypedArray {
                            buffer,
                            object,
                            kind,
                            ..
                        }) => (*buffer, object.offset, kind.width()),
                        _ => unreachable!("typed array receiver was validated"),
                    };
                    let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
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
            Native::Uint8ArraySet => unreachable!("set handled before common validation"),
            Native::Uint8ArraySubarray => unreachable!("subarray handled before common validation"),
            Native::Uint8ArraySlice => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end_arg = args.get(1).filter(|value| !value.is_undefined());
                let end = self.typed_array_relative_index(p, end_arg, length)?;
                let end = if end_arg.is_none() { length } else { end };
                let start = begin.min(end);
                let count = end.saturating_sub(begin);
                let target = self.typed_array_species_create_for_writing(p, this, count)?;
                let target_buffer = match self.heap.get(target) {
                    Some(Cell::TypedArray { buffer, .. }) => *buffer,
                    _ => unreachable!("species result was validated as a typed array"),
                };
                let current_length = self.typed_array_length(this).unwrap_or_default();
                if count > 0
                    && (self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer))
                {
                    return Err(self.type_error(p, "typed array receiver is invalid".into()));
                }
                let copy_count = count.min(current_length.saturating_sub(start));
                let source_kind = self.typed_array_kind(this).unwrap();
                if copy_count > 0 && self.typed_array_kind(target) == Some(source_kind) {
                    let source_offset =
                        self.typed_array_byte_offset(this).unwrap() + start * source_kind.width();
                    let target_offset = self.typed_array_byte_offset(target).unwrap();
                    let byte_count = copy_count * source_kind.width();
                    if buffer == target_buffer {
                        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer)
                        else {
                            unreachable!("validated typed array backing buffer");
                        };
                        let mut bytes = bytes.make_mut();
                        // Slice copies forward even when species returns an overlapping view.
                        for index in 0..byte_count {
                            bytes[target_offset + index] = bytes[source_offset + index];
                        }
                    } else {
                        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) else {
                            unreachable!("validated typed array backing buffer");
                        };
                        let source_bytes = bytes.read()[source_offset..source_offset + byte_count].to_vec();
                        let Some(Cell::ArrayBuffer { bytes, .. }) =
                            self.heap.get_mut(target_buffer)
                        else {
                            unreachable!("validated species backing buffer");
                        };
                        bytes.make_mut()[target_offset..target_offset + byte_count]
                            .copy_from_slice(&source_bytes);
                    }
                } else {
                    for index in 0..copy_count {
                        let value = self
                            .typed_array_get(this, start + index)
                            .unwrap_or(Value::UNDEFINED);
                        self.typed_array_set(p, target, index, value)?;
                    }
                }
                Ok(target)
            }
            Native::Uint8ArrayIncludes | Native::Uint8ArrayIndexOf => {
                if length == 0 {
                    return Ok(if native == Native::Uint8ArrayIncludes {
                        Value::FALSE
                    } else {
                        Value::number(-1.0)
                    });
                }
                let search = args.first().copied().unwrap_or(Value::UNDEFINED);
                let from = self.typed_array_relative_index(p, args.get(1), length)?;
                let invalid_view =
                    self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer);
                if native == Native::Uint8ArrayIndexOf && invalid_view {
                    return Ok(Value::number(-1.0));
                }
                let current_length = if native == Native::Uint8ArrayIncludes {
                    length
                } else {
                    self.typed_array_length(this)
                        .unwrap_or_default()
                        .min(length)
                };
                let found = (from..current_length).find(|index| {
                    let Some(value) = self.typed_array_get(this, *index) else {
                        return false;
                    };
                    if native == Native::Uint8ArrayIncludes {
                        self.same_value_zero(value, search)
                    } else {
                        self.strict_equal(value, search)
                    }
                });
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
            Native::Uint8ArrayJoin | Native::Uint8ArrayToString => {
                let separator = if native == Native::Uint8ArrayToString {
                    ",".to_owned()
                } else {
                    match args.first().copied() {
                        Some(value) if !value.is_undefined() => self.to_string(p, value)?,
                        None => ",".to_owned(),
                        Some(_) => ",".to_owned(),
                    }
                };
                let mut result = String::new();
                for index in 0..length {
                    if index > 0 {
                        result.push_str(&separator);
                    }
                    let value = self
                        .typed_array_get(this, index)
                        .unwrap_or(Value::UNDEFINED);
                    if value.is_null() || value.is_undefined() {
                        continue;
                    }
                    result.push_str(&self.to_string(p, value)?);
                }
                Ok(self.heap.alloc_string(result.into()))
            }
            _ => unreachable!(),
        }
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
            length_tracking,
            kind,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let (buffer, offset, length_tracking, kind) =
            (*buffer, object.offset, *length_tracking, *kind);
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
        super::sort::try_stable_sort_by(values, |left, right| {
            if let Some(comparator) = comparator {
                let comparator = self.heap.root_value(comparator).unwrap();
                let left = self.heap.root_value(*left).unwrap();
                let right = self.heap.root_value(*right).unwrap();
                self.compare_sort_callback(p, comparator, left, right)
            } else {
                let left = self.heap.root_value(*left).unwrap();
                let right = self.heap.root_value(*right).unwrap();
                Ok(self.typed_array_sort_compare(left, right))
            }
        })
    }

    fn typed_array_modern_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, kind, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let (buffer, kind) = (*buffer, *kind);
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer) {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        }
        let length = self.typed_array_length(this).unwrap_or_default();
        if native == Native::TypedArrayAt {
            return Ok(self
                .indexed_at_index(p, args.first().copied().unwrap_or(Value::UNDEFINED), length)?
                .and_then(|index| self.typed_array_get(this, index))
                .unwrap_or(Value::UNDEFINED));
        }
        if native == Native::TypedArrayWith {
            let index = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let index = if index.is_nan() || index == 0.0 {
                0.0
            } else {
                index.trunc()
            };
            let relative = if index < 0.0 {
                length as f64 + index
            } else {
                index
            };
            let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let converted = if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64)
            {
                let value = self.to_bigint(p, value)?;
                self.heap.alloc(Cell::bigint(value))
            } else {
                Value::number(self.to_number(p, value)?)
            };
            let current_length = self.typed_array_length(this).unwrap_or_default();
            if relative < 0.0 || relative >= current_length as f64 || !relative.is_finite() {
                return Err(self.range_error(p, "typed array index is out of range".into()));
            }
            let values = (0..length)
                .map(|item| {
                    if item == relative as usize {
                        converted
                    } else {
                        self.typed_array_get(this, item).unwrap_or(Value::UNDEFINED)
                    }
                })
                .collect::<Vec<_>>();
            let target = self.new_typed_array_of_kind(p, kind, length)?;
            for (item, value) in values.into_iter().enumerate() {
                self.typed_array_set(p, target, item, value)?;
            }
            return Ok(target);
        }

        let comparator = args.first().copied().filter(|value| !value.is_undefined());
        if native == Native::TypedArrayToSorted
            && let Some(value) = comparator
            && !self.is_function(value)
        {
            return Err(self.type_error(p, "sort comparator is not callable".into()));
        }
        let source = self.heap.root(this);
        let comparator = if native == Native::TypedArrayToSorted {
            comparator.map(|value| self.heap.root(value))
        } else {
            None
        };
        let mut values = Vec::new();
        let mut target = None;
        let outcome = (|| {
            for index in 0..length {
                let this = self.heap.root_value(source).unwrap();
                let value = self
                    .typed_array_get(this, index)
                    .unwrap_or(Value::UNDEFINED);
                values.push(self.heap.root(value));
            }
            if native == Native::TypedArrayToReversed {
                values.reverse();
            } else {
                self.typed_array_sort_values(p, &mut values, comparator)?;
            }
            let result = self.new_typed_array_of_kind(p, kind, length)?;
            let result = self.heap.root(result);
            target = Some(result);
            for (index, value) in values.iter().enumerate() {
                let target = self.heap.root_value(result).unwrap();
                let value = self.heap.root_value(*value).unwrap();
                self.typed_array_set(p, target, index, value)?;
            }
            Ok(self.heap.root_value(result).unwrap())
        })();
        if let Some(target) = target {
            self.heap.release_root(target);
        }
        for value in values {
            self.heap.release_root(value);
        }
        if let Some(root) = comparator {
            self.heap.release_root(root);
        }
        self.heap.release_root(source);
        outcome
    }

    fn new_typed_array_of_kind(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        length: usize,
    ) -> Result<Value, JsError> {
        let name = TYPED_ARRAY_INSTALLS
            .iter()
            .find_map(|(candidate, _, name)| (*candidate == kind).then_some(*name))
            .unwrap_or("Uint8Array");
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
        let name = TYPED_ARRAY_INSTALLS
            .iter()
            .find_map(|(candidate, _, name)| (*candidate == kind).then_some(*name))
            .unwrap_or("Uint8Array");
        self.heap.alloc_string(name.into())
    }

    pub(super) fn validate_typed_array_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            let method = TYPED_ARRAY_BASE_METHODS
                .iter()
                .find_map(|(name, candidate)| (*candidate == native).then_some(*name));
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
            let method = TYPED_ARRAY_BASE_METHODS
                .iter()
                .find_map(|(name, candidate)| (*candidate == native).then_some(*name));
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
                Some(Cell::Symbol(description)) => {
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
            length_tracking,
            kind,
            ..
        }) = self.heap.get(this)
        else {
            return None;
        };
        let (buffer, offset, length, length_tracking, kind) =
            (*buffer, object.offset, *length, *length_tracking, *kind);
        let (buffer_length, detached) = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => (bytes.len(), *detached),
            _ => (0, true),
        };
        let out_of_bounds = detached
            || if length_tracking {
                offset > buffer_length
            } else {
                offset.saturating_add(length.saturating_mul(kind.width())) > buffer_length
            };
        let view_length = if detached || out_of_bounds {
            0
        } else if length_tracking {
            buffer_length.saturating_sub(offset) / kind.width()
        } else {
            length
        };
        Some(match native {
            Native::TypedArrayBufferGetter => buffer,
            Native::TypedArrayByteOffsetGetter if !out_of_bounds => {
                Value::number(self.typed_array_byte_offset(this).unwrap_or_default() as f64)
            }
            Native::TypedArrayLengthGetter if !out_of_bounds => Value::number(view_length as f64),
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
        let Some(value) = value else { return Ok(0) };
        let number = self.to_number(p, *value)?;
        let integer = if number.is_finite() {
            number.trunc()
        } else {
            number
        };
        Ok(if integer.is_nan() || integer == 0.0 {
            0
        } else if integer < 0.0 {
            length.saturating_sub(integer.abs() as usize)
        } else {
            (integer as usize).min(length)
        })
    }

    /// ToUint8: the low bits of ToUint32, as 2^8 divides 2^32.
    pub(super) fn uint8_from_value(number: f64) -> u8 {
        crate::value::number_to_u32(number) as u8
    }

    pub(super) fn uint8_clamped_from_value(number: f64) -> u8 {
        if number.is_nan() || number <= 0.0 {
            return 0;
        }
        if number >= 255.0 {
            return 255;
        }
        let floor = number.floor();
        let fraction = number - floor;
        if fraction < 0.5 || (fraction == 0.5 && (floor as u64).is_multiple_of(2)) {
            floor as u8
        } else {
            floor as u8 + 1
        }
    }

    /// ToUint16: the low bits of ToUint32, as 2^16 divides 2^32.
    pub(super) fn uint16_from_value(number: f64) -> u16 {
        crate::value::number_to_u32(number) as u16
    }

    pub(super) fn uint32_from_value(number: f64) -> u32 {
        crate::value::number_to_u32(number)
    }
}
