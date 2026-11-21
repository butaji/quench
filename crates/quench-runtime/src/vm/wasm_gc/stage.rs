//! The interpreter's `WasmGc` stage: the VM's heap cells, read and written in place by the GC
//! rules of `staged::wasm_gc`. Its scalar operators are `ConcreteWasm`'s, so the rules compute as
//! the interpreter does. A rule's trap is the VM's trap error; its rejection is a validation error.

use super::super::*;
use crate::WasmTypes;
use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_gc::{WasmGc, reference_cast};
use crate::wasm::gc::GcField;
use crate::wasm::reference::{ReferenceTarget, TargetHeap};

/// The VM's heap, as a stage the GC rules run on.
pub(in crate::vm) struct HeapStage<'a> {
    pub(in crate::vm) vm: &'a mut Vm,
}

impl Machine for HeapStage<'_> {
    type Word = u64;
}

impl MachineStage for HeapStage<'_> {
    type Exit = JsError;

    crate::staged::wasm_concrete::scalar_operators! {}

    fn trap_when(&mut self, flag: u64, trap: crate::WasmTrap) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::wasm_trap_error(trap)) }
    }
}

impl WasmGc for HeapStage<'_> {
    fn is_null(&mut self, reference: u64) -> u64 {
        u64::from(Value::from_bits(reference).is_null())
    }

    fn is_object(&mut self, reference: u64) -> u64 {
        u64::from(self.vm.heap.wasm_gc(Value::from_bits(reference)).is_some())
    }

    /// One heap lookup: the address of the field's stored bytes in the object's payload, at the
    /// offset its layout gives; 0 for a miss. A read, from a shared borrow: no write barrier. The
    /// rules read it at once, before anything allocates.
    fn field_slot(&mut self, reference: u64, index: u64) -> u64 {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        let Some(record) = self.vm.heap.wasm_gc(Value::from_bits(reference)) else {
            return 0;
        };
        match record.layout() {
            Some(layout) => record.fields().address(layout, index).map_or(0, |slot| slot as usize as u64),
            None => 0,
        }
    }

    /// `field_slot` for a store into the field: the write barrier is passed before the address is
    /// given, so the object is logged as the store into it requires.
    fn field_slot_for_write(&mut self, reference: u64, index: u64) -> u64 {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        let Some(record) = self.vm.heap.wasm_gc_mut(Value::from_bits(reference)) else {
            return 0;
        };
        match record.split_mut() {
            Some((layout, fields)) => fields.slot(layout, index).map_or(0, |slot| slot as usize as u64),
            None => 0,
        }
    }

    fn field_count(&mut self, reference: u64) -> u64 {
        match self.vm.heap.wasm_gc(Value::from_bits(reference)) {
            Some(record) => {
                let layout = record.layout().expect("the rules count the fields of an object they checked");
                record.fields().count(layout) as u64
            }
            _ => unreachable!("the rules count the fields of an object they checked"),
        }
    }

    /// The field's Wasm bits as its storage holds them: one load, no box and no tag.
    fn read_slot(&mut self, slot: u64, field: GcField) -> u64 {
        // SAFETY: `slot` is a nonzero `field_slot` answer, taken just before with nothing
        // allocated since, so it addresses the field's `size()` bytes of a live payload.
        let bytes = unsafe { std::slice::from_raw_parts(slot as usize as *const u8, field.size()) };
        field.load(bytes)
    }

    /// Stores the field's Wasm bits `value` in its storage: one store, no unboxing.
    fn write_slot(&mut self, slot: u64, field: GcField, value: u64) {
        // SAFETY: as `read_slot`: a fresh nonzero `field_slot` answer.
        let bytes = unsafe { std::slice::from_raw_parts_mut(slot as usize as *mut u8, field.size()) };
        field.store(bytes, value);
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
        let element = self.array_element(reference).expect("the rules fill an array they checked");
        let field = GcField::new(0, element).expect("index 0 fits");
        let bits = self.vm.wasm_stored_bits(field, Value::from_bits(value));
        self.fill_field_bits(reference, index, extent, bits);
    }

    fn copy_fields(&mut self, destination: u64, output: u64, source: u64, input: u64, extent: u64) {
        let (output, input, extent) = (output as usize, input as usize, extent as usize);
        let Some(record) = self.vm.heap.wasm_gc_mut(Value::from_bits(destination)) else {
            unreachable!("the rules copy into an array they checked")
        };
        let layout = record.layout().expect("a validated array");
        let output = layout.element_range(output, extent).expect("a checked array range");
        let input = layout.element_range(input, extent).expect("a checked array range");
        if destination == source {
            record.fields_mut().copy_within(input, output.start);
            return;
        }
        // Distinct arrays cannot alias: the destination's payload leaves its record for one pass.
        let mut to = std::mem::take(record.fields_mut());
        let Some(from) = self.vm.heap.wasm_gc(Value::from_bits(source)) else {
            unreachable!("the rules copy from an array they checked")
        };
        to.copy_from(output, from.fields(), input);
        if let Some(record) = self.vm.heap.wasm_gc_mut(Value::from_bits(destination)) {
            record.set_fields(to);
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

impl HeapStage<'_> {
    /// Stores the element's Wasm bits `bits` in `extent` elements of the array `reference` from
    /// `index`: the fill a tier makes with a value it already holds as bits, no box between.
    pub(in crate::vm) fn fill_field_bits(&mut self, reference: u64, index: u64, extent: u64, bits: u64) {
        let element = self.array_element(reference).expect("the rules fill an array they checked");
        let field = GcField::new(0, element).expect("index 0 fits");
        let Some(record) = self.vm.heap.wasm_gc_mut(Value::from_bits(reference)) else {
            unreachable!("the rules fill an array they checked")
        };
        let Some((layout, fields)) = record.split_mut() else {
            unreachable!("a validated array")
        };
        fields.fill(layout, field, index as usize, extent as usize, bits);
    }

    /// The element storage of the array `reference`, if it is an array.
    pub(in crate::vm) fn array_element(&self, reference: u64) -> Option<wasmparser::StorageType> {
        self.array_owner(reference).map(|(field, _)| field.element_type)
    }

    /// The element declaration of the array `reference`, with the declarations that own it, if it
    /// is an array.
    fn array_owner(&self, reference: u64) -> Option<(wasmparser::FieldType, &WasmTypes)> {
        let record = self.vm.heap.wasm_gc(Value::from_bits(reference))?;
        Some((record.declarations().array_field(record.ty())?, record.declarations()))
    }

    /// Whether a cast to `target` admits `reference` when that is null, an i31 or a GC object,
    /// read from the object's supertype display or kind; `None` otherwise.
    fn gc_object_cast(&self, target: ReferenceTarget, reference: Value, declarations: &WasmTypes) -> Option<bool> {
        let heap = target.heap()?;
        if reference.is_null() {
            return Some(target.is_nullable());
        }
        let object = match self.vm.heap.wasm_gc(reference) {
            Some(record) => Some((record.declarations(), record.ty())),
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

impl Vm {
    /// `ref.test` (`cast` false) or `ref.cast` of `reference`, its Wasm bits, to `target`, under
    /// `program`'s declarations: the admit flag, or the reference the cast leaves.
    pub(in crate::vm) fn wasm_ref_test_or_cast(
        &mut self,
        program: ProgramId,
        target: ReferenceTarget,
        reference: u64,
        cast: bool,
    ) -> Result<u64, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        let stage = &mut HeapStage { vm: self };
        if cast {
            reference_cast(stage, reference, target, &declarations)
        } else {
            Ok(stage.admits(reference, target, &declarations))
        }
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
