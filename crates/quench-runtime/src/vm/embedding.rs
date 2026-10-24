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

    pub(super) fn boxed_primitive_value(&self, value: Value) -> Option<Value> {
        self.object_data(value)?.boxed_primitive()
    }

    pub(super) fn boxed_primitive_data(&self, value: Value) -> Option<(crate::BuiltinBrand, Value)> {
        let payload = self.boxed_primitive_value(value)?;
        use crate::BuiltinBrand as B;
        let brand = match self.heap.get(payload) {
            _ if self.heap.is_string(payload) => B::StringObject,
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

    pub(crate) fn embedding_collection_epoch(&self) -> u64 {
        self.heap.collection_epoch()
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

    pub(crate) fn embedding_array_like_values(&mut self, root: RootId) -> Result<Vec<RootId>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        let values = self.create_list_from_array_like(
            &program,
            value,
            super::operations::ArrayLikeElementKind::Any,
        )?;
        Ok(values.into_iter().map(|value| self.root(value)).collect())
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

    /// [[GetOwnProperty]] of ToObject(`root`) for ToPropertyKey(`key`), as descriptor fields
    /// rather than a descriptor object. The values are unrooted; the caller roots them before
    /// anything else allocates.
    pub(crate) fn embedding_own_property_fields(
        &mut self,
        root: RootId,
        key: RootId,
    ) -> Result<Option<crate::api::DescriptorFields<Value>>, JsError> {
        let program = self.embedding_program()?;
        let target = self.embedding_value(root)?;
        let target = self.box_object_or_type_error(&program, target)?;
        let target_root = self.heap.root(target);
        let mut key_root = None;
        let outcome = (|| {
            let key = self.embedding_value(key)?;
            let key = self.to_property_key(&program, key)?;
            key_root = Some(self.heap.root(key));
            let record = if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
                let descriptor = self.object_get_own_property_descriptor(&program, &[target, key])?;
                if descriptor.is_undefined() {
                    None
                } else {
                    Some(self.to_property_descriptor(&program, descriptor)?)
                }
            } else {
                self.ordinary_get_own_property(&program, target, key)?
                    .map(|(value, attributes)| {
                        super::object_descriptors::PropertyDescriptorRecord::from_attributes(
                            value, attributes,
                        )
                    })
            };
            Ok(record.map(|record| crate::api::DescriptorFields {
                value: record.value,
                writable: record.writable,
                get: record.getter,
                set: record.setter,
                enumerable: record.enumerable,
                configurable: record.configurable,
            }))
        })();
        for root in [Some(target_root), key_root].into_iter().flatten() {
            self.heap.release_root(root);
        }
        outcome
    }

    /// `Reflect.defineProperty(object, key, descriptor)` for a descriptor given as its fields:
    /// whether [[DefineOwnProperty]] succeeded. The fields' values stay rooted by the caller.
    pub(crate) fn embedding_define_property_fields(
        &mut self,
        object: RootId,
        key: RootId,
        fields: crate::api::DescriptorFields<Value>,
    ) -> Result<bool, JsError> {
        let program = self.embedding_program()?;
        let target = self.embedding_value(object)?;
        if !self.is_object_like(target) {
            return Err(self.type_error(&program, "Reflect.defineProperty called on non-object".into()));
        }
        let record = super::object_descriptors::PropertyDescriptorRecord {
            value: fields.value,
            writable: fields.writable,
            enumerable: fields.enumerable,
            configurable: fields.configurable,
            getter: fields.get,
            setter: fields.set,
        };
        // ToPropertyDescriptor's checks, which a descriptor object would have passed.
        if [record.getter, record.setter]
            .into_iter()
            .flatten()
            .any(|accessor| !accessor.is_undefined() && !self.is_function(accessor))
        {
            return Err(self.type_error(&program, "Accessor descriptor must be callable".into()));
        }
        if record.has_accessor_fields() && record.has_data_fields() {
            return Err(self.type_error(
                &program,
                "Property descriptor cannot mix accessor and data fields".into(),
            ));
        }
        let key = self.embedding_value(key)?;
        let key = self.to_property_key(&program, key)?;
        self.define_own_property_record(&program, target, key, record)
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

    pub(crate) fn embedding_non_index_own_keys(&mut self, root: RootId, enumerable_only: bool) -> Result<Vec<RootId>, JsError> {
        self.embedding_filtered_own_keys(root, true, false, enumerable_only)
    }

    fn embedding_filtered_own_keys(&mut self, root: RootId, include_symbols: bool, include_indices: bool, enumerable_only: bool) -> Result<Vec<RootId>, JsError> {
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
                    if let Some(text) = self.heap.string(property) {
                        if super::object_static::array_index(text.host_string()).is_some() {
                            continue;
                        }
                    }
                }
                if !include_symbols && matches!(self.heap.get(property), Some(Cell::Symbol(_))) {
                    continue;
                }
                // As `Object.keys` asks: a pending lazy data property answers from its
                // attributes without materializing, and a Proxy through its descriptor trap.
                let found = self.own_property_enumerable(&program, value, property)?;
                if found.is_some_and(|enumerable| enumerable || !enumerable_only) {
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
            B::External => self.heap.object(value).and_then(Object::external_address).is_some(),
            B::Date => matches!(cell, Some(Cell::Date { .. })),
            B::RegExp => matches!(cell, Some(Cell::RegExp { .. })),
            B::NativeError => self.error_is_error(value) || matches!(cell, Some(Cell::Error(_))),
            B::Promise => self.is_promise(value),
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
            B::AsyncFunction | B::GeneratorFunction => match self.heap.function(value) {
                Some(FunctionRecord {
                    kind:
                        FunctionKind::User(program, function),
                    ..
                }) => self
                    .programs
                    .get(*program)
                    .and_then(|program| {
                        program.functions.get(*function as usize).map(|function| {
                            let header = function.header();
                            if brand == B::AsyncFunction {
                                header.is_async
                            } else {
                                header.is_generator
                            }
                        })
                    })
                    .unwrap_or(false),
                _ => false,
            },
            B::GeneratorObject => matches!(
                self.heap.iterator(value),
                Some(IteratorRecord {
                    kind: IteratorKind::Generator | IteratorKind::AsyncGenerator,
                    ..
                })
            ),
            B::MapIterator => matches!(
                self.heap.iterator(value),
                Some(IteratorRecord {
                    kind: IteratorKind::MapKeys
                        | IteratorKind::MapValues
                        | IteratorKind::MapEntries,
                    ..
                })
            ),
            B::SetIterator => matches!(
                self.heap.iterator(value),
                Some(IteratorRecord {
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
            | B::BoxedPrimitive => {
                self.boxed_primitive_data(value)
                    .is_some_and(|(actual, _)| brand == B::BoxedPrimitive || brand == actual)
            }
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

    pub(crate) fn embedding_source_stack_frames(&mut self, limit: usize) -> Vec<crate::SourceStackFrame> {
        self.materialize_deferred_lane();
        let records: Vec<_> = self.frames.iter().rev().filter_map(|frame| {
            self.source_call_site_record(frame, false).map(|(record, module)| (frame.program, record, module))
        }).take(limit).collect();
        records.into_iter().map(|(program, mut record, module)| {
            Self::map_call_site_source(&mut self.host, &mut record, module);
            let name = self.call_site_name(&record).map_or_else(Vec::new, |name| name.units().to_vec());
            let unnamed_script = record.eval_origin.is_some();
            let CallSiteLocation::Source { file_name, line, column } = record.location else { unreachable!("source projection"); };
            crate::SourceStackFrame {
                function_name: name,
                script_id: u64::from(self.programs.source_identity(program).raw()) + 1,
                script_name: if unnamed_script { String::new() } else { file_name },
                is_eval: unnamed_script,
                line_number: line,
                column_number: column,
            }
        }).collect()
    }

    pub(crate) fn embedding_error_source_position(
        &mut self,
        error: RootId,
    ) -> Result<Option<crate::ErrorSourcePosition>, JsError> {
        let value = self.embedding_value(error)?;
        // Proxy objects deliberately do not expose their target's captured metadata.
        if let Some(crate::heap::StackData::Captured { records, source }) =
            self.object_data(value).and_then(crate::heap::Object::stack_data)
        {
            if let Some(record) = records.iter().find(|record| matches!(record.location, CallSiteLocation::Source { .. })) {
                let CallSiteLocation::Source { file_name, line, column } = &record.location else { unreachable!() };
                return Ok(Some(crate::ErrorSourcePosition {
                    source_line: source.as_ref().and_then(|source| source.source_line(*line)).map(str::to_owned),
                    script_name: record.eval_origin.is_none().then(|| file_name.clone()),
                    line_number: *line,
                    column_number: *column,
                }));
            }
        }
        let Some((mut record, module, source_line)) = self.frames.iter().rev().find_map(|frame| {
            let (record, module) = self.source_call_site_record(frame, false)?;
            let CallSiteLocation::Source { line, .. } = &record.location else { return None };
            let source_line = self.programs.get(frame.program)?.source.as_ref()
                .and_then(|source| source.source_line(*line)).map(str::to_owned);
            Some((record, module, source_line))
        }) else { return Ok(None) };
        Self::map_call_site_source(&mut self.host, &mut record, module);
        let CallSiteLocation::Source { file_name, line, column } = record.location else { unreachable!() };
        Ok(Some(crate::ErrorSourcePosition {
            source_line,
            script_name: record.eval_origin.is_none().then_some(file_name),
            line_number: line,
            column_number: column,
        }))
    }

    pub(crate) fn embedding_call_stack_source_names(&self) -> Vec<String> {
        self.frames.iter().rev().filter_map(|frame| {
            self.programs.source_name(frame.program).map(str::to_owned)
        }).collect()
    }

    pub(crate) fn embedding_instanceof(&mut self, value: RootId, constructor: RootId) -> Result<bool, JsError> {
        let value = self.embedding_value(value)?;
        let constructor = self.embedding_value(constructor)?;
        let program = self.embedding_program()?;
        self.instanceof(&program, value, constructor)
    }

    pub(crate) fn embedding_proxy_target(&self, root: RootId) -> Result<Option<Value>, JsError> {
        Ok(self.embedding_proxy_details(root)?.map(|(target, _)| target))
    }

    pub(crate) fn embedding_proxy_details(&self, root: RootId) -> Result<Option<(Value, Value)>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(match self.heap.get(value) {
            Some(Cell::Proxy { target, handler, .. }) => Some((*target, *handler)),
            _ => None,
        })
    }

    /// Read the same live slots and cursor used by normal collection iteration.
    pub(crate) fn embedding_collection_preview(&self, root: RootId) -> Result<Option<crate::CollectionPreview<Value>>, JsError> {
        use crate::CollectionPreview;
        let value = self.embedding_value(root)?;
        let preview = match (self.heap.iterator(value), self.heap.get(value)) {
            (_, Some(Cell::Map { entries, .. })) => CollectionPreview {
                entries: entries.iter().flat_map(|(key, value)| [key, value]).collect(), is_key_value: true,
            },
            (_, Some(Cell::Set { entries, .. })) => CollectionPreview { entries: entries.to_vec(), is_key_value: false },
            (_, Some(Cell::WeakMap { entries, .. })) => CollectionPreview {
                entries: entries.iter().flat_map(|&(key, value)| [key, value]).collect(), is_key_value: true,
            },
            (_, Some(Cell::WeakSet { entries, .. })) => CollectionPreview { entries: entries.to_vec(), is_key_value: false },
            (Some(IteratorRecord { source, kind, index, done, ext, .. }), _) => {
                let is_key_value = match kind {
                    IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::SetValues => false,
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
                                if is_key_value { output.push(value); }
                                position = slot + 1;
                            }
                        }
                        _ => {},
                    }
                }
                CollectionPreview { entries: output, is_key_value }
            }
            (_, _) => return Ok(None),
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
        if self.is_promise(value) {
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
        Ok(self.heap.string(value).map(|text| text.units().to_vec()))
    }

    pub(crate) fn embedding_string_text(&self, root: RootId) -> Result<Option<String>, JsError> {
        let value = self.embedding_value(root)?;
        Ok(self.heap.string(value).map(|text| text.host_string().to_owned()))
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

        if let Some(text) = self.heap.string(value) {
            return text.host_string().to_owned();
        }
        match self.heap.get(value) {
            Some(Cell::BigInt(integer)) => {
                if Self::detail_bigint_is_too_large(integer) {
                    "<a very large BigInt>".into()
                } else {
                    integer.to_string()
                }
            }
            Some(Cell::Symbol(description)) => Self::detail_symbol(description.as_deref()),
            Some(Cell::Error(message)) => format!("Error: {message}"),
            Some(Cell::Map { .. }) => "#<Map>".into(),
            Some(Cell::Set { .. }) => "#<Set>".into(),
            Some(cell) => format!("[object {}]", Self::diagnostic_class_name(cell)),
            None if self.heap.is_function_value(value) => "[object Function]".into(),
            None => "[object Unknown]".into(),
        }
    }

    fn detail_bigint_is_too_large(integer: &num_bigint::BigInt) -> bool {
        const MAX_V8_DETAIL_BIGINT_LIMBS: u64 = 100;
        integer.bits() > MAX_V8_DETAIL_BIGINT_LIMBS * u64::from(usize::BITS)
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
        self.heap.string(value).map(|text| text.host_string().to_owned())
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
            Cell::Function(_) => "Function",
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

    pub(crate) fn embedding_resolve_promise(&mut self, promise: RootId, value: RootId) -> Result<(), JsError> {
        let promise = self.embedding_value(promise)?;
        let value = self.embedding_value(value)?;
        let state = self.promise_record(promise)
            .ok_or_else(|| JsError("value does not have Promise internal slots".into()))?.state;
        if state != super::promise::PromiseState::Pending { return Ok(()); }
        let program = self.embedding_program()?;
        self.promise_resolve_value(&program, promise, value)
    }

    pub(crate) fn embedding_reject_promise(&mut self, promise: RootId, reason: RootId) -> Result<(), JsError> {
        let promise = self.embedding_value(promise)?;
        let reason = self.embedding_value(reason)?;
        if !self.is_promise(promise) {
            return Err(JsError("value does not have Promise internal slots".into()));
        }
        let program = self.embedding_program()?;
        self.promise_settle(&program, promise, super::promise::PromiseState::Rejected, reason)
    }

    pub(crate) fn embedding_promise_snapshot(&self, root: RootId) -> Result<Option<crate::PromiseSnapshot<Value>>, JsError> {
        use crate::PromiseSnapshot as Snapshot;
        let value = self.embedding_value(root)?;
        Ok(self.promise_record(value).map(|record| match record.state {
            super::promise::PromiseState::Pending => Snapshot::Pending,
            super::promise::PromiseState::Fulfilled => Snapshot::Fulfilled(record.result),
            super::promise::PromiseState::Rejected => Snapshot::Rejected(record.result),
        }))
    }

    pub(crate) fn embedding_mark_promise_handled(&mut self, root: RootId) -> Result<(), JsError> {
        let promise = self.embedding_value(root)?;
        if !self.is_promise(promise) {
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

    pub(crate) fn embedding_to_string_code_units(&mut self, root: RootId) -> Result<Vec<u16>, JsError> {
        let value = self.embedding_value(root)?;
        let program = self.embedding_program()?;
        Ok(self.coerce_js_string(&program, value)?.units().wide().into_owned())
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
        let next = self.runtime_atoms.next;
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
        let result = self.eval_host_script(&program, source, name);
        self.direct_eval = previous;
        result
    }

    pub(crate) fn parse_embedding_json(&mut self, source: &str) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let source = self.heap.alloc_string(source.into());
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
                elements,
            ),
        }))
    }

    pub(crate) fn create_embedding_exception(
        &mut self,
        kind: Native,
        message: &str,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let message = self.heap.alloc_string(message.into());
        self.construct_error_native(&program, kind, &[message])
    }

    pub(crate) fn global_root(&mut self) -> Result<RootId, JsError> {
        self.embedding_program()?;
        Ok(self.root(self.realm.globals))
    }

    pub(crate) fn string_rooted(&mut self, text: &str) -> RootId {
        // Host strings mostly spell property keys already interned; such a string is the
        // atom's shared string value (strings have no identity), else a fresh copy.
        let value = match self.lookup_atom(text) {
            Some(atom) => self.atom_string(atom),
            None => self.heap.alloc_string(JsString::from_str(text)),
        };
        self.root(value)
    }

    pub(crate) fn decoded_string_rooted(&mut self, text: &str) -> RootId {
        let value = self.heap.alloc_string(JsString::decoded(text));
        self.root(value)
    }

    pub(crate) fn bigint_rooted(&mut self, integer: i128) -> RootId {
        let value = self.heap.alloc(Cell::bigint(integer.into()));
        self.root(value)
    }

    pub(crate) fn string_units_rooted(&mut self, units: &[u16]) -> RootId {
        let value = self.heap.alloc_string(JsString::from_units(units));
        self.root(value)
    }

    pub(crate) fn create_embedding_external(&mut self, address: usize) -> Result<Value, JsError> {
        let value = self.create_embedding_null_object()?;
        self.heap.object_mut(value).expect("new native object").set_external_address(address);
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
        self.heap.alloc(Cell::Symbol(crate::heap::SymbolData::public(description)))
    }

    pub(crate) fn embedding_private_symbol(&mut self, description: Option<String>) -> Value {
        self.heap.alloc(Cell::Symbol(crate::heap::SymbolData::host_private(description)))
    }

    /// The ordinary object `{ [names[0]]: values[0], ... }` of the realm's
    /// `Object.prototype`, whose own data properties are writable, enumerable and
    /// configurable in `names` order, as a literal makes it: allocated at its final shape,
    /// which the VM derives once per `names` list (`embedding_record_shapes`). `names` hold
    /// neither an index name nor a duplicate.
    pub(crate) fn create_embedding_record(
        &mut self,
        names: &'static [&'static str],
        values: &[RootId],
    ) -> Result<Value, JsError> {
        debug_assert_eq!(names.len(), values.len());
        let shape = self.embedding_record_shape(names);
        let values = values
            .iter()
            .map(|root| self.embedding_value(*root))
            .collect::<Result<smallvec::SmallVec<[Value; crate::value_vec::INLINE_PROPERTY_COUNT]>, _>>()?;
        let (storage, inline) = self.heap.initial_object_storage(shape, &values, true);
        let object = crate::heap::Object::with_property_storage(self.object_proto, storage, inline);
        Ok(self.heap.alloc_object(object))
    }

    /// The shape adding `names` in order to an empty ordinary object reaches, memoized by
    /// the list's address: a `'static` list is never freed, so its address names it.
    fn embedding_record_shape(&mut self, names: &'static [&'static str]) -> u32 {
        let key = names.as_ptr() as usize;
        if let Some(shape) = self.embedding_record_shapes.get(&key) {
            return *shape;
        }
        let mut shape = Self::empty_object(self.object_proto).shape();
        for name in names {
            debug_assert!(super::object_static::array_index(name).is_none());
            let atom = self.intern_atom(name);
            shape = self.transition_data_property_shape(
                shape,
                super::property_key::PropertyKey::string(atom),
                super::DEFAULT_DATA_ATTRIBUTES,
            );
        }
        self.embedding_record_shapes.insert(key, shape);
        shape
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

    /// `object[name]` for a property key given as Rust text, without allocating the key
    /// string: an array-index name keeps `get_index`'s element path, and any other name is
    /// the interned atom `get_index` would resolve.
    pub(crate) fn get_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
    ) -> Result<Value, JsError> {
        let object = self.embedding_value(object)?;
        let program = self.embedding_program()?;
        if super::object_static::array_index(name).is_some() {
            let key = self.heap.alloc_string(JsString::from_str(name));
            return self.get_index(&program, object, key);
        }
        let atom = self.intern_atom(name);
        self.get_property(&program, object, atom)
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
        &mut self, global: RootId, source: &str, name: &str,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_global_script(source, name))
    }

    pub(crate) fn evaluate_embedding_specialized_script_in_realm_with_origin(
        &mut self, global: RootId, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_specialized_script_with_origin(source, name, Some(origin)))
    }

    pub(crate) fn prepare_embedding_specialized_script_in_realm_with_origin(
        &mut self, global: RootId, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<crate::PreparedScript, JsError> {
        self.with_embedding_realm(global, |vm| vm.prepare_embedding_specialized_script(source, name, Some(origin)))
    }

    pub(crate) fn evaluate_embedding_prepared_script_in_realm(
        &mut self, global: RootId, prepared: &crate::PreparedScript,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_prepared_script(prepared))
    }

    pub(crate) fn compile_embedding_script_in_realm(
        &mut self, global: RootId, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        self.with_embedding_realm(global, |vm| vm.compile_embedding_host_script(source, name, origin))
    }

    pub(crate) fn evaluate_embedding_compiled_script_in_realm(
        &mut self, global: RootId, script: &crate::CompiledScript,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_compiled_script(script))
    }

    fn with_embedding_realm<T>(
        &mut self, global: RootId, run: impl FnOnce(&mut Self) -> Result<T, JsError>,
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

    /// Whether `value` is a realm's global object, live or parked.
    pub(super) fn is_global_object(&self, value: Value) -> bool {
        value == self.realm.globals
            || self.realm.global_lexical_states.contains_key(&value)
            || self.realm.parked.as_ref().is_some_and(|parked| parked.global == value)
    }

    fn is_realm_global(&self, value: Value) -> bool {
        self.is_global_object(value)
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
        let program = self.embedding_program()?;
        if super::object_static::array_index(name).is_some() {
            let key = self.heap.alloc_string(JsString::from_str(name));
            return self.set_index_mode(&program, object, key, value, true);
        }
        let atom = self.intern_atom(name);
        self.set_property_with_program_mode(&program, object, atom, value, true)
    }

    /// Strict-mode `object[key] = value` for a string or symbol key, as the guest's
    /// `SetIndex` performs it.
    pub(crate) fn assign_property_rooted(
        &mut self,
        object: RootId,
        key: RootId,
        value: RootId,
    ) -> Result<(), JsError> {
        let object = self.embedding_value(object)?;
        let key = self.embedding_value(key)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        self.set_index_mode(&program, object, key, value, true)
    }

    pub(crate) fn evaluate_module_sync(
        &mut self,
        module: &crate::ModuleSource,
    ) -> Result<crate::SynchronousModule<Value>, JsError> {
        let program = self.embedding_program()?;
        self.synchronous_module_namespace(&program, module)
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

    /// `Reflect.set(object, name, value)` for a string key the host spells, without
    /// allocating the key: the same `[[Set]]` that `Reflect.set` makes after `ToPropertyKey`.
    pub(crate) fn set_named_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        if !self.is_object_like(object) {
            return Err(self.type_error(&program, "Reflect target is not an object".into()));
        }
        let atom = self.intern_atom(name);
        self.set_property_with_receiver(&program, object, atom, value, object)
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

    pub(crate) fn pending_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        key: RootId,
        listener: RootId,
        index: u32,
    ) -> Result<Option<Value>, JsError> {
        let holder = self.embedding_value(holder)?;
        let key = self.embedding_value(key)?;
        let listener = self.embedding_value(listener)?;
        // Lazy data properties have string keys.
        let Some(name) = self.heap.string(key).cloned() else {
            return Ok(None);
        };
        let key = PropertyKey::string(self.intern_js_atom(&name));
        Ok(self.pending_lazy_data_property(holder, key, listener, index))
    }

    /// `define_data_property_rooted` for a string key the host spells, without allocating
    /// the key where it can: an absent key of an extensible plain object is a shape addition
    /// with the given attributes, exactly as `[[DefineOwnProperty]]` adds it.
    pub(crate) fn define_named_data_property_rooted(
        &mut self,
        object: RootId,
        name: &str,
        value: RootId,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<bool, JsError> {
        let object = self.embedding_value(object)?;
        let value = self.embedding_value(value)?;
        let program = self.embedding_program()?;
        let atom = self.intern_atom(name);
        let key = super::property_key::PropertyKey::string(atom);
        let absent_plain = self.plain_named_key(atom)
            && !self.descriptors.contains_key(&(object, key))
            && self.heap.ordinary_object(object).is_some_and(|data| {
                data.is_extensible()
                    && !data.is_module_namespace()
                    && data.deferred_module().is_none()
                    && self.property_shape_slot(data.shape(), key).is_none()
            });
        if absent_plain {
            let attributes = super::PropertyAttributes {
                writable,
                enumerable,
                configurable,
                accessor: false,
                getter: None,
                setter: None,
            };
            self.set_property_with_attributes(object, atom, value, attributes)?;
            return Ok(true);
        }
        let key = self.atom_string(atom);
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

    pub(crate) fn is_pending_lazy_data_property_rooted(
        &mut self,
        holder: RootId,
        name: &str,
    ) -> Result<bool, JsError> {
        let holder = self.embedding_value(holder)?;
        let key = PropertyKey::string(self.intern_atom(name));
        Ok(self.is_pending_lazy_data_property(holder, key))
    }

    pub(crate) fn define_lazy_data_properties_rooted(
        &mut self,
        holder: RootId,
        names: &[&str],
        enumerable: bool,
        initializer: RootId,
    ) -> Result<(), JsError> {
        let members = names.iter().map(|name| (*name, enumerable)).collect::<Vec<_>>();
        self.define_lazy_group_rooted(holder, &members, false, initializer)
    }

    pub(crate) fn define_lazy_group_rooted(
        &mut self,
        holder: RootId,
        members: &[(&str, bool)],
        completes_keys: bool,
        initializer: RootId,
    ) -> Result<(), JsError> {
        let holder = self.embedding_value(holder)?;
        let initializer = self.embedding_value(initializer)?;
        let program = self.embedding_program()?;
        let keys = members
            .iter()
            .map(|(name, _)| {
                // The key shares the atom table's text.
                let atom = self.intern_atom(name);
                let name = self.heap.alloc_string(self.atom_value(atom));
                self.heap.root(name)
            })
            .collect::<Vec<_>>();
        let values = keys
            .iter()
            .zip(members)
            .map(
                |(&key, &(_, enumerable))| super::lazy_data_property::LazyMember {
                    key: self.heap.root_value(key).unwrap(),
                    enumerable,
                    redefined: false,
                },
            )
            .collect::<Vec<_>>();
        let defined = self.define_lazy_data_properties(
            &program,
            holder,
            &values,
            completes_keys,
            initializer,
        );
        for key in keys {
            self.heap.release_root(key);
        }
        defined
    }

    /// Defines `members` (a rooted string or symbol key, whether it is enumerable while
    /// pending, and whether `initializer` defines it itself) on `holder` as one lazy group
    /// (`LazyMember`).
    pub(crate) fn define_lazy_members_rooted(
        &mut self,
        holder: RootId,
        members: &[(RootId, bool, bool)],
        initializer: RootId,
    ) -> Result<(), JsError> {
        let holder = self.embedding_value(holder)?;
        let initializer = self.embedding_value(initializer)?;
        let program = self.embedding_program()?;
        let members = members
            .iter()
            .map(|&(key, enumerable, redefined)| {
                Ok(super::lazy_data_property::LazyMember {
                    key: self.embedding_value(key)?,
                    enumerable,
                    redefined,
                })
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        // An initializer that redefines members decides the holder's final keys, so listing
        // them with symbols or fixing the holder's integrity materializes the group first.
        let completes_keys = members.iter().any(|member| member.redefined);
        self.define_lazy_data_properties(&program, holder, &members, completes_keys, initializer)
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
