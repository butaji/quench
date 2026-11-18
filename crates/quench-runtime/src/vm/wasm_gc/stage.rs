//! The interpreter's `WasmGc` stage: the VM's heap cells, read and written in place by the GC
//! rules of `staged::wasm_gc`. Its scalar operators are `ConcreteWasm`'s, so the rules compute as
//! the interpreter does. A rule's trap is the VM's trap error; its rejection is a validation error.

use super::super::*;
use crate::WasmTypes;
use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_gc::WasmGc;
use crate::wasm::gc::GcField;
use crate::wasm::reference::{ReferenceTarget, TargetHeap};

/// The VM's heap, as a stage the GC rules run on.
pub(in crate::vm) struct HeapStage<'a, H: Host> {
    pub(in crate::vm) vm: &'a mut Vm<H>,
}

impl<H: Host> Machine for HeapStage<'_, H> {
    type Word = u64;
}

impl<H: Host> MachineStage for HeapStage<'_, H> {
    type Exit = JsError;

    crate::staged::wasm_concrete::scalar_operators! {}

    fn trap_when(&mut self, flag: u64, trap: crate::WasmTrap) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::wasm_trap_error(trap)) }
    }
}

impl<H: Host> WasmGc for HeapStage<'_, H> {
    fn is_null(&mut self, reference: u64) -> u64 {
        u64::from(Value::from_bits(reference).is_null())
    }

    fn is_object(&mut self, reference: u64) -> u64 {
        u64::from(matches!(self.vm.heap.get(Value::from_bits(reference)), Some(Cell::WasmGc { .. })))
    }

    /// One heap lookup: the address of the field's `Value` in the object's fields, 0 for a miss.
    /// The rules read or write it at once, before anything allocates.
    fn field_slot(&mut self, reference: u64, index: u64) -> u64 {
        match self.vm.heap.get_mut(Value::from_bits(reference)) {
            Some(Cell::WasmGc { fields, .. }) => fields
                .get_mut(usize::try_from(index).unwrap_or(usize::MAX))
                .map_or(0, |slot| std::ptr::from_mut(slot) as usize as u64),
            _ => 0,
        }
    }

    fn field_count(&mut self, reference: u64) -> u64 {
        match self.vm.heap.get(Value::from_bits(reference)) {
            Some(Cell::WasmGc { fields, .. }) => fields.len() as u64,
            _ => unreachable!("the rules count the fields of an object they checked"),
        }
    }

    fn read_slot(&mut self, slot: u64, field: GcField) -> u64 {
        // SAFETY: `slot` is a nonzero `field_slot` answer, taken just before with nothing
        // allocated since, so it addresses a live field of the object.
        let stored = unsafe { *(slot as usize as *const Value) };
        if field.packed_bits().is_some() {
            stored.as_int().expect("packed Wasm field") as u32 as u64
        } else {
            stored.to_bits()
        }
    }

    fn write_slot(&mut self, slot: u64, field: GcField, value: u64) {
        // SAFETY: as `read_slot`: a fresh nonzero `field_slot` answer.
        unsafe { *(slot as usize as *mut Value) = field.store(Value::from_bits(value)) };
    }

    fn array_mutable(&mut self, reference: u64) -> u64 {
        u64::from(self.array_owner(reference).is_some_and(|(field, _)| field.mutable))
    }

    fn storage_admits(&mut self, source: u64, destination: u64) -> u64 {
        let (Some((source_field, source_owner)), Some((destination_field, destination_owner))) =
            (self.array_owner(source), self.array_owner(destination))
        else {
            return 0;
        };
        u64::from(crate::wasm::gc::storage_subtype(
            source_field.element_type,
            source_owner,
            destination_field.element_type,
            destination_owner,
        ))
    }

    fn fields_exceed(&mut self, reference: u64, index: u64, extent: u64) -> u64 {
        let length = self.field_count(reference) as usize;
        u64::from(crate::wasm::memory::checked_range(index, extent as usize, length).is_err())
    }

    fn fill_fields(&mut self, reference: u64, index: u64, extent: u64, value: u64) {
        let Some(Cell::WasmGc { declarations, ty, fields, .. }) = self.vm.heap.get_mut(Value::from_bits(reference)) else {
            unreachable!("the rules fill an array they checked")
        };
        let element = declarations.array_field(*ty).expect("a validated array").element_type;
        let stored = GcField::new(0, element).expect("index 0 fits").store(Value::from_bits(value));
        let start = index as usize;
        fields[start..start + extent as usize].fill(stored);
    }

    fn copy_fields(&mut self, destination: u64, output: u64, source: u64, input: u64, extent: u64) {
        let (output, input, extent) = (output as usize, input as usize, extent as usize);
        let Some(Cell::WasmGc { fields, .. }) = self.vm.heap.get_mut(Value::from_bits(destination)) else {
            unreachable!("the rules copy into an array they checked")
        };
        if destination == source {
            fields.copy_within(input..input + extent, output);
            return;
        }
        // Distinct arrays cannot alias: the destination's fields leave their cell for one pass.
        let mut to = std::mem::take(fields);
        let Some(Cell::WasmGc { fields: from, .. }) = self.vm.heap.get(Value::from_bits(source)) else {
            unreachable!("the rules copy from an array they checked")
        };
        to[output..output + extent].copy_from_slice(&from[input..input + extent]);
        if let Some(Cell::WasmGc { fields, .. }) = self.vm.heap.get_mut(Value::from_bits(destination)) {
            *fields = to;
        }
    }

    fn admits(&mut self, reference: u64, target: ReferenceTarget, declarations: &WasmTypes) -> u64 {
        let reference = Value::from_bits(reference);
        let valid = match self.gc_object_cast(target, reference, declarations) {
            Some(valid) => valid,
            None => self.reference_valid(target, reference, declarations),
        };
        u64::from(valid)
    }

    fn reject_when(&mut self, flag: u64, reason: &'static str) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::validation(reason.into())) }
    }
}

