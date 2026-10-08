use super::*;

pub(super) enum ArrayInitialization {
    Repeated { count: u32, value: Value },
    Default(u32),
    Fixed(Vec<Value>),
}

pub(super) enum GcFieldIndex {
    Struct(u32),
    Array(u32),
}

enum ArraySegmentValues {
    Data {
        bytes: Option<std::rc::Rc<Vec<u8>>>,
        load: crate::wasm::memory::MemoryLoad,
        range: std::ops::Range<usize>,
    },
    Elements {
        source: Value,
        range: std::ops::Range<usize>,
    },
}
impl ArraySegmentValues {
    fn len(&self) -> usize {
        match self {
            Self::Data { load, range, .. } => range.len() / load.width(),
            Self::Elements { range, .. } => range.len(),
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn wasm_struct_new(
        &mut self,
        declarations: &crate::WasmTypes,
        index: u32,
        values: Option<Vec<Value>>,
        descriptor: Option<Value>,
    ) -> Result<Value, JsError> {
        let declarations = declarations.clone();
        let fields = declarations
            .struct_fields(index)
            .ok_or_else(|| JsError::validation("invalid Wasm struct declaration".into()))?;
        match (declarations.descriptor_type(index), descriptor) {
            (None, None) => {}
            (Some(ty), Some(value)) => {
                if value.is_null() {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::NullDescriptorReference,
                    ));
                }
                if !self.wasm_reference_valid_in(value, ty, Some(&declarations)) {
                    return Err(JsError::validation("invalid Wasm struct descriptor".into()));
                }
            }
            _ => {
                return Err(JsError::validation(
                    "Wasm struct constructor descriptor mismatch".into(),
                ));
            }
        }
        let mut values = match values {
            Some(values) if values.len() == fields.len() => values,
            Some(_) => {
                return Err(JsError::validation(
                    "Wasm struct field count mismatch".into(),
                ));
            }
            None => {
                let mut values = Vec::new();
                values
                    .try_reserve_exact(fields.len())
                    .map_err(|_| JsError::validation("Wasm struct allocation failed".into()))?;
                for field in fields {
                    let value = self.wasm_storage_default(field.element_type, &declarations)?;
                    values.push(value);
                }
                values
            }
        };
        for (value, field) in values.iter_mut().zip(fields) {
            *value = self.wasm_storage_value(*value, field.element_type, &declarations)?;
        }
        Ok(self.heap.alloc(Cell::WasmGc {
            declarations,
            ty: index,
            fields: values,
            descriptor,
        }))
    }
    pub(super) fn wasm_descriptor_matches(
        &self,
        owner: &crate::WasmTypes,
        target: crate::wasm::reference::ReferenceTarget,
        reference: Value,
        descriptor: Value,
    ) -> Result<bool, JsError> {
        let ty = target
            .descriptor_type(owner)
            .ok_or_else(|| JsError::validation("Wasm cast target has no descriptor".into()))?;
        if descriptor.is_null() {
            return Err(JsError::wasm_trap_error(
                crate::WasmTrap::NullDescriptorReference,
            ));
        }
        if !self.wasm_reference_valid_in(descriptor, ty, Some(owner)) {
            return Err(JsError::validation("invalid Wasm cast descriptor".into()));
        }
        if reference.is_null() {
            return Ok(target.reference_type().unwrap().is_nullable());
        }
        Ok(
            matches!(self.heap.get(reference), Some(Cell::WasmGc { descriptor: Some(actual), .. }) if *actual == descriptor),
        )
    }

