use super::*;
use crate::wasm::gc::{GcField, GcFieldAccess, GcFieldValue, ObjectLayout};
use crate::staged::wasm_memory_bytes::MemoryLoadWasmMemoryBytesExt;

mod window;
pub mod stage;

/// The error of a struct whose storage no room is left for.
fn wasm_struct_allocation_failed() -> JsError {
    JsError::validation("Wasm struct allocation failed".into())
}

/// The trap of an array past the implementation's size bound, or whose storage no room is left for.
fn wasm_array_too_large() -> JsError {
    JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge)
}

pub(super) enum ArrayInitialization {
    Repeated { count: u32, value: Value },
    Default(u32),
    Fixed(Vec<Value>),
}

pub enum GcFieldIndex {
    Struct(u32),
    Array(u32),
}

enum ArraySegmentValues {
    Data {
        bytes: Option<crate::heap::BufferBytes>,
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

impl Vm {
    /// A new struct of declaration `index`: `values` answers its `len` field operands, or
    /// its fields take their defaults when it is `None`.
    pub(super) fn wasm_struct_new(
        &mut self,
        declarations: &crate::WasmTypes,
        index: u32,
        values: Option<(usize, &dyn Fn(&Self, usize) -> Value)>,
        descriptor: Option<Value>,
    ) -> Result<Value, JsError> {
        let fields = declarations
            .struct_fields(index)
            .ok_or_else(|| JsError::validation("invalid Wasm struct declaration".into()))?;
        self.check_wasm_struct_descriptor(declarations, index, descriptor)?;
        if let Some((len, _)) = values && len != fields.len() {
            return Err(JsError::validation("Wasm struct field count mismatch".into()));
        }
        let layout = declarations.layout(index).ok_or_else(|| JsError::validation("invalid Wasm struct declaration".into()))?;
        let mut stored = crate::heap::WasmGcFields::new(layout, 0, descriptor).ok_or_else(wasm_struct_allocation_failed)?;
        for (position, declared) in fields.iter().enumerate() {
            let field = GcField::new(position as u32, declared.element_type).expect("index fits");
            let value = match values {
                Some((_, value)) => value(self, position),
                None => self.wasm_storage_default(declared.element_type, declarations).map_err(|_| wasm_struct_allocation_failed())?,
            };
            let bits = self.wasm_stored_bits(field, value);
            stored.put(layout, position, field, bits);
        }
        Ok(self.heap.alloc_wasm_gc(crate::heap::WasmGcRecord::new(declarations.clone(), index, stored)))
    }

    fn check_wasm_struct_descriptor(
        &self,
        declarations: &crate::WasmTypes,
        index: u32,
        descriptor: Option<Value>,
    ) -> Result<(), JsError> {
        match (declarations.descriptor_type(index), descriptor) {
            (None, None) => Ok(()),
            (Some(_), Some(value)) if value.is_null() => {
                Err(JsError::wasm_trap_error(crate::WasmTrap::NullDescriptorReference))
            }
            (Some(ty), Some(value)) if self.wasm_reference_valid_in(value, ty, Some(declarations)) => Ok(()),
            (Some(_), Some(_)) => Err(JsError::validation("invalid Wasm struct descriptor".into())),
            _ => Err(JsError::validation("Wasm struct constructor descriptor mismatch".into())),
        }
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
            matches!(self.heap.wasm_gc(reference), Some(record) if record.fields().descriptor() == Some(descriptor)),
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
        match self.heap.wasm_gc(reference) {
            Some(record) if let Some(descriptor) = record.fields().descriptor() => {
                Ok(descriptor)
            }
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

    /// The bits a field or element of `field`'s storage keeps for the validated `value`: a 64-bit
    /// number's bits, read from its cell where it has one, and the other storages' `raw_of`.
    /// The register value of the Wasm bits `bits` a read of `field` by `access` gave
    /// (`staged::wasm_gc::field_read`): a 32-bit word or a packed integer as the register's
    /// integer, a 64-bit number as the register's `Value` (boxed only where its bits are in the tag
    /// space), a reference's bits as they are.
    #[inline(always)]
    pub fn wasm_field_value(&mut self, field: GcField, access: GcFieldAccess, bits: u64) -> Value {
        match (access, field.value()) {
            (GcFieldAccess::ReadSigned | GcFieldAccess::ReadUnsigned, _) | (_, GcFieldValue::Narrow) => Value::integer(bits as i32),
            (_, GcFieldValue::Wide) => self.encode_bits64(bits),
            _ => Value::from_bits(bits),
        }
    }

    pub fn wasm_stored_bits(&self, field: GcField, value: Value) -> u64 {
        match field.value() {
            GcFieldValue::Wide => self.decode_bits64(value).expect("validated Wasm 64-bit field"),
            _ => field.raw_of(value),
        }
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
        let layout = declarations.layout(index).ok_or_else(|| JsError::validation("invalid Wasm array declaration".into()))?;
        let mut stored = crate::heap::WasmGcFields::new(layout, count, None).ok_or_else(wasm_array_too_large)?;
        self.wasm_array_elements(declarations, layout, field.element_type, initialization, &mut stored)?;
        Ok(self.heap.alloc_wasm_gc(crate::heap::WasmGcRecord::new(declarations.clone(), index, stored)))
    }

    /// Stores the elements `initialization` gives in the new array `stored` of `layout`, whose
    /// elements have the `element` storage.
    fn wasm_array_elements(
        &mut self,
        declarations: &crate::WasmTypes,
        layout: &ObjectLayout,
        element: wasmparser::StorageType,
        initialization: ArrayInitialization,
        stored: &mut crate::heap::WasmGcFields,
    ) -> Result<(), JsError> {
        let field = GcField::new(0, element).expect("index 0 fits");
        let count = stored.count(layout);
        match initialization {
            ArrayInitialization::Fixed(values) => {
                for (position, value) in values.into_iter().enumerate() {
                    let bits = self.wasm_stored_bits(field, value);
                    stored.put(layout, position, field, bits);
                }
            }
            ArrayInitialization::Repeated { value, .. } => {
                let bits = self.wasm_stored_bits(field, value);
                stored.fill(layout, field, 0, count, bits);
            }
            ArrayInitialization::Default(_) => {
                let value = self.wasm_storage_default(element, declarations)?;
                let bits = self.wasm_stored_bits(field, value);
                stored.fill(layout, field, 0, count, bits);
            }
        }
        Ok(())
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
                        &bytes.as_ref().expect("nonempty available segment").read(),
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
        let (field, length) = self.wasm_array_fields(reference)?;
        if !field.mutable {
            return Err(JsError::validation(
                "immutable Wasm array segment destination".into(),
            ));
        }
        let output =
            crate::wasm::memory::checked_range(u64::from(output), count as usize, length)
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsArray))?;
        let source =
            self.wasm_array_segment_values(kind, source, field.element_type, input, count)?;
        let element = GcField::new(0, field.element_type).expect("index 0 fits");
        for (index, destination) in output.enumerate() {
            let value = self.wasm_array_segment_value(&source, index);
            let bits = self.wasm_stored_bits(element, value);
            let Some(record) = self.heap.wasm_gc_mut(reference) else {
                unreachable!("validated live array")
            };
            let Some((layout, fields)) = record.split_mut() else {
                unreachable!("a validated array")
            };
            fields.put(layout, destination, element, bits);
        }
        Ok(())
    }

    /// The element declaration of the live array `reference` and its length.
    fn wasm_array_fields(&self, reference: Value) -> Result<(wasmparser::FieldType, usize), JsError> {
        if reference.is_null() {
            return Err(JsError::wasm_trap_error(
                crate::WasmTrap::NullArrayReference,
            ));
        }
        let Some(record) = self.heap.wasm_gc(reference) else {
            return Err(JsError::validation("invalid Wasm array reference".into()));
        };
        let invalid = || JsError::validation("invalid Wasm array reference".into());
        let field = record.declarations().array_field(record.ty()).ok_or_else(invalid)?;
        let layout = record.layout().ok_or_else(invalid)?;
        Ok((field, record.fields().count(layout)))
    }
}