impl<H: Host> HeapStage<'_, H> {
    /// The element storage of the array `reference`, if it is an array.
    pub(in crate::vm) fn array_element(&self, reference: u64) -> Option<wasmparser::StorageType> {
        self.array_owner(reference).map(|(field, _)| field.element_type)
    }

    /// The element declaration of the array `reference`, with the declarations that own it, if it
    /// is an array.
    fn array_owner(&self, reference: u64) -> Option<(wasmparser::FieldType, &WasmTypes)> {
        let Some(Cell::WasmGc { declarations, ty, .. }) = self.vm.heap.get(Value::from_bits(reference)) else {
            return None;
        };
        Some((declarations.array_field(*ty)?, declarations))
    }

    /// Whether a cast to `target` admits `reference` when that is null, an i31 or a GC object,
    /// read from the object's supertype display or kind; `None` otherwise.
    fn gc_object_cast(&self, target: ReferenceTarget, reference: Value, declarations: &WasmTypes) -> Option<bool> {
        let heap = target.heap()?;
        if reference.is_null() {
            return Some(target.is_nullable());
        }
        let object = match self.vm.heap.get(reference) {
            Some(Cell::WasmGc { declarations: owner, ty, .. }) => Some((owner, *ty)),
            _ if crate::wasm::i31::bits(reference).is_some() => None,
            _ => return None,
        };
        match (heap, object) {
            (TargetHeap::Module { index, exact }, Some((owner, ty))) => Some(if exact {
                owner.equivalent(ty as usize, declarations, index as usize)
            } else {
                owner.is_subtype(ty as usize, declarations, index as usize)
            }),
            (TargetHeap::Module { .. }, None) => Some(false),
            (TargetHeap::Abstract(abstract_type), object) => abstract_cast(abstract_type, object),
        }
    }

    /// Whether `reference` has the type a cast to `target` names, by the value-type relation of
    /// the module's declarations.
    fn reference_valid(&self, target: ReferenceTarget, reference: Value, declarations: &WasmTypes) -> bool {
        let target = target.reference_type().expect("validated reference target");
        declarations
            .callable_value_type(wasmparser::ValType::Ref(target))
            .is_some_and(|ty| self.vm.wasm_reference_valid_in(reference, ty, Some(declarations)))
    }
}

/// Whether an abstract cast target admits a GC object of `object`'s type, or an i31 when `object`
/// is `None`; `None` for a target outside the internal hierarchy.
fn abstract_cast(target: wasmparser::AbstractHeapType, object: Option<(&WasmTypes, u32)>) -> Option<bool> {
    use wasmparser::AbstractHeapType as Abstract;
    Some(match (target, object) {
        (Abstract::Any | Abstract::Eq, _) => true,
        (Abstract::Struct, Some((owner, ty))) => owner.struct_fields(ty).is_some(),
        (Abstract::Array, Some((owner, ty))) => owner.array_field(ty).is_some(),
        (Abstract::I31, object) => object.is_none(),
        (Abstract::Struct | Abstract::Array | Abstract::None, _) => false,
        _ => return None,
    })
}
