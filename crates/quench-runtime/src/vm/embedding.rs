use super::*;

const TYPED_BRANDS: &[(crate::BuiltinBrand, TypedArrayKind)] = &[
    (crate::BuiltinBrand::Uint8Array, TypedArrayKind::Uint8),
    (
        crate::BuiltinBrand::Uint8ClampedArray,
        TypedArrayKind::Uint8Clamped,
    ),
    (crate::BuiltinBrand::Uint16Array, TypedArrayKind::Uint16),
    (crate::BuiltinBrand::Uint32Array, TypedArrayKind::Uint32),
    (crate::BuiltinBrand::Int8Array, TypedArrayKind::Int8),
    (crate::BuiltinBrand::Int16Array, TypedArrayKind::Int16),
    (crate::BuiltinBrand::Int32Array, TypedArrayKind::Int32),
    (crate::BuiltinBrand::BigInt64Array, TypedArrayKind::BigInt64),
    (
        crate::BuiltinBrand::BigUint64Array,
        TypedArrayKind::BigUint64,
    ),
    (crate::BuiltinBrand::Float16Array, TypedArrayKind::Float16),
    (crate::BuiltinBrand::Float32Array, TypedArrayKind::Float32),
    (crate::BuiltinBrand::Float64Array, TypedArrayKind::Float64),
];
pub(super) fn typed_array_brand(kind: TypedArrayKind) -> crate::BuiltinBrand {
    TYPED_BRANDS
        .iter()
        .find(|(_, candidate)| *candidate == kind)
        .expect("typed kind")
        .0
}

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_heap_memory_composition(&self) -> crate::HeapMemoryComposition {
        self.heap.memory_composition()
    }

    pub(crate) fn embedding_objects_with_prototype(&self, prototype: Value) -> Vec<Value> {
        self.heap.objects_with_prototype(prototype)
    }

    pub(super) fn boxed_primitive_value(&self, value: Value) -> Option<Value> {
        self.object_data(value)?.boxed_primitive()
    }

    pub(super) fn boxed_primitive_data(
        &self,
        value: Value,
    ) -> Option<(crate::BuiltinBrand, Value)> {
        let payload = self.boxed_primitive_value(value)?;
        use crate::BuiltinBrand as B;
        let brand = match self.heap.get(payload) {
            Some(Cell::String(_)) => B::StringObject,
            Some(Cell::Symbol(_)) => B::SymbolObject,
            Some(Cell::BigInt(_)) => B::BigIntObject,
            _ if payload.as_bool().is_some() => B::BooleanObject,
            _ if payload.as_number().is_some() => B::NumberObject,
            _ => unreachable!("primitive wrapper contains primitive data"),
        };
        Some((brand, payload))
    }

    pub(crate) fn embedding_host_weak_root(&mut self, target: RootId) -> Result<RootId, JsError> {
        let value = self.embedding_value(target)?;
        let target = self
            .heap
            .weak_handle(value)
            .ok_or_else(|| JsError("host weak reference requires a live heap value".into()))?;
        // The existing WeakRef cell owns target pruning before slot reuse. This
        // private cell has no guest prototype and never adds a kept object.
        let reference = self.heap.alloc(Cell::WeakRef {
            object: Box::new(Self::empty_object(Value::NULL)),
            target: Some(target),
        });
        Ok(self.heap.root(reference))
    }

    pub(crate) fn embedding_host_weak_value(&self, reference: RootId) -> Option<Value> {
        let value = self.heap.root_value(reference)?;
        match self.heap.get(value)? {
            Cell::WeakRef {
                target: Some(target),
                ..
            } => self.heap.weak_value(*target),
            _ => None,
        }
    }

    pub(crate) fn embedding_push_native_stack_frame(&mut self, name: &str) {
        self.native_stack_frames
            .push((self.frames.len(), name.to_owned()));
    }

    pub(crate) fn embedding_pop_native_stack_frame(&mut self) {
        self.native_stack_frames
            .pop()
            .expect("balanced native stack scope");
    }

    pub(crate) fn embedding_is_array(&mut self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.is_array(&program, value)
    }

    pub(crate) fn embedding_prototype(&mut self, root: RootId) -> Result<Value, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.object_get_prototype_of(&program, value)
    }

    pub(crate) fn embedding_weak_map_get(
        &self,
        map: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let map = self.embedding_value(map)?;
        let key = self.embedding_value(key)?;
        let Some(Cell::WeakMap { entries, .. }) = self.heap.get(map) else {
            return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
        };
        Ok(entries.get(key).unwrap_or(Value::UNDEFINED))
    }

    pub(crate) fn embedding_own_keys(&mut self, root: RootId) -> Result<Vec<RootId>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let keys = self.object_own_key_values(&program, value)?;
        Ok(keys.into_iter().map(|key| self.root(key)).collect())
    }

    pub(crate) fn embedding_define_descriptor(
        &mut self,
        object: RootId,
        key: RootId,
        descriptor: RootId,
    ) -> Result<Value, JsError> {
        let args = [
            self.embedding_value(object)?,
            self.embedding_value(key)?,
            self.embedding_value(descriptor)?,
        ];
        let program = self.embedding_program()?;
        self.object_define_property(&program, &args)
    }

    pub(crate) fn embedding_own_descriptor(
        &mut self,
        root: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let value = self.embedding_value(root)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.object_get_own_property_descriptor(&program, &[value, key])
    }

    pub(crate) fn embedding_enumerable_own_keys(
        &mut self,
        root: RootId,
        include_symbols: bool,
        include_indices: bool,
    ) -> Result<Vec<RootId>, JsError> {
        self.embedding_filtered_own_keys(root, include_symbols, include_indices, true)
    }

    pub(crate) fn embedding_non_index_own_keys(
        &mut self,
        root: RootId,
        enumerable_only: bool,
    ) -> Result<Vec<RootId>, JsError> {
        self.embedding_filtered_own_keys(root, true, false, enumerable_only)
    }

    fn embedding_filtered_own_keys(
        &mut self,
        root: RootId,
        include_symbols: bool,
        include_indices: bool,
        enumerable_only: bool,
    ) -> Result<Vec<RootId>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let keys = if !include_indices && !matches!(self.heap.get(value), Some(Cell::Proxy { .. }))
        {
            self.ordinary_own_key_values(value, false)
        } else {
            self.object_own_key_values(&program, value)?
        }
        .into_iter()
        .map(|key| self.root(key))
        .collect::<Vec<_>>();
        let mut retained = vec![false; keys.len()];
        let outcome = (|| {
            let mut enumerable = Vec::with_capacity(keys.len());
            for (index, key) in keys.iter().enumerate() {
                let property = self.root_value(*key).expect("own key root is live");
                if !include_indices {
                    if let Some(Cell::String(text)) = self.heap.get(property) {
                        if super::object_static::array_index(text.host_string()).is_some() {
                            continue;
                        }
                    }
                }
                if !include_symbols && matches!(self.heap.get(property), Some(Cell::Symbol(_))) {
                    continue;
                }
                let descriptor =
                    self.object_get_own_property_descriptor(&program, &[value, property])?;
                if !descriptor.is_undefined()
                    && (!enumerable_only || self.descriptor_flag(descriptor, "enumerable"))
                {
                    enumerable.push(*key);
                    retained[index] = true;
                }
            }
            Ok(enumerable)
        })();
        for (key, retained) in keys.into_iter().zip(retained) {
            if outcome.is_err() || !retained {
                self.release_root(key);
            }
        }
        outcome
    }

    pub(crate) fn embedding_has_builtin_brand(
        &self,
        root: RootId,
        brand: crate::BuiltinBrand,
    ) -> Result<bool, JsError> {
        use crate::BuiltinBrand as B;
        let value = self.embedding_value(root)?;
        let cell = self.heap.get(value);
        Ok(match brand {
            B::External => self
                .heap
                .object(value)
                .and_then(Object::external_address)
                .is_some(),
            B::Date => matches!(cell, Some(Cell::Date { .. })),
            B::RegExp => matches!(cell, Some(Cell::RegExp { .. })),
            B::NativeError => self.error_is_error(value) || matches!(cell, Some(Cell::Error(_))),
            B::Promise => self.realm.promise.records.contains_key(&value),
            B::Map => matches!(cell, Some(Cell::Map { .. })),
            B::Set => matches!(cell, Some(Cell::Set { .. })),
            B::WeakMap => matches!(cell, Some(Cell::WeakMap { .. })),
            B::WeakSet => matches!(cell, Some(Cell::WeakSet { .. })),
            B::Proxy => matches!(cell, Some(Cell::Proxy { .. })),
            B::ArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { shared: false, .. })),
            B::SharedArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { shared: true, .. })),
            B::AnyArrayBuffer => matches!(cell, Some(Cell::ArrayBuffer { .. })),
            B::DataView => matches!(cell, Some(Cell::DataView { .. })),
            B::TypedArray => matches!(cell, Some(Cell::TypedArray { .. })),
            B::ArrayBufferView => {
                matches!(cell, Some(Cell::TypedArray { .. } | Cell::DataView { .. }))
            }
            B::ArgumentsObject => self
                .object_data(value)
                .is_some_and(Object::is_arguments_object),
            B::AsyncFunction | B::GeneratorFunction => match cell {
                Some(Cell::Function {
                    kind:
                        FunctionKind::User(program, function)
                        | FunctionKind::NumericUser(program, function),
                    ..
                }) => self
                    .programs
                    .get(*program)
                    .and_then(|program| {
                        program.functions.get(*function as usize).map(|function| {
                            if brand == B::AsyncFunction {
                                function.is_async
                            } else {
                                function.is_generator
                            }
                        })
                    })
                    .unwrap_or(false),
                _ => false,
            },
            B::GeneratorObject => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::Generator | IteratorKind::AsyncGenerator,
                    ..
                })
            ),
            B::MapIterator => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::MapKeys
                        | IteratorKind::MapValues
                        | IteratorKind::MapEntries,
                    ..
                })
            ),
            B::SetIterator => matches!(
                cell,
                Some(Cell::Iterator {
                    kind: IteratorKind::SetValues | IteratorKind::SetEntries,
                    ..
                })
            ),
            B::ModuleNamespaceObject => {
                (value.direct_object_index().is_some()
                    && self
                        .heap
                        .object(value)
                        .is_some_and(Object::is_module_namespace))
                    || matches!(cell, Some(Cell::Object(object)) if object.is_module_namespace())
            }
            B::BooleanObject
            | B::NumberObject
            | B::StringObject
            | B::SymbolObject
            | B::BigIntObject
            | B::BoxedPrimitive => self
                .boxed_primitive_data(value)
                .is_some_and(|(actual, _)| brand == B::BoxedPrimitive || brand == actual),
            typed => {
                let kind = TYPED_BRANDS
                    .iter()
                    .find(|(brand, _)| *brand == typed)
                    .expect("typed brand")
                    .1;
                matches!(cell, Some(Cell::TypedArray { kind: actual, .. }) if *actual == kind)
            }
        })
    }

    pub(crate) fn embedding_strict_equal(
        &self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        Ok(self.strict_equal(self.embedding_value(left)?, self.embedding_value(right)?))
    }

    pub(crate) fn embedding_source_stack_frames(
        &mut self,
        limit: usize,
    ) -> Vec<crate::SourceStackFrame> {
        let records: Vec<_> = self
            .frames
            .iter()
            .rev()
            .filter_map(|frame| {
                self.source_call_site_record(frame, false)
                    .map(|(record, module)| (frame.program, record, module))
            })
            .take(limit)
            .collect();
        records
            .into_iter()
            .map(|(program, mut record, module)| {
                Self::map_call_site_source(&mut self.host, &mut record, module);
                let name = self
                    .call_site_name(&record)
                    .map_or_else(Vec::new, |name| name.units().to_vec());
                let unnamed_script = record.eval_origin.is_some();
                let CallSiteLocation::Source {
                    file_name,
                    line,
                    column,
                } = record.location
                else {
                    unreachable!("source projection");
                };
                crate::SourceStackFrame {
                    function_name: name,
                    script_id: u64::from(self.programs.source_identity(program).raw()) + 1,
                    script_name: if unnamed_script {
                        String::new()
                    } else {
                        file_name
                    },
                    is_eval: unnamed_script,
                    line_number: line,
                    column_number: column,
                }
            })
            .collect()
    }

    pub(crate) fn embedding_error_source_position(
        &mut self,
        error: RootId,
    ) -> Result<Option<crate::ErrorSourcePosition>, JsError> {
        let value = self.embedding_value(error)?;
        // Proxy objects deliberately do not expose their target's captured metadata.
        if let Some(crate::heap::StackData::Captured { records, source }) = self
            .object_data(value)
            .and_then(crate::heap::Object::stack_data)
        {
            if let Some(record) = records
                .iter()
                .find(|record| matches!(record.location, CallSiteLocation::Source { .. }))
            {
                let CallSiteLocation::Source {
                    file_name,
                    line,
                    column,
                } = &record.location
                else {
                    unreachable!()
                };
                return Ok(Some(crate::ErrorSourcePosition {
                    source_line: source
                        .as_ref()
                        .and_then(|source| source.source_line(*line))
                        .map(str::to_owned),
                    script_name: record.eval_origin.is_none().then(|| file_name.clone()),
                    line_number: *line,
                    column_number: *column,
                }));
            }
        }
        let Some((mut record, module, source_line)) = self.frames.iter().rev().find_map(|frame| {
            let (record, module) = self.source_call_site_record(frame, false)?;
            let CallSiteLocation::Source { line, .. } = &record.location else {
                return None;
            };
            let source_line = self
                .programs
                .get(frame.program)?
                .source
                .as_ref()
                .and_then(|source| source.source_line(*line))
                .map(str::to_owned);
            Some((record, module, source_line))
        }) else {
            return Ok(None);
        };
        Self::map_call_site_source(&mut self.host, &mut record, module);
        let CallSiteLocation::Source {
            file_name,
            line,
            column,
        } = record.location
        else {
            unreachable!()
        };
        Ok(Some(crate::ErrorSourcePosition {
            source_line,
            script_name: record.eval_origin.is_none().then_some(file_name),
            line_number: line,
            column_number: column,
        }))
    }

    pub(crate) fn embedding_call_stack_source_names(&self) -> Vec<String> {
        self.frames
            .iter()
            .rev()
            .filter_map(|frame| self.programs.source_name(frame.program).map(str::to_owned))
            .collect()
    }

    pub(crate) fn embedding_instanceof(
        &mut self,
        value: RootId,
        constructor: RootId,
    ) -> Result<bool, JsError> {
        let value = self.embedding_value(value)?;
        let constructor = self.embedding_value(constructor)?;
        let program = self.embedding_program()?;
        self.instanceof(&program, value, constructor)
    }

    pub(crate) fn embedding_proxy_target(&self, root: RootId) -> Result<Option<Value>, JsError> {
        Ok(self
            .embedding_proxy_details(root)?
            .map(|(target, _)| target))
    }

    pub(crate) fn embedding_proxy_details(
        &self,
        root: RootId,
    ) -> Result<Option<(Value, Value)>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::Proxy {
                target, handler, ..
            }) => Some((*target, *handler)),
            _ => None,
        })
    }

    /// Read the same live slots and cursor used by normal collection iteration.
    pub(crate) fn embedding_collection_preview(
        &self,
        root: RootId,
    ) -> Result<Option<crate::CollectionPreview<Value>>, JsError> {
        use crate::CollectionPreview;
        let value = self.embedding_value(root)?;
        let preview = match self.heap.get(value) {
            Some(Cell::Map { entries, .. }) => CollectionPreview {
                entries: entries
                    .iter()
                    .flat_map(|(key, value)| [key, value])
                    .collect(),
                is_key_value: true,
            },
            Some(Cell::Set { entries, .. }) => CollectionPreview {
                entries: entries.to_vec(),
                is_key_value: false,
            },
            Some(Cell::WeakMap { entries, .. }) => CollectionPreview {
                entries: entries
                    .iter()
                    .flat_map(|&(key, value)| [key, value])
                    .collect(),
                is_key_value: true,
            },
            Some(Cell::WeakSet { entries, .. }) => CollectionPreview {
                entries: entries.to_vec(),
                is_key_value: false,
            },
            Some(Cell::Iterator {
                source,
                kind,
                index,
                done,
                ext,
                ..
            }) => {
                let is_key_value = match kind {
                    IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::SetValues => {
                        false
                    }
                    IteratorKind::MapEntries | IteratorKind::SetEntries => true,
                    _ => return Ok(None),
                };
                let mut output = Vec::new();
                let mut position = ext.position(*index);
                if !*done {
                    match self.heap.get(*source) {
                        Some(Cell::Map { entries, .. }) => {
                            while let Some((slot, (key, value))) = entries.next_live(position) {
                                match kind {
                                    IteratorKind::MapKeys => output.push(key),
                                    IteratorKind::MapValues => output.push(value),
                                    IteratorKind::MapEntries => output.extend([key, value]),
                                    _ => unreachable!("map iterator source"),
                                }
                                position = slot + 1;
                            }
                        }
                        Some(Cell::Set { entries, .. }) => {
                            while let Some((slot, value)) = entries.next_live(position) {
                                output.push(value);
                                if is_key_value {
                                    output.push(value);
                                }
                                position = slot + 1;
                            }
                        }
                        _ => {}
                    }
                }
                CollectionPreview {
                    entries: output,
                    is_key_value,
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(preview))
    }

    pub(crate) fn embedding_builtin_data(
        &self,
        root: RootId,
    ) -> Result<Option<crate::BuiltinData<Value>>, JsError> {
        use crate::{BuiltinBrand as B, BuiltinData as D};
        let value = self.embedding_value(root)?;
        // Error branding is owned by object error data, including direct records.
        if self.error_is_error(value) {
            return Ok(Some(D::Error));
        }
        if self.realm.promise.records.contains_key(&value) {
            return Ok(Some(D::Opaque));
        }
        if value.direct_object_index().is_some() {
            return Ok(self
                .boxed_primitive_data(value)
                .map(|(brand, value)| D::Primitive { brand, value }));
        }
        let data = match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => D::Primitive {
                brand: B::Date,
                value: Value::number(*milliseconds),
            },
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                ..
            }) => D::Bytes {
                brand: if *shared {
                    B::SharedArrayBuffer
                } else {
                    B::ArrayBuffer
                },
                bytes: if *detached {
                    Vec::new()
                } else {
                    bytes.to_vec()
                },
            },
            Some(Cell::TypedArray { kind, .. }) => D::Bytes {
                brand: typed_array_brand(*kind),
                bytes: self.embedding_view_bytes(value).unwrap_or_default(),
            },
            Some(Cell::DataView { .. }) => D::Bytes {
                brand: B::DataView,
                bytes: self.embedding_view_bytes(value).unwrap_or_default(),
            },
            Some(Cell::Map { entries, .. }) => D::Map(entries.to_vec()),
            Some(Cell::Set { entries, .. }) => D::Set(entries.to_vec()),
            Some(Cell::RegExp { .. }) => D::RegExp,
            Some(Cell::WeakMap { .. } | Cell::WeakSet { .. }) => D::Opaque,
            Some(Cell::Error(_)) => D::Error,
            Some(Cell::Object(_)) => {
                return Ok(self
                    .boxed_primitive_data(value)
                    .map(|(brand, value)| D::Primitive { brand, value }));
            }
            _ => return Ok(None),
        };
        Ok(Some(data))
    }

    pub(crate) fn embedding_floating_view_values(
        &mut self,
        root: RootId,
    ) -> Result<Option<Vec<f64>>, JsError> {
        let value = self.embedding_value(root)?;
        if !matches!(
            self.heap.get(value),
            Some(Cell::TypedArray {
                kind: TypedArrayKind::Float16 | TypedArrayKind::Float32 | TypedArrayKind::Float64,
                ..
            })
        ) {
            return Ok(None);
        }
        let length = self.typed_array_length(value).unwrap_or(0);
        Ok(Some(
            (0..length)
                .map(|index| {
                    self.typed_array_get(value, index)
                        .and_then(Value::as_number)
                        .unwrap_or(f64::NAN)
                })
                .collect(),
        ))
    }

    pub(crate) fn embedding_is_callable(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.is_function(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_is_object(&self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.is_object_like(value) && !self.is_function(value))
    }

    pub(crate) fn embedding_string_units(&self, root: RootId) -> Result<Option<Vec<u16>>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.units().to_vec()),
            _ => None,
        })
    }

    pub(crate) fn embedding_string_text(&self, root: RootId) -> Result<Option<String>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        })
    }

    pub(crate) fn embedding_detail_string(&self, root: RootId) -> Result<String, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.detail_string(value))
    }

    pub(crate) fn embedding_prevent_extensions(&mut self, object: RootId) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let program = self.embedding_program()?;
        self.prevent_extensions(&program, object)
    }

    pub(crate) fn embedding_has_property(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.has_property(&program, object, key)
    }

    pub(crate) fn embedding_has_own_property(
        &self,
        root: RootId,
        name: &str,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(root)?;
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            return Ok(false);
        }
        let Some(atom) = self.lookup_atom(name) else {
            return Ok(false);
        };
        Ok(self
            .property_attributes(object, super::property_key::PropertyKey::string(atom))
            .is_some())
    }

    pub(crate) fn embedding_is_error(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.error_is_error(self.embedding_value(root)?))
    }

    pub(super) fn detail_string(&self, value: Value) -> String {
        if let Some(number) = value.as_number() {
            return crate::number_to_string::format(number);
        }
        if let Some(boolean) = value.as_bool() {
            return boolean.to_string();
        }
        if value.is_null() {
            return "null".into();
        }
        if value.is_undefined() {
            return "undefined".into();
        }

        let mut value = value;
        while let Some(Cell::Proxy { target, .. }) = self.heap.get(value) {
            if target.is_null() || target.is_undefined() {
                return "[object Proxy]".into();
            }
            value = *target;
        }

        if self
            .heap
            .object_record(value)
            .is_some_and(Object::has_error_data)
        {
            let name = self
                .own_data_string(value, "name")
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Error".into());
            let message = self.own_data_string(value, "message").unwrap_or_default();
            return if message.is_empty() {
                name
            } else {
                format!("{name}: {message}")
            };
        }
        if self.heap.object_record(value).is_some() {
            return "#<Object>".into();
        }
        if self.heap.array(value).is_some() {
            return "[object Array]".into();
        }

        match self.heap.get(value) {
            Some(Cell::String(text)) => text.host_string().to_owned(),
            Some(Cell::BigInt(integer)) => {
                if Self::detail_bigint_is_too_large(integer) {
                    "<a very large BigInt>".into()
                } else {
                    integer.clone()
                }
            }
            Some(Cell::Symbol(description)) => Self::detail_symbol(description.as_deref()),
            Some(Cell::Error(message)) => format!("Error: {message}"),
            Some(Cell::Map { .. }) => "#<Map>".into(),
            Some(Cell::Set { .. }) => "#<Set>".into(),
            Some(cell) => format!("[object {}]", Self::diagnostic_class_name(cell)),
            None => "[object Unknown]".into(),
        }
    }

    fn detail_bigint_is_too_large(integer: &str) -> bool {
        const MAX_V8_DETAIL_BIGINT_LIMBS: usize = 100;

        let digits = integer
            .strip_prefix('-')
            .unwrap_or(integer)
            .trim_start_matches('0');
        if digits.is_empty() || digits.parse::<usize>().is_ok() {
            return false;
        }

        let mut limbs = vec![0usize];
        for byte in digits.bytes() {
            let Some(digit) = byte.checked_sub(b'0').filter(|digit| *digit <= 9) else {
                return false;
            };
            let mut carry = u128::from(digit);
            for limb in &mut limbs {
                let product = (*limb as u128) * 10 + carry;
                *limb = product as usize;
                carry = product >> usize::BITS;
            }
            if carry != 0 {
                limbs.push(carry as usize);
                if limbs.len() > MAX_V8_DETAIL_BIGINT_LIMBS {
                    return true;
                }
            }
        }
        limbs.len() > MAX_V8_DETAIL_BIGINT_LIMBS
    }

    fn detail_symbol(description: Option<&str>) -> String {
        const MAX_V8_DETAIL_SYMBOL_UNITS: usize = 128;
        const V8_DETAIL_SYMBOL_EDGE_UNITS: usize = 56;

        let Some(description) = description else {
            return "Symbol()".into();
        };
        if description.encode_utf16().count() <= MAX_V8_DETAIL_SYMBOL_UNITS {
            return format!("Symbol({description})");
        }
        let units: Vec<_> = description.encode_utf16().collect();
        let prefix = String::from_utf16_lossy(&units[..V8_DETAIL_SYMBOL_EDGE_UNITS]);
        let suffix = String::from_utf16_lossy(&units[units.len() - V8_DETAIL_SYMBOL_EDGE_UNITS..]);
        format!("Symbol({prefix}...<omitted>...{suffix})")
    }

    fn own_data_string(&self, object: Value, name: &str) -> Option<String> {
        let atom = self.lookup_atom(name)?;
        let data = self.object_data(object)?;
        let index = self.shape_slot(data.shape(), atom)?;
        if self
            .property_attributes(object, super::property_key::PropertyKey::string(atom))
            .is_some_and(|attributes| attributes.accessor)
        {
            return None;
        }
        let value = self.heap.property_get(data, index)?;
        match self.heap.get(value) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        }
    }

    fn diagnostic_class_name(cell: &Cell) -> &'static str {
        match cell {
            Cell::Array { .. } => "Array",
            Cell::ArrayBuffer { shared: true, .. } => "SharedArrayBuffer",
            Cell::ArrayBuffer { .. } => "ArrayBuffer",
            Cell::TypedArray { kind, .. } => match kind {
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
            Cell::DataView { .. } => "DataView",
            Cell::Map { .. } => "Map",
            Cell::Set { .. } => "Set",
            Cell::Date { .. } => "Date",
            Cell::RegExp { .. } => "RegExp",
            Cell::Function { .. } => "Function",
            Cell::Error(_) => "Error",
            _ => "Object",
        }
    }

    pub(crate) fn embedding_is_symbol(&self, root: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(root)?;
        Ok(matches!(self.heap.get(value), Some(Cell::Symbol(_))))
    }

    pub(crate) fn embedding_is_promise(&self, root: RootId) -> Result<bool, JsError> {
        self.embedding_has_builtin_brand(root, crate::BuiltinBrand::Promise)
    }

    pub(crate) fn embedding_pending_promise(&mut self) -> Result<Value, JsError> {
        self.promise_object()
    }

    pub(crate) fn embedding_resolve_promise(
        &mut self,
        promise: RootId,
        value: RootId,
    ) -> Result<(), JsError> {
        let promise = self.embedding_value(promise)?;
        let value = self.embedding_value(value)?;
        let state = self
            .realm
            .promise
            .records
            .get(&promise)
            .ok_or_else(|| JsError("value does not have Promise internal slots".into()))?
            .state;
        if state != super::promise::PromiseState::Pending {
            return Ok(());
        }
        let program = self.embedding_program()?;
        self.promise_resolve_value(&program, promise, value)
    }

    pub(crate) fn embedding_reject_promise(
        &mut self,
        promise: RootId,
        reason: RootId,
    ) -> Result<(), JsError> {
        let promise = self.embedding_value(promise)?;
        let reason = self.embedding_value(reason)?;
        if !self.realm.promise.records.contains_key(&promise) {
            return Err(JsError("value does not have Promise internal slots".into()));
        }
        let program = self.embedding_program()?;
        self.promise_settle(
            &program,
            promise,
            super::promise::PromiseState::Rejected,
            reason,
        )
    }

    pub(crate) fn embedding_promise_snapshot(
        &self,
        root: RootId,
    ) -> Result<Option<crate::PromiseSnapshot<Value>>, JsError> {
        use crate::PromiseSnapshot as Snapshot;
        let value = self.embedding_value(root)?;
        Ok(self
            .realm
            .promise
            .records
            .get(&value)
            .map(|record| match record.state {
                super::promise::PromiseState::Pending => Snapshot::Pending,
                super::promise::PromiseState::Fulfilled => Snapshot::Fulfilled(record.result),
                super::promise::PromiseState::Rejected => Snapshot::Rejected(record.result),
            }))
    }

    pub(crate) fn embedding_mark_promise_handled(&mut self, root: RootId) -> Result<(), JsError> {
        let promise = self.embedding_value(root)?;
        if !self.realm.promise.records.contains_key(&promise) {
            return Err(JsError("value does not have Promise internal slots".into()));
        }
        self.observe_promise_rejection(promise);
        Ok(())
    }

    pub(crate) fn embedding_to_string(&mut self, root: RootId) -> Result<String, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.to_string(&program, value)
    }

    pub(crate) fn embedding_to_string_code_units(
        &mut self,
        root: RootId,
    ) -> Result<Vec<u16>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        Ok(self
            .coerce_js_string(&program, value)?
            .units()
            .wide()
            .into_owned())
    }

    pub(crate) fn embedding_to_number(&mut self, root: RootId) -> Result<f64, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        self.to_number(&program, value)
    }

    pub(crate) fn embedding_arithmetic(
        &mut self,
        operation: crate::NativeArithmetic,
        left: RootId,
        right: RootId,
    ) -> Result<Value, JsError> {
        use crate::NativeArithmetic;
        use oxc_ast::ast::BinaryOperator;
        let operation = match operation {
            NativeArithmetic::Add => BinaryOperator::Addition,
            NativeArithmetic::Multiply => BinaryOperator::Multiplication,
            NativeArithmetic::Divide => BinaryOperator::Division,
            NativeArithmetic::BitwiseAnd => BinaryOperator::BitwiseAnd,
        };
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.binary(&program, operation as u32, left, right)
    }

    pub(crate) fn embedding_numeric_update(
        &mut self,
        root: RootId,
        decrement: bool,
    ) -> Result<Value, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let numeric = self.to_numeric(&program, value)?;
        self.with_call_roots(
            [numeric].into_iter().filter(|value| value.is_heap()),
            |vm| vm.numeric_update(&program, numeric, decrement),
        )
    }

    pub(crate) fn embedding_subtract(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<Value, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.binary(
            &program,
            oxc_ast::ast::BinaryOperator::Subtraction as u32,
            left,
            right,
        )
    }

    pub(crate) fn embedding_greater_than(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        self.embedding_relational(
            super::operations::RelationalOperator::GreaterThan,
            left,
            right,
        )
    }

    pub(crate) fn embedding_greater_equal(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        self.embedding_relational(
            super::operations::RelationalOperator::GreaterEqual,
            left,
            right,
        )
    }

    fn embedding_relational(
        &mut self,
        operator: super::operations::RelationalOperator,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        let result = self.binary(&program, operator as u32, left, right)?;
        Ok(self.truthy(result))
    }

    pub(crate) fn embedding_bitwise_or(
        &mut self,
        left: RootId,
        right: RootId,
    ) -> Result<Value, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.binary(
            &program,
            oxc_ast::ast::BinaryOperator::BitwiseOR as u32,
            left,
            right,
        )
    }

    pub(crate) fn embedding_iterator_record(
        &mut self,
        source: RootId,
    ) -> Result<(RootId, RootId), JsError> {
        let source = self.embedding_value(source)?;
        let program = self.embedding_program()?;
        let iterator = self.get_iterator(&program, source)?;
        let iterator = self.root(iterator);
        let value = self.embedding_value(iterator)?;
        let next = self.intern_atom("next");
        match self.get_property(&program, value, next) {
            Ok(next) => Ok((iterator, self.root(next))),
            Err(error) => {
                self.release_root(iterator);
                Err(error)
            }
        }
    }

    pub(crate) fn embedding_iterator_step(
        &mut self,
        iterator: RootId,
        next: RootId,
    ) -> Result<Option<Value>, JsError> {
        self.embedding_value(iterator)?;
        self.embedding_value(next)?;
        let program = self.embedding_program()?;
        self.rooted_iterator_step_value(&program, iterator, next)
    }

    pub(crate) fn embedding_iterator_close(&mut self, iterator: RootId) -> Result<Value, JsError> {
        let value = self.embedding_value(iterator)?;
        let program = self.embedding_program()?;
        self.iterator_close(&program, value)
    }

    pub(crate) fn embedding_truthy(&self, root: RootId) -> Result<bool, JsError> {
        Ok(self.truthy(self.embedding_value(root)?))
    }

    pub(crate) fn embedding_same_value(
        &self,
        left: RootId,
        right: RootId,
    ) -> Result<bool, JsError> {
        Ok(self.same_value(self.embedding_value(left)?, self.embedding_value(right)?))
    }

    pub(crate) fn embedding_equal(&mut self, left: RootId, right: RootId) -> Result<bool, JsError> {
        let left = self.embedding_value(left)?;
        let right = self.embedding_value(right)?;
        let program = self.embedding_program()?;
        self.equal(&program, left, right)
    }

    pub(crate) fn evaluate_embedding_script(
        &mut self,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let previous = std::mem::replace(&mut self.direct_eval, false);
        let result = self.eval_global_script_named(&program, source, false, name);
        self.direct_eval = previous;
        result
    }

    pub(crate) fn parse_embedding_json(&mut self, source: &str) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let source = self.heap.alloc(Cell::String(source.into()));
        self.json_parse(&program, &[source])
    }

    pub(crate) fn stringify_embedding_json(&mut self, root: RootId) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let value = self.embedding_value(root)?;
        self.json_stringify(&program, &[value])
    }

    pub(crate) fn create_embedding_array(&mut self, values: &[RootId]) -> Result<Value, JsError> {
        self.embedding_program()?;
        let elements = self.embedding_arguments(values)?;
        Ok(self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                Rc::new(elements),
            ),
        }))
    }

    pub(crate) fn create_embedding_exception(
        &mut self,
        kind: Native,
        message: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let message = self.heap.alloc(Cell::String(message.into()));
        self.construct_error_native(&program, kind, &[message])
    }

    pub(crate) fn global_root(&mut self) -> Result<RootId, JsError> {
        self.embedding_program()?;
        Ok(self.root(self.realm.globals))
    }

    pub(crate) fn string_rooted(&mut self, text: &str) -> RootId {
        // Host strings mostly spell property keys already interned; such a string shares the
        // atom's storage instead of copying the text.
        let string = match self.lookup_atom(text) {
            Some(atom) => self.atom_value(atom),
            None => JsString::from_str(text),
        };
        let value = self.heap.alloc(Cell::String(string));
        self.root(value)
    }

    pub(crate) fn bigint_rooted(&mut self, integer: i128) -> RootId {
        let value = self.heap.alloc(Cell::BigInt(integer.to_string()));
        self.root(value)
    }

    pub(crate) fn string_units_rooted(&mut self, units: &[u16]) -> RootId {
        let value = self.heap.alloc(Cell::String(JsString::from_units(units)));
        self.root(value)
    }

    pub(crate) fn create_embedding_external(&mut self, address: usize) -> Result<Value, JsError> {
        let value = self.create_embedding_null_object()?;
        self.heap
            .object_mut(value)
            .expect("new native object")
            .set_external_address(address);
        Ok(value)
    }

    pub(crate) fn embedding_external_address(&self, root: RootId) -> Option<usize> {
        let value = self.root_value(root)?;
        self.heap.object(value)?.external_address()
    }

    pub(crate) fn create_embedding_object(&mut self) -> Result<Value, JsError> {
        self.create_embedding_object_with_prototype(self.object_proto)
    }

    pub(crate) fn create_embedding_null_object(&mut self) -> Result<Value, JsError> {
        self.create_embedding_object_with_prototype(Value::NULL)
    }

    pub(crate) fn embedding_symbol(&mut self, description: Option<String>) -> Value {
        self.heap
            .alloc(Cell::Symbol(crate::heap::SymbolData::public(description)))
    }

    pub(crate) fn embedding_private_symbol(&mut self, description: Option<String>) -> Value {
        self.heap
            .alloc(Cell::Symbol(crate::heap::SymbolData::host_private(
                description,
            )))
    }

    fn create_embedding_object_with_prototype(
        &mut self,
        prototype: Value,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        self.call_object_native(&program, Native::ObjectCreate, &[prototype])
    }

    pub(crate) fn embedding_value(&self, root: RootId) -> Result<Value, JsError> {
        self.root_value(root)
            .ok_or_else(|| JsError("released or foreign embedding root".into()))
    }

    pub(crate) fn embedding_program(&self) -> Result<Rc<ResidualProgram>, JsError> {
        self.programs
            .get(self.active_program)
            .ok_or_else(|| JsError("runtime is not initialized".into()))
    }

    pub(crate) fn get_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
    ) -> Result<Value, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let program = self.embedding_program()?;
        self.get_index(&program, object, key)
    }

    pub(crate) fn call_rooted(
        &mut self,
        callee: RootId,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<Value, JsError> {
        let callee = self.embedding_value(callee)?;
        let receiver = self.embedding_value(receiver)?;
        let args = self.embedding_arguments(args)?;
        let program = self.embedding_program()?;
        // A host-initiated call enters the callee's realm, as promise jobs do.
        let realm = self.function_realm(&program, callee)?;
        let previous_global = self.switch_realm_global(realm);
        let result = self.call_value(&program, callee, receiver, &args);
        self.switch_realm_global(previous_global);
        result
    }

    /// Creates a realm and returns its global object.
    pub(crate) fn create_embedding_realm(&mut self) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        self.create_realm_global(&program)
    }

    /// Evaluates host Script source as a global Script of the realm owning `global`.
    pub(crate) fn evaluate_embedding_script_in_realm(
        &mut self,
        global: RootId,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| {
            vm.evaluate_embedding_global_script(source, name)
        })
    }

    pub(crate) fn evaluate_embedding_specialized_script_in_realm_with_origin(
        &mut self,
        global: RootId,
        source: &str,
        name: &str,
        origin: (u32, u32),
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| {
            vm.evaluate_embedding_specialized_script_with_origin(source, name, Some(origin))
        })
    }

    pub(crate) fn prepare_embedding_specialized_script_in_realm_with_origin(
        &mut self,
        global: RootId,
        source: &str,
        name: &str,
        origin: (u32, u32),
    ) -> Result<crate::PreparedScript, JsError> {
        self.with_embedding_realm(global, |vm| {
            vm.prepare_embedding_specialized_script(source, name, Some(origin))
        })
    }

    pub(crate) fn evaluate_embedding_prepared_script_in_realm(
        &mut self,
        global: RootId,
        prepared: &crate::PreparedScript,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_prepared_script(prepared))
    }

    pub(crate) fn compile_embedding_script_in_realm(
        &mut self,
        global: RootId,
        source: &str,
        name: &str,
        origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        self.with_embedding_realm(global, |vm| {
            vm.compile_embedding_host_script(source, name, origin)
        })
    }

    pub(crate) fn evaluate_embedding_compiled_script_in_realm(
        &mut self,
        global: RootId,
        script: &crate::CompiledScript,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_compiled_script(script))
    }

    fn with_embedding_realm<T>(
        &mut self,
        global: RootId,
        run: impl FnOnce(&mut Self) -> Result<T, JsError>,
    ) -> Result<T, JsError> {
        let global = self.embedding_value(global)?;
        if !self.is_realm_global(global) {
            let program = self.embedding_program()?;
            return Err(self.type_error(&program, "value is not a realm global object".into()));
        }
        let previous_global = self.switch_realm_global(global);
        let result = run(self);
        self.switch_realm_global(previous_global);
        result
    }

    fn is_realm_global(&self, value: Value) -> bool {
        value == self.realm.globals
            || self.realm.global_lexical_states.contains_key(&value)
            || self
                .realm
                .intrinsics
                .builtin_prototypes
                .contains_key(&(value, Native::Object))
    }

    pub(crate) fn embedding_enqueue_job(
        &mut self,
        callback: RootId,
        args: &[RootId],
    ) -> Result<(), JsError> {
        let callback = self.embedding_value(callback)?;
        let args = self.embedding_arguments(args)?;
        self.enqueue_job(callback, args);
        Ok(())
    }

    fn embedding_arguments(&self, args: &[RootId]) -> Result<Vec<Value>, JsError> {
        args.iter()
            .map(|root| self.embedding_value(*root))
            .collect()
    }

    pub(crate) fn construct_rooted(
        &mut self,
        callee: RootId,
        new_target: RootId,
        args: &[RootId],
    ) -> Result<Value, JsError> {
        let callee = self.embedding_value(callee)?;
        let new_target = self.embedding_value(new_target)?;
        let args = self.embedding_arguments(args)?;
        let program = self.embedding_program()?;
        self.construct_value_with_new_target(&program, callee, new_target, &args)
    }

    /// Host builtins that perform strict named assignment share the language
    /// operation, including rejected-write diagnostics and Proxy effects.
    pub(crate) fn assign_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<(), JsError> {
        let object = self.embedding_value(object)?;
        let value = self.embedding_value(value)?;
        let atom = self.intern_atom(name);
        let program = self.embedding_program()?;
        self.set_property_with_program_mode(&program, object, atom, value, true)
    }

    pub(crate) fn evaluate_module_sync(
        &mut self,
        module: &crate::ModuleSource,
    ) -> Result<crate::SynchronousModule<Value>, JsError> {
        let program = self.embedding_program()?;
        self.synchronous_module_namespace(&program, module)
    }

    pub(crate) fn evaluate_module_sync_in_realm(
        &mut self,
        global: RootId,
        module: &crate::ModuleSource,
    ) -> Result<crate::SynchronousModule<Value>, JsError> {
        self.with_embedding_realm(global, |vm| {
            let program = vm.embedding_program()?;
            vm.synchronous_module_namespace(&program, module)
        })
    }

    pub(crate) fn module_namespace_with_export_rooted(
        &mut self,
        namespace: RootId,
        name: &str,
        value: RootId,
    ) -> Result<Value, JsError> {
        let namespace = self.embedding_value(namespace)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        self.module_namespace_with_export(&program, namespace, name, value)
    }

    pub(crate) fn set_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        receiver: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let value = self.embedding_value(value)?;
        let receiver = self.embedding_value(receiver)?;
        let program = self.embedding_program()?;
        let accepted = self.call_reflect_native(
            &program,
            Native::ReflectSet,
            &[object, key, value, receiver],
        )?;
        Ok(accepted.as_bool().expect("Reflect.set returns a boolean"))
    }

    pub(crate) fn define_data_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        let key = self.to_property_key(&program, key)?;
        self.define_own_property_record(
            &program,
            object,
            key,
            super::object_descriptors::PropertyDescriptorRecord {
                value: Some(value),
                writable: Some(writable),
                enumerable: Some(enumerable),
                configurable: Some(configurable),
                getter: None,
                setter: None,
            },
        )
    }

    pub(crate) fn materialize_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        name: &str,
    ) -> Result<(), JsError> {
        let holder = self.embedding_value(holder)?;
        let program = self.embedding_program()?;
        let key = PropertyKey::string(self.intern_atom(name));
        self.materialize_lazy_data_property(&program, holder, key)
    }

    pub(crate) fn define_lazy_data_properties_rooted(
        &mut self,
        holder: RootId,
        names: &[&str],
        enumerable: bool,
        initializer: RootId,
    ) -> Result<(), JsError> {
        let holder = self.embedding_value(holder)?;
        let initializer = self.embedding_value(initializer)?;
        let program = self.embedding_program()?;
        let keys = names
            .iter()
            .map(|name| {
                // The key shares the atom table's text.
                let atom = self.intern_atom(name);
                let name = self.heap.alloc(Cell::String(self.atom_value(atom)));
                self.heap.root(name)
            })
            .collect::<Vec<_>>();
        let values = keys
            .iter()
            .map(|&key| self.heap.root_value(key).unwrap())
            .collect::<Vec<_>>();
        let defined =
            self.define_lazy_data_properties(&program, holder, &values, enumerable, initializer);
        for key in keys {
            self.heap.release_root(key);
        }
        defined
    }

    pub(crate) fn set_prototype_rooted(
        &mut self,
        object: RootId,
        prototype: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let prototype = self.embedding_value(prototype)?;
        let program = self.embedding_program()?;
        self.set_prototype_of(&program, object, prototype)
    }
}