    pub(super) fn wasm_ref_get_desc(
        &self,
        owner: &crate::WasmTypes,
        index: u32,
        reference: Value,
    ) -> Result<Value, JsError> {
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::NullReference));
        }
        let ty = crate::WasmType::Reference {
            kind: crate::WasmReferenceKind::DeclaredGc {
                index,
                exact: false,
            },
            nullable: false,
        };
        if owner.descriptor_type(index).is_none()
            || !self.wasm_reference_valid_in(reference, ty, Some(owner))
        {
            return Err(JsError::validation(
                "invalid Wasm described struct reference".into(),
            ));
        }
        match self.heap.get(reference) {
            Some(Cell::WasmGc {
                descriptor: Some(descriptor),
                ..
            }) => Ok(*descriptor),
            _ => Err(JsError::validation("Wasm struct has no descriptor".into())),
        }
    }

    fn wasm_storage_default(
        &mut self,
        storage: wasmparser::StorageType,
        declarations: &crate::WasmTypes,
    ) -> Result<Value, JsError> {
        Ok(match storage {
            wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => Value::integer(0),
            wasmparser::StorageType::Val(ty) => {
                let ty = declarations
                    .callable_value_type(ty)
                    .ok_or_else(|| JsError::validation("unsupported Wasm struct field".into()))?;
                let value = ty.default_value().ok_or_else(|| {
                    JsError::validation("non-defaultable Wasm struct field".into())
                })?;
                self.encode_wasm_value(value)
            }
        })
    }
    fn wasm_storage_value(
        &self,
        value: Value,
        storage: wasmparser::StorageType,
        declarations: &crate::WasmTypes,
    ) -> Result<Value, JsError> {
        match storage {
            wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => {
                let bits = value
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid packed Wasm field".into()))?;
                let mask = if storage == wasmparser::StorageType::I8 {
                    i32::from(u8::MAX)
                } else {
                    i32::from(u16::MAX)
                };
                Ok(Value::integer(bits & mask))
            }
            wasmparser::StorageType::Val(ty) => {
                let ty = declarations
                    .callable_value_type(ty)
                    .ok_or_else(|| JsError::validation("unsupported Wasm field".into()))?;
                self.decode_wasm_value_in(value, ty, Some(declarations))?;
                Ok(value)
            }
        }
    }

    pub(super) fn wasm_gc_field(
        &mut self,
        reference: Value,
        location: GcFieldIndex,
        access: crate::wasm::gc::GcFieldAccess,
        value: Option<Value>,
    ) -> Result<Value, JsError> {
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(match location {
                GcFieldIndex::Struct(_) => crate::WasmTrap::NullStructReference,
                GcFieldIndex::Array(_) => crate::WasmTrap::NullArrayReference,
            }));
        }
        let Some(Cell::WasmGc {
            declarations,
            ty,
            fields,
            ..
        }) = self.heap.get(reference)
        else {
            return Err(JsError::validation(
                "invalid Wasm aggregate reference".into(),
            ));
        };
        let (index, field) = match location {
            GcFieldIndex::Struct(index) => (
                index,
                declarations
                    .struct_fields(*ty)
                    .and_then(|fields| fields.get(index as usize))
                    .copied(),
            ),
            GcFieldIndex::Array(index) => (index, declarations.array_field(*ty)),
        };
        let field = field
            .filter(|field| access.supports(*field))
            .ok_or_else(|| JsError::validation("invalid Wasm aggregate field access".into()))?;
        let stored = fields
            .get(index as usize)
            .copied()
            .ok_or_else(|| match location {
                GcFieldIndex::Array(_) => {
                    JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray)
                }
                GcFieldIndex::Struct(_) => {
                    JsError::validation("invalid Wasm struct field storage".into())
                }
            })?;
        if access != crate::wasm::gc::GcFieldAccess::Write {
            return access.read(field.element_type, stored).ok_or_else(|| {
                JsError::validation("invalid Wasm struct field representation".into())
            });
        }
        let value = self.wasm_storage_value(
            value.ok_or_else(|| JsError::validation("missing Wasm struct field value".into()))?,
            field.element_type,
            declarations,
        )?;
        let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(reference) else {
            unreachable!("validated live struct")
        };
        fields[index as usize] = value;
        Ok(Value::UNDEFINED)
    }
    pub(super) fn wasm_array_new(
        &mut self,
        declarations: &crate::WasmTypes,
        index: u32,
        initialization: ArrayInitialization,
    ) -> Result<Value, JsError> {
        // Bound one dense Value payload to 64 MiB: unsigned guest lengths must
        // not turn virtual-memory reservations into unbounded committed writes.
        // This is an implementation resource limit; exhaustion is a Wasm trap.
        let field = declarations
            .array_field(index)
            .ok_or_else(|| JsError::validation("invalid Wasm array declaration".into()))?;
        let count = match &initialization {
            ArrayInitialization::Repeated { count, .. } | ArrayInitialization::Default(count) => {
                *count as usize
            }
            ArrayInitialization::Fixed(values) => values.len(),
        };
        Self::check_wasm_array_size(count)?;
        let fields = match initialization {
            ArrayInitialization::Fixed(mut fields) => {
                for value in &mut fields {
                    *value = self.wasm_storage_value(*value, field.element_type, declarations)?;
                }
                fields
            }
            initialization => {
                let value = match initialization {
                    ArrayInitialization::Repeated { value, .. } => {
                        self.wasm_storage_value(value, field.element_type, declarations)?
                    }
                    ArrayInitialization::Default(_) => {
                        self.wasm_storage_default(field.element_type, declarations)?
                    }
                    ArrayInitialization::Fixed(_) => unreachable!("fixed elements handled above"),
                };
                let mut fields = Vec::new();
                fields
                    .try_reserve_exact(count)
                    .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge))?;
                fields.resize(count, value);
                fields
            }
        };
        Ok(self.heap.alloc(Cell::WasmGc {
            declarations: declarations.clone(),
            ty: index,
            fields,
            descriptor: None,
        }))
    }

    fn check_wasm_array_size(count: usize) -> Result<(), JsError> {
        const ARRAY_PAYLOAD_BUDGET_BYTES: usize = 64 * (1 << 20);
        if count > ARRAY_PAYLOAD_BUDGET_BYTES / std::mem::size_of::<Value>() {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge));
        }
        Ok(())
    }

    fn wasm_array_segment_values(
        &self,
        kind: crate::wasm::gc::ArraySegmentKind,
        source: Value,
        storage: wasmparser::StorageType,
        input: u32,
        count: u32,
    ) -> Result<ArraySegmentValues, JsError> {
        use crate::wasm::gc::ArraySegmentKind;
        match kind {
            ArraySegmentKind::Data => {
                let load = ArraySegmentKind::data_load(storage)
                    .ok_or_else(|| JsError::validation("invalid Wasm array data storage".into()))?;
                let bytes = if source == Value::UNDEFINED {
                    None
                } else {
                    let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(source) else {
                        return Err(JsError::validation(
                            "invalid Wasm data segment binding".into(),
                        ));
                    };
                    Some(bytes.clone())
                };
                let length = (count as usize)
                    .checked_mul(load.width())
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?;
                let range = crate::wasm::memory::checked_range(
                    u64::from(input),
                    length,
                    bytes.as_ref().map_or(0, |bytes| bytes.len()),
                )
                .map_err(JsError::wasm_trap_error)?;
                Ok(ArraySegmentValues::Data { bytes, load, range })
            }
            ArraySegmentKind::Element => {
                if !kind.accepts(storage) {
                    return Err(JsError::validation(
                        "invalid Wasm array element storage".into(),
                    ));
                }
                let length = if source == Value::UNDEFINED {
                    0
                } else {
                    let Some(Cell::WasmElements(values)) = self.heap.get(source) else {
                        return Err(JsError::validation(
                            "invalid Wasm element segment binding".into(),
                        ));
                    };
                    values.len()
                };
                let range =
                    super::wasm_table::table_range(u64::from(input), u64::from(count), length)?;
                Ok(ArraySegmentValues::Elements { source, range })
            }
        }
    }

    fn wasm_array_segment_value(&mut self, values: &ArraySegmentValues, index: usize) -> Value {
        match values {
            ArraySegmentValues::Data { bytes, load, range } => {
                let value = load
                    .read(
                        bytes.as_ref().expect("nonempty available segment"),
                        (range.start + index * load.width()) as u64,
                    )
                    .expect("checked complete segment range");
                self.encode_wasm_value(value)
            }
            ArraySegmentValues::Elements { source, range } => {
                let Some(Cell::WasmElements(values)) = self.heap.get(*source) else {
                    unreachable!("nonempty available element segment")
                };
                values[range.start + index]
            }
        }
    }

    pub(super) fn wasm_array_segment_new(
        &mut self,
        declarations: &crate::WasmTypes,
        index: u32,
        kind: crate::wasm::gc::ArraySegmentKind,
        source: Value,
        input: u32,
        count: u32,
    ) -> Result<Value, JsError> {
        let field = declarations
            .array_field(index)
            .ok_or_else(|| JsError::validation("invalid Wasm array declaration".into()))?;
        let source =
            self.wasm_array_segment_values(kind, source, field.element_type, input, count)?;
        Self::check_wasm_array_size(source.len())?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(source.len())
            .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge))?;
        for index in 0..source.len() {
            fields.push(self.wasm_array_segment_value(&source, index));
        }
        self.wasm_array_new(declarations, index, ArrayInitialization::Fixed(fields))
    }

    pub(super) fn wasm_array_segment_init(
        &mut self,
        reference: Value,
        kind: crate::wasm::gc::ArraySegmentKind,
        source: Value,
        output: u32,
        input: u32,
        count: u32,
    ) -> Result<(), JsError> {
        let (field, owner, fields) = self.wasm_array_fields(reference)?;
        if !field.mutable {
            return Err(JsError::validation(
                "immutable Wasm array segment destination".into(),
            ));
        }
        let output =
            crate::wasm::memory::checked_range(u64::from(output), count as usize, fields.len())
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray))?;
        let source =
            self.wasm_array_segment_values(kind, source, field.element_type, input, count)?;
        // Reference type admission is complete before the first mutation.
        if let ArraySegmentValues::Elements { source, range } = &source {
            if !range.is_empty() {
                let Some(Cell::WasmElements(values)) = self.heap.get(*source) else {
                    unreachable!("checked element segment")
                };
                for value in &values[range.clone()] {
                    self.wasm_storage_value(*value, field.element_type, owner)?;
                }
            }
        }
        for (index, destination) in output.enumerate() {
            let value = self.wasm_array_segment_value(&source, index);
            let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(reference) else {
                unreachable!("validated live array")
            };
            fields[destination] = value;
        }
        Ok(())
    }

    fn wasm_array_fields(
        &self,
        reference: Value,
    ) -> Result<(wasmparser::FieldType, &crate::WasmTypes, &[Value]), JsError> {
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(
                crate::WasmTrap::NullArrayReference,
            ));
        }
        let Some(Cell::WasmGc {
            declarations,
            ty,
            fields,
            ..
        }) = self.heap.get(reference)
        else {
            return Err(JsError::validation("invalid Wasm array reference".into()));
        };
        let field = declarations
            .array_field(*ty)
            .ok_or_else(|| JsError::validation("invalid Wasm array reference".into()))?;
        Ok((field, declarations, fields))
    }

    pub(super) fn wasm_array_len(&self, reference: Value) -> Result<u32, JsError> {
        u32::try_from(self.wasm_array_fields(reference)?.2.len())
            .map_err(|_| JsError::validation("invalid Wasm array length".into()))
    }

    pub(super) fn wasm_array_fill(
        &mut self,
        reference: Value,
        output: u32,
        value: Value,
        count: u32,
    ) -> Result<(), JsError> {
        let (field, owner, fields) = self.wasm_array_fields(reference)?;
        if !field.mutable {
            return Err(JsError::validation(
                "immutable Wasm array fill destination".into(),
            ));
        }
        let output =
            crate::wasm::memory::checked_range(u64::from(output), count as usize, fields.len())
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray))?;
        let value = self.wasm_storage_value(value, field.element_type, owner)?;
        let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(reference) else {
            unreachable!("validated live array")
        };
        fields[output].fill(value);
        Ok(())
    }

    pub(super) fn wasm_array_copy(
        &mut self,
        destination: Value,
        source: Value,
        output: u32,
        input: u32,
        count: u32,
    ) -> Result<(), JsError> {
        let (destination_field, destination_owner, destination_fields) =
            self.wasm_array_fields(destination)?;
        let (source_field, source_owner, source_fields) = self.wasm_array_fields(source)?;
        if !destination_field.mutable
            || !crate::wasm::gc::storage_subtype(
                source_field.element_type,
                source_owner,
                destination_field.element_type,
                destination_owner,
            )
        {
            return Err(JsError::validation(
                "incompatible Wasm array copy storage".into(),
            ));
        }
        // Both complete ranges, including empty slices, are checked before any write.
        let output = crate::wasm::memory::checked_range(
            u64::from(output),
            count as usize,
            destination_fields.len(),
        )
        .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray))?;
        let input = crate::wasm::memory::checked_range(
            u64::from(input),
            count as usize,
            source_fields.len(),
        )
        .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray))?;
        if destination == source {
            let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(destination) else {
                unreachable!("validated live array")
            };
            fields.copy_within(input, output.start);
        } else {
            // Distinct owners cannot alias; no temporary vector or collector re-entry is needed.
            for (destination_index, source_index) in output.zip(input) {
                let Some(Cell::WasmGc { fields, .. }) = self.heap.get(source) else {
                    unreachable!("validated live source")
                };
                let value = fields[source_index];
                let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(destination) else {
                    unreachable!("validated live destination")
                };
                fields[destination_index] = value;
            }
        }
        Ok(())
    }
}
