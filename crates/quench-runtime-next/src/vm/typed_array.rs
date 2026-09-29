use super::typed_array_install::TYPED_ARRAY_CALLBACK_METHODS;
use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::*;
impl<H: Host> Vm<H> {
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
        self.intern_atom("value");
        self.intern_atom("done");
        let uint8_array = self.native_value(Native::Uint8Array);
        let typed_array = self.native_value(Native::TypedArray);
        self.uint8_array_proto = self.object();
        self.typed_array_proto = self.object();
        self.object_data_mut(self.uint8_array_proto)
            .expect("Uint8Array prototype")
            .proto = self.typed_array_proto;
        self.object_data_mut(typed_array)
            .expect("TypedArray constructor")
            .proto = self.function_proto;
        self.set_named(program, typed_array, "prototype", self.typed_array_proto)?;
        self.set_builtin_value_named(self.typed_array_proto, "constructor", typed_array)?;
        let typed_name = self.heap.alloc(Cell::String("TypedArray".into()));
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
            .proto = typed_array;
        self.set_named(program, uint8_array, "prototype", self.uint8_array_proto)?;
        let prototype_atom = self.intern_atom("prototype");
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
        for (name, native) in [
            ("set", Native::Uint8ArraySet),
            ("reverse", Native::Uint8ArrayReverse),
            ("fill", Native::Uint8ArrayFill),
            ("copyWithin", Native::Uint8ArrayCopyWithin),
            ("subarray", Native::Uint8ArraySubarray),
            ("slice", Native::Uint8ArraySlice),
            ("includes", Native::Uint8ArrayIncludes),
            ("indexOf", Native::Uint8ArrayIndexOf),
            ("join", Native::Uint8ArrayJoin),
            ("keys", Native::Uint8ArrayKeys),
            ("values", Native::Uint8ArrayValues),
            ("entries", Native::Uint8ArrayEntries),
        ] {
            self.set_builtin_named(program, self.typed_array_proto, name, native)?;
        }
        for (name, native) in [
            ("setFromBase64", Native::Uint8ArraySetFromBase64),
            ("setFromHex", Native::Uint8ArraySetFromHex),
            ("toBase64", Native::Uint8ArrayToBase64),
            ("toHex", Native::Uint8ArrayToHex),
        ] {
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
        for (name, native) in [
            ("fromBase64", Native::Uint8ArrayFromBase64),
            ("fromHex", Native::Uint8ArrayFromHex),
        ] {
            self.set_builtin_named(program, uint8_array, name, native)?;
        }
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
        let iterator = self.well_known_symbols["iterator"];
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
            return Ok(
                if matches!(
                    args.first().and_then(|value| self.heap.get(*value)),
                    Some(Cell::TypedArray { .. }) | Some(Cell::DataView { .. })
                ) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            );
        }
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(*buffer) {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        }
        let buffer = match self.heap.get(this) {
            Some(Cell::TypedArray { buffer, .. }) => *buffer,
            _ => unreachable!("typed array receiver was validated"),
        };
        let length = self.typed_array_length(this).unwrap_or_default();
        if matches!(
            native,
            Native::Uint8ArrayReverse
                | Native::Uint8ArrayFill
                | Native::Uint8ArrayCopyWithin
                | Native::Uint8ArraySet
        ) && matches!(self.heap.get(buffer), Some(Cell::ArrayBuffer { immutable: true, .. }))
        {
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
            Native::Uint8ArrayFill => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value = if matches!(
                    self.typed_array_kind(this),
                    Some(TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64)
                ) {
                    let value = self.to_bigint(p, value)?;
                    self.heap.alloc(Cell::BigInt(value.to_string()))
                } else {
                    Value::number(self.to_number(p, value)?)
                };
                self.typed_array_validate_current_write(p, this)?;
                let start = self.typed_array_relative_index(p, args.get(1), length)?;
                self.typed_array_validate_current_write(p, this)?;
                let end_arg = args.get(2).filter(|value| !value.is_undefined());
                let end = match end_arg {
                    Some(value) => self.typed_array_relative_index(p, Some(value), length)?,
                    None => length,
                };
                self.typed_array_validate_current_write(p, this)?;
                for index in start..end {
                    self.typed_array_set(p, this, index, value)?;
                }
                Ok(this)
            }
            Native::Uint8ArrayCopyWithin => {
                let target = self.typed_array_relative_index(p, args.first(), length)?;
                self.typed_array_validate_current_write(p, this)?;
                let start = self.typed_array_relative_index(p, args.get(1), length)?;
                self.typed_array_validate_current_write(p, this)?;
                let end_arg = args.get(2).filter(|value| !value.is_undefined());
                let end = match end_arg {
                    Some(value) => self.typed_array_relative_index(p, Some(value), length)?,
                    None => length,
                };
                self.typed_array_validate_current_write(p, this)?;
                let current_length = self.typed_array_length(this).unwrap_or_default();
                let effective_length = current_length.min(length);
                let count = end
                    .saturating_sub(start)
                    .min(effective_length.saturating_sub(target))
                    .min(effective_length.saturating_sub(start));
                let values = (0..count)
                    .map(|index| {
                        self.typed_array_get(this, start + index)
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>();
                for (index, value) in values.into_iter().enumerate() {
                    self.typed_array_set(p, this, target + index, value)?;
                }
                Ok(this)
            }
            Native::Uint8ArraySet => {
                let source = args.first().copied().unwrap_or(Value::UNDEFINED);
                let start = args
                    .get(1)
                    .map(|value| self.to_number(p, *value))
                    .transpose()?
                    .unwrap_or(0.0);
                let start = if start.is_nan() { 0.0 } else { start.trunc() };
                if start < 0.0 || start.is_infinite() {
                    return Err(self.range_error(p, "typed array set offset is invalid".into()));
                }
                let start = start as usize;
                if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer) {
                    return Err(self.type_error(p, "typed array receiver is invalid".into()));
                }
                let current_length = self.typed_array_length(this).unwrap_or_default();
                let typed_source = matches!(self.heap.get(source), Some(Cell::TypedArray { .. }));
                let source_object = if typed_source {
                    source
                } else {
                    self.box_object_or_type_error(p, source)?
                };
                let source_length = if typed_source {
                    if self.typed_array_out_of_bounds(source) {
                        return Err(self.type_error(p, "typed array source is invalid".into()));
                    }
                    self.typed_array_length(source).unwrap_or_default()
                } else {
                    self.array_like_length(p, source_object)?
                };
                if start > current_length || source_length > current_length - start {
                    return Err(self.range_error(p, "typed array set source is too large".into()));
                }
                if typed_source {
                    let values = (0..source_length)
                        .map(|index| {
                            self.typed_array_get(source, index).unwrap_or(Value::UNDEFINED)
                        })
                        .collect::<Vec<_>>();
                    for (index, value) in values.into_iter().enumerate() {
                        self.typed_array_set(p, this, start + index, value)?;
                    }
                } else {
                    for index in 0..source_length {
                        let value = self.get_index(
                            p,
                            source_object,
                            Value::number(index as f64),
                        )?;
                        self.typed_array_set(p, this, start + index, value)?;
                    }
                }
                Ok(Value::UNDEFINED)
            }
            Native::Uint8ArraySubarray => unreachable!("subarray handled before common validation"),
            Native::Uint8ArraySlice => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end_arg = args.get(1).filter(|value| !value.is_undefined());
                let end = self.typed_array_relative_index(p, end_arg, length)?;
                let end = if end_arg.is_none() { length } else { end };
                let start = begin.min(end);
                let count = end.saturating_sub(begin);
                let target = self.typed_array_species_create(p, this, count)?;
                let target_buffer = match self.heap.get(target) {
                    Some(Cell::TypedArray { buffer, .. }) => *buffer,
                    _ => unreachable!("species result was validated as a typed array"),
                };
                let target_immutable = matches!(
                    self.heap.get(target_buffer),
                    Some(Cell::ArrayBuffer { immutable: true, .. })
                );
                if target_immutable && target_buffer != buffer {
                    return Err(self.type_error(p, "typed array species result is not writable".into()));
                }
                let current_length = self.typed_array_length(this).unwrap_or_default();
                if count > 0
                    && (self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer))
                {
                    return Err(self.type_error(p, "typed array receiver is invalid".into()));
                }
                let copy_count = count.min(current_length.saturating_sub(start));
                for index in 0..copy_count {
                    let value = self
                        .typed_array_get(this, start + index)
                        .unwrap_or(Value::UNDEFINED);
                    self.typed_array_set(p, target, index, value)?;
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
                    self.typed_array_length(this).unwrap_or_default().min(length)
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
                Ok(self.heap.alloc(Cell::String(result.into())))
            }
            _ => unreachable!(),
        }
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
            || matches!(self.heap.get(buffer), Some(Cell::ArrayBuffer { immutable: true, .. }))
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
            offset,
            length_tracking,
            kind,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let (buffer, offset, length_tracking, kind) = (*buffer, *offset, *length_tracking, *kind);
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
        self.typed_array_species_create_with_args(
            p,
            this,
            &constructor_args,
            target_length,
        )
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
        let immutable = matches!(self.heap.get(buffer), Some(Cell::ArrayBuffer { immutable: true, .. }));
        if self.typed_array_out_of_bounds(this) || self.array_buffer_detached(buffer) || immutable {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        }

        let length = self.typed_array_length(this).unwrap_or_default();
        let mut values = (0..length)
            .map(|index| self.typed_array_get(this, index).unwrap_or(Value::UNDEFINED))
            .collect::<Vec<_>>();
        self.typed_array_sort_values(p, &mut values, comparator)?;
        for (index, value) in values.into_iter().enumerate() {
            self.typed_array_set(p, this, index, value)?;
        }
        Ok(this)
    }

    fn typed_array_sort_values(
        &mut self,
        p: &ResidualProgram,
        values: &mut [Value],
        comparator: Option<Value>,
    ) -> Result<(), JsError> {
        super::sort::try_stable_sort_by(values, |left, right| {
            Ok(match self.typed_array_sort_compare(p, comparator, *left, *right)? {
                -1 => std::cmp::Ordering::Less,
                1 => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
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
                self.heap.alloc(Cell::BigInt(value.to_string()))
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
        let mut values = (0..length)
            .map(|index| self.typed_array_get(this, index).unwrap_or(Value::UNDEFINED))
            .collect::<Vec<_>>();
        if native == Native::TypedArrayToReversed {
            values.reverse();
        } else {
            self.typed_array_sort_values(p, &mut values, comparator)?;
        }
        let target = self.new_typed_array_of_kind(p, kind, length)?;
        for (index, value) in values.into_iter().enumerate() {
            self.typed_array_set(p, target, index, value)?;
        }
        Ok(target)
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

    fn typed_array_sort_compare(
        &mut self,
        p: &ResidualProgram,
        comparator: Option<Value>,
        left: Value,
        right: Value,
    ) -> Result<i8, JsError> {
        if let Some(comparator) = comparator {
            let result = self.call_value(p, comparator, Value::UNDEFINED, &[left, right])?;
            let number = self.to_number(p, result)?;
            return Ok(if number.is_nan() || number == 0.0 {
                0
            } else if number < 0.0 {
                -1
            } else {
                1
            });
        }
        if let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) =
            (self.heap.get(left), self.heap.get(right))
        {
            let left = left.parse::<i128>().unwrap_or_default();
            let right = right.parse::<i128>().unwrap_or_default();
            return Ok(match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
        }
        let left = left.as_number().unwrap_or(f64::NAN);
        let right = right.as_number().unwrap_or(f64::NAN);
        Ok(if left.is_nan() {
            if right.is_nan() { 0 } else { 1 }
        } else if right.is_nan() {
            -1
        } else if left == 0.0 && right == 0.0 {
            match (left.is_sign_negative(), right.is_sign_negative()) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            }
        } else {
            match left.partial_cmp(&right).unwrap_or(std::cmp::Ordering::Equal) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        })
    }

    pub(super) fn typed_array_to_string_tag_native(&mut self, this: Value) -> Value {
        let Some(kind) = self.typed_array_kind(this) else {
            return Value::UNDEFINED;
        };
        let name = TYPED_ARRAY_INSTALLS
            .iter()
            .find_map(|(candidate, _, name)| (*candidate == kind).then_some(*name))
            .unwrap_or("Uint8Array");
        self.heap.alloc(Cell::String(name.into()))
    }

    pub(super) fn typed_array_getter_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::TypedArray {
            buffer,
            offset,
            length,
            length_tracking,
            kind,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(p, "typed array receiver is invalid".into()));
        };
        let (buffer, offset, length, length_tracking, kind) =
            (*buffer, *offset, *length, *length_tracking, *kind);
        let (buffer_length, detached) = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, detached, .. }) => (bytes.len(), *detached),
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
        Ok(match native {
            Native::TypedArrayBufferGetter => buffer,
            Native::TypedArrayByteOffsetGetter if !out_of_bounds => {
                Value::number(self.typed_array_byte_offset(this).unwrap_or_default() as f64)
            }
            Native::TypedArrayLengthGetter if !out_of_bounds => {
                Value::number(view_length as f64)
            }
            Native::TypedArrayByteLengthGetter if !out_of_bounds => Value::number(
                (view_length * kind.width()) as f64,
            ),
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
        let integer = if number.is_finite() { number.trunc() } else { number };
        Ok(if integer.is_nan() || integer == 0.0 {
            0
        } else if integer < 0.0 {
            length.saturating_sub(integer.abs() as usize)
        } else {
            (integer as usize).min(length)
        })
    }

    pub(super) fn construct_uint8_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_typed_array_native(p, args, TypedArrayKind::Uint8, "Uint8Array")
    }

    pub(super) fn uint8_from_value(number: f64) -> u8 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(256.0) as u8
        }
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

    pub(super) fn uint16_from_value(number: f64) -> u16 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(65_536.0) as u16
        }
    }

    pub(super) fn uint32_from_value(number: f64) -> u32 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(4_294_967_296.0) as u32
        }
    }
}
