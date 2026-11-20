//! The GC rules' check order on a probe stage: each primitive the rules name is logged in the
//! order they ask for it, with the trap or rejection each check raises.

use super::wasm_gc::{Aggregate, WasmGc, array_copy, field_read};
use super::wasm_tests::Probe;
use crate::WasmTrap;
use crate::WasmTypes;
use crate::wasm::gc::{GcField, GcFieldAccess};
use crate::wasm::reference::ReferenceTarget;
use wasmparser::{StorageType, ValType};

impl Probe {
    fn gc_record(&mut self, entry: &str) -> usize {
        self.log.push(entry.to_owned());
        self.log.len() - 1
    }
}

impl WasmGc for Probe {
    fn is_null(&mut self, _: usize) -> usize {
        self.gc_record("is_null")
    }

    fn is_object(&mut self, _: usize) -> usize {
        self.gc_record("is_object")
    }

    fn field_slot(&mut self, _: usize, _: usize) -> usize {
        self.gc_record("field_slot")
    }

    fn field_slot_for_write(&mut self, _: usize, _: usize) -> usize {
        self.gc_record("field_slot_for_write")
    }

    fn field_count(&mut self, _: usize) -> usize {
        self.gc_record("field_count")
    }

    fn read_slot(&mut self, _: usize, _: GcField) -> usize {
        self.gc_record("read_slot")
    }

    fn write_slot(&mut self, _: usize, _: GcField, _: usize) {
        self.gc_record("write_slot");
    }

    fn array_mutable(&mut self, _: usize) -> usize {
        self.gc_record("array_mutable")
    }

    fn storage_admits(&mut self, _: usize, _: usize) -> usize {
        self.gc_record("storage_admits")
    }

    fn fields_exceed(&mut self, _: usize, _: usize, _: usize) -> usize {
        self.gc_record("fields_exceed")
    }

    fn fill_fields(&mut self, _: usize, _: usize, _: usize, _: usize) {
        self.gc_record("fill_fields");
    }

    fn copy_fields(&mut self, _: usize, _: usize, _: usize, _: usize, _: usize) {
        self.gc_record("copy_fields");
    }

    fn admits(&mut self, _: usize, _: ReferenceTarget, _: &WasmTypes) -> usize {
        self.gc_record("admits")
    }

    fn reject_when(&mut self, _: usize, reason: &'static str) -> Result<(), WasmTrap> {
        self.gc_record(&format!("reject {reason}"));
        Ok(())
    }
}

/// A field operand of storage `storage`, at field index 0.
fn field(storage: StorageType) -> GcField {
    GcField::new(0, storage).expect("index 0 fits")
}

/// The log a probe records for `entries`, in order.
fn log(entries: &[&str]) -> Vec<String> {
    entries.iter().map(|entry| (*entry).to_owned()).collect()
}

#[test]
fn a_struct_read_checks_null_then_the_field_then_reads() {
    let mut probe = Probe::default();
    let value = field(StorageType::Val(ValType::I32));
    field_read(&mut probe, Aggregate::Struct, 0, 0, value, GcFieldAccess::Read).expect("the probe never traps");
    assert_eq!(
        probe.log,
        log(&[
            "is_null",
            "trap NullStructReference",
            "field_slot",
            "EqualZero I64",
            "reject invalid Wasm aggregate reference",
            "read_slot",
        ])
    );
}

#[test]
fn an_array_read_traps_past_the_end_after_the_null_check() {
    let mut probe = Probe::default();
    let element = field(StorageType::Val(ValType::I32));
    field_read(&mut probe, Aggregate::Array, 0, 0, element, GcFieldAccess::Read).expect("the probe never traps");
    assert_eq!(
        probe.log,
        log(&[
            "is_null",
            "trap NullArrayReference",
            "field_slot",
            "EqualZero I64",
            "trap OutOfBoundsArray",
            "read_slot",
        ])
    );
}

#[test]
fn a_signed_packed_read_sign_extends_after_the_checks() {
    let mut probe = Probe::default();
    let packed = field(StorageType::I8);
    field_read(&mut probe, Aggregate::Struct, 0, 0, packed, GcFieldAccess::ReadSigned).expect("the probe never traps");
    assert_eq!(
        probe.log,
        log(&[
            "is_null",
            "trap NullStructReference",
            "field_slot",
            "EqualZero I64",
            "reject invalid Wasm aggregate reference",
            "read_slot",
            "constant I64 56",
            "ShiftLeft I64",
            "ShiftRightSigned I64",
        ])
    );
}

#[test]
fn an_array_copy_checks_both_operands_then_storage_then_both_ranges() {
    let mut probe = Probe::default();
    array_copy(&mut probe, 0, 0, 0, 0, 0).expect("the probe never traps");
    let operand = [
        "is_null",
        "trap NullArrayReference",
        "is_object",
        "EqualZero I32",
        "reject invalid Wasm array reference",
    ];
    let mut expected = log(&operand);
    expected.extend(log(&operand));
    expected.extend(log(&[
        "array_mutable",
        "storage_admits",
        "And I32",
        "EqualZero I32",
        "reject incompatible Wasm array copy storage",
        "fields_exceed",
        "trap OutOfBoundsArray",
        "fields_exceed",
        "trap OutOfBoundsArray",
        "copy_fields",
    ]));
    assert_eq!(probe.log, expected);
}

#[test]
fn the_probe_names_the_width_of_its_extension() {
    assert_eq!(field(StorageType::I8).packed_bits(), Some(8));
    assert_eq!(field(StorageType::I16).packed_bits(), Some(16));
    assert_eq!(field(StorageType::Val(ValType::I64)).packed_bits(), None);
}
