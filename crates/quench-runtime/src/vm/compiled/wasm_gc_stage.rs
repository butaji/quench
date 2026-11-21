//! The Wasm GC operations of the u-jit: the staged rules of `staged::wasm_gc` emitted over the
//! compiled frame, so the checks, their order and their traps are the rules'. The primitives that
//! read or write the VM's heap are callouts that run `HeapStage`, the interpreter's stage, so the
//! two tiers share the heap's operations. A reference is its `Reference` kind's Value bits, as
//! every callout passes it; a field, element or value is the Wasm bits of its kind
//! (`GcFieldValue`): an `I32` zero-extended, an `I64` number's bits, or a reference's bits. The
//! interpreter's `HeapStage` works on Value bits, so each callout converts at its boundary.
//! Allocating operations are `wasm_gc_allocate`'s: an allocation only makes a collection due, and
//! a callout that allocates runs no interpreter code, so it needs no roots of its own.

use super::wasm_instance_stage::value_kind;
use super::wasm_stage::infallible;
use super::{Analysis, AnalysisState, CallContext, Emitter, JsError, Kind, Slot, Value, Vm, finish_callout};
use crate::bytecode::{Function, Op, WideInstruction};
use crate::heap::WasmGcRecord;
use crate::staged::layout_offsets::NATIVE_WASM_GC_LAYOUT;
use crate::staged::machine::{Extend, Lane, MachineAccess, MachineFlow, MachineScalar, MachineStage, Relation};
use crate::staged::value_encoding::{DIRECT_WASM_GC_HANDLE_BITS, HEAP_FAMILY_MASK, TAG_HEAP};
use cranelift_codegen::ir::MemFlagsData;
use crate::staged::wasm::Width;
use crate::staged::wasm_gc::{Aggregate, GcOp, GcStep, WasmGc, array_length, field_read, field_write, non_null, reference_cast};
use crate::vm::wasm_gc::stage::HeapStage;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::gc::{GcField, GcFieldAccess, GcFieldValue, static_offset};
use crate::wasm::integer::I64BinaryOperator as Binary;
use crate::wasm::reference::{NonNullCheck, ReferenceTarget};
use crate::WasmTypes;
use cranelift_codegen::ir;
use std::convert::Infallible;

/// The context word that holds the Wasm GC record range's base (`CallContext::wasm_gc_records`).
const CONTEXT_WASM_GC_RECORDS: i32 = std::mem::offset_of!(CallContext, wasm_gc_records) as i32;
/// The low bits of a Wasm GC reference that name its record: the record's ID in the record space.
const RECORD_ID_MASK: u64 = u32::MAX as u64;
/// The bytes of an array's length word, which its elements follow.
const ARRAY_LENGTH_BYTES: usize = std::mem::size_of::<u64>();

/// The VM of a callout's context. The compiled code that calls the callout suspends its VM in it.
///
/// # Safety
/// `context` is the live context of the call that runs the callout.
unsafe fn machine<'a>(context: *const CallContext) -> &'a mut Vm {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    unsafe { &mut *(*context).head.vm.cast::<Vm>() }
}

/// The heap primitives of `WasmGc`, one callout each; every one returns a word, zero where the
/// primitive has none. The rules have checked each argument before the callout runs.
unsafe extern "C" fn gc_is_object(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.is_object(reference)
}

unsafe extern "C" fn gc_field_slot(context: *mut CallContext, reference: u64, index: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.field_slot(reference, index)
}

unsafe extern "C" fn gc_field_slot_for_write(context: *mut CallContext, reference: u64, index: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.field_slot_for_write(reference, index)
}

unsafe extern "C" fn gc_field_count(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.field_count(reference)
}

/// The machine cell of `field`'s storage (`GcField::size`): the lane a compiled access loads and
/// stores in place.
fn field_lane(field: GcField) -> Lane {
    match field.size() {
        1 => Lane::I8,
        2 => Lane::I16,
        4 => Lane::I32,
        _ => Lane::I64,
    }
}

unsafe extern "C" fn gc_array_mutable(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.array_mutable(reference)
}

unsafe extern "C" fn gc_storage_admits(context: *mut CallContext, source: u64, destination: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.storage_admits(source, destination)
}

unsafe extern "C" fn gc_fields_exceed(context: *mut CallContext, reference: u64, index: u64, extent: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.fields_exceed(reference, index, extent)
}

/// Stores the Wasm bits `bits` in `extent` elements of the array; the element storage the array
/// holds gives the value's form.
unsafe extern "C" fn gc_fill_fields(context: *mut CallContext, reference: u64, index: u64, extent: u64, bits: u64) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.fill_field_bits(reference, index, extent, bits);
    0
}

unsafe extern "C" fn gc_copy_fields(
    context: *mut CallContext,
    destination: u64,
    output: u64,
    source: u64,
    input: u64,
    extent: u64,
) -> u64 {
    HeapStage { vm: unsafe { machine(context) } }.copy_fields(destination, output, source, input, extent);
    0
}

/// Whether `reference` passes a cast to the target `target` (a `ReferenceTarget` tag), by the
/// declarations of the running program.
unsafe extern "C" fn gc_admits(context: *mut CallContext, reference: u64, target: u64) -> u64 {
    let vm = unsafe { machine(context) };
    let program = unsafe { (*context).head.program };
    let Some(signatures) = vm.programs.wasm_signatures_shared(program) else {
        return 0;
    };
    let target = ReferenceTarget::validated(target as u32);
    HeapStage { vm }.admits(reference, target, &signatures.declarations)
}

/// Rejects with the validation message `reason` (static bytes): the error the rules raise for a
/// module validation proved impossible, which the status propagates.
unsafe extern "C" fn gc_reject(context: *mut CallContext, reason: u64, length: u64) -> i64 {
    // SAFETY: the emitted code passes the address and length of a static message.
    let bytes = unsafe { std::slice::from_raw_parts(reason as usize as *const u8, length as usize) };
    let message = std::str::from_utf8(bytes).unwrap_or("invalid Wasm GC operation");
    // SAFETY: compiled code passes the context of its call.
    finish_callout(unsafe { &mut *context }, Ok(Err(JsError::validation(message.into()))))
}

impl WasmGc for Emitter<'_, '_> {
    const PROVEN_STRUCT_READS: bool = true;

    fn is_null(&mut self, reference: ir::Value) -> ir::Value {
        let null = self.constant(Width::I64, Value::NULL.to_bits() as i64);
        self.operate(Binary::Equal, Width::I64, reference, null)
    }

    fn is_object(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_is_object as *const () as usize, &[context, reference])
    }

    fn field_slot(&mut self, reference: ir::Value, index: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_field_slot as *const () as usize, &[context, reference, index])
    }

    /// An array element's read address, inline (`array_element_address`), where the reference is a
    /// Wasm GC record and the record range is made; otherwise the `field_slot` callout answers it.
    fn element_slot(&mut self, reference: ir::Value, index: ir::Value, field: GcField) -> ir::Value {
        self.record_slot(reference, index, |emitter, payload| emitter.array_element_address(payload, index, field))
    }

    /// A struct field's read address, inline at the record's payload plus the field's static `offset`
    /// (`record_slot`). Without an offset, or where the record is not inline, the `field_slot` callout answers.
    fn struct_slot(&mut self, reference: ir::Value, index: ir::Value, offset: Option<u32>) -> ir::Value {
        let Some(offset) = offset else { return self.field_slot(reference, index) };
        self.record_slot(reference, index, |emitter, payload| {
            let offset = emitter.constant(Width::I64, i64::from(offset));
            emitter.operate(Binary::Add, Width::I64, payload, offset)
        })
    }

    fn field_slot_for_write(&mut self, reference: ir::Value, index: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_field_slot_for_write as *const () as usize, &[context, reference, index])
    }

    fn field_count(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_field_count as *const () as usize, &[context, reference])
    }

    /// One load of the field's storage, zero-extended to the word: the field's static type says
    /// what the bits are, so no callout, box or tag test stands between the slot and the value.
    fn read_slot(&mut self, slot: ir::Value, field: GcField) -> ir::Value {
        match field_lane(field) {
            Lane::I64 => self.raw_load(Lane::I64, MemFlagsData::trusted(), slot, 0),
            cell => self.raw_load_extended(cell, Extend::Zero, Lane::I64, MemFlagsData::trusted(), slot, 0),
        }
    }

    /// One store of the word's low bits to the field's storage.
    fn write_slot(&mut self, slot: ir::Value, field: GcField, value: ir::Value) {
        self.raw_store(field_lane(field), MemFlagsData::trusted(), (value, false), slot, 0);
    }

    fn array_mutable(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_array_mutable as *const () as usize, &[context, reference])
    }

    fn storage_admits(&mut self, source: ir::Value, destination: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_storage_admits as *const () as usize, &[context, source, destination])
    }

    fn fields_exceed(&mut self, reference: ir::Value, index: ir::Value, extent: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_fields_exceed as *const () as usize, &[context, reference, index, extent])
    }

    fn fill_fields(&mut self, reference: ir::Value, index: ir::Value, extent: ir::Value, value: ir::Value) {
        let context = self.body.core.context;
        self.call_leaf(gc_fill_fields as *const () as usize, &[context, reference, index, extent, value]);
    }

    fn copy_fields(
        &mut self,
        destination: ir::Value,
        output: ir::Value,
        source: ir::Value,
        input: ir::Value,
        extent: ir::Value,
    ) {
        let context = self.body.core.context;
        let arguments = [context, destination, output, source, input, extent];
        self.call_leaf(gc_copy_fields as *const () as usize, &arguments);
    }

    /// The cast's declarations are the running program's, which the callout reads itself; the
    /// emitted operation names the same program.
    fn admits(&mut self, reference: ir::Value, target: ReferenceTarget, _declarations: &WasmTypes) -> ir::Value {
        let context = self.body.core.context;
        let tag = self.constant(Width::I64, i64::from(target.tag()));
        self.call_leaf(gc_admits as *const () as usize, &[context, reference, tag])
    }

    /// Continues where `flag` is 0; where it is 1, the emitted code raises `reason` as the
    /// callout's validation error and propagates it, as a failing callout does.
    fn reject_when(&mut self, flag: ir::Value, reason: &'static str) -> Result<(), Infallible> {
        let context = self.body.core.context;
        let (reject, next) = (self.body.core.builder.create_block(), self.body.core.builder.create_block());
        self.body.core.builder.set_cold_block(reject);
        self.branch_if(flag, (reject, &[]), (next, &[]));
        self.enter(reject);
        let message = self.constant(Width::I64, reason.as_ptr() as usize as i64);
        let length = self.constant(Width::I64, reason.len() as i64);
        let status = self.call_leaf(gc_reject as *const () as usize, &[context, message, length]);
        self.propagate(status);
        self.goto(next, &[]);
        self.enter(next);
        Ok(())
    }
}

impl Emitter<'_, '_> {
    /// The address of a field of `reference`, a Wasm GC record: `address` gives it from the record's
    /// payload, inline where the record range is made (`base` nonzero) and the reference names a
    /// record; otherwise the `field_slot` callout answers.
    fn record_slot(
        &mut self,
        reference: ir::Value,
        index: ir::Value,
        address: impl FnOnce(&mut Self, ir::Value) -> ir::Value,
    ) -> ir::Value {
        let context = self.body.core.context;
        let base = self.raw_load(Lane::I64, MemFlagsData::trusted(), context, CONTEXT_WASM_GC_RECORDS);
        let (checked, inline, callout, join) = (self.new_block(), self.new_block(), self.new_block(), self.new_block());
        let slot = self.block_parameter(join, Lane::I64);
        let is_record = self.is_wasm_gc_record(reference);
        self.body.core.builder.set_cold_block(callout);
        self.branch_if(is_record, (checked, &[]), (callout, &[]));
        self.enter(checked);
        let made = self.raw_compare_imm(Relation::NotEqual, base, 0);
        self.branch_if(made, (inline, &[]), (callout, &[]));
        self.enter(inline);
        let payload = self.record_payload(base, reference);
        let address = address(self, payload);
        self.goto(join, &[address]);
        self.enter(callout);
        let address = self.call_leaf(gc_field_slot as *const () as usize, &[context, reference, index]);
        self.goto(join, &[address]);
        self.enter(join);
        slot
    }

    /// The flag that `reference` names a Wasm GC record: its handle family's bits are the record
    /// family's (`DIRECT_WASM_GC_HANDLE_BITS`), as `Value::direct_wasm_gc_index` tests them.
    fn is_wasm_gc_record(&mut self, reference: ir::Value) -> ir::Value {
        let mask = self.constant(Width::I64, HEAP_FAMILY_MASK as i64);
        let family = self.operate(Binary::And, Width::I64, reference, mask);
        let tag = self.constant(Width::I64, (TAG_HEAP | DIRECT_WASM_GC_HANDLE_BITS) as i64);
        self.operate(Binary::Equal, Width::I64, family, tag)
    }

    /// The payload address of the record `reference` names, at `base` plus its ID times the record's
    /// size: the record's head holds the address of its payload bytes.
    fn record_payload(&mut self, base: ir::Value, reference: ir::Value) -> ir::Value {
        let id_mask = self.constant(Width::I64, RECORD_ID_MASK as i64);
        let id = self.operate(Binary::And, Width::I64, reference, id_mask);
        let record_size = self.constant(Width::I64, std::mem::size_of::<WasmGcRecord>() as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, id, record_size);
        let record = self.operate(Binary::Add, Width::I64, base, offset);
        let payload_offset = NATIVE_WASM_GC_LAYOUT.payload_offset as i32;
        self.raw_load(Lane::I64, MemFlagsData::trusted(), record, payload_offset)
    }

    /// The address of element `index` of the array whose payload is `payload`: the length word leads
    /// the payload, and the element sits at its storage's size past it. Past the length the address is
    /// 0, as `WasmGcFields::address` answers. The element's storage size is the field's (`GcField::size`).
    fn array_element_address(&mut self, payload: ir::Value, index: ir::Value, field: GcField) -> ir::Value {
        let length = self.raw_load(Lane::I64, MemFlagsData::trusted(), payload, 0);
        let within = self.raw_compare(Relation::UnsignedLess, index, length);
        let element_size = self.constant(Width::I64, field.size() as i64);
        let scaled = self.operate(Binary::Multiply, Width::I64, index, element_size);
        let first = self.constant(Width::I64, ARRAY_LENGTH_BYTES as i64);
        let elements = self.operate(Binary::Add, Width::I64, payload, first);
        let element = self.operate(Binary::Add, Width::I64, elements, scaled);
        let absent = self.constant(Width::I64, 0);
        self.raw_select(within, element, absent)
    }
}

/// The register kind a field's word takes; `None` for a vector, which no tier reads.
pub(super) fn field_kind(field: GcField) -> Option<Kind> {
    match field.value() {
        GcFieldValue::Narrow => Some(Kind::I32),
        GcFieldValue::Wide => Some(Kind::I64),
        GcFieldValue::Reference => Some(Kind::Reference),
        GcFieldValue::Vector => None,
    }
}

/// Whether a field access names a struct's field, whose index its tag holds, rather than an
/// array element, whose index is a register.
pub(super) fn structure_access(op: Op) -> bool {
    matches!(GcStep::of(op), Some(GcStep::StructGet { .. } | GcStep::StructSet))
}

/// The registers of a field access, as the interpreter reads them: the reference, the array index
/// (`None` for a struct) and the data register, which a write stores and a read writes.
pub(super) fn field_registers(instruction: WideInstruction, write: bool, structure: bool) -> (u16, Option<u16>, u16) {
    match (structure, write) {
        (true, true) => (instruction.a(), None, instruction.b()),
        (true, false) => (instruction.b(), None, instruction.a()),
        (false, true) => (instruction.a(), Some(instruction.b()), instruction.c()),
        (false, false) => (instruction.b(), Some(instruction.c()), instruction.a()),
    }
}

/// Whether `register` holds a word of `kind`: the operand check of a GC step.
fn operand(state: &AnalysisState, registers: u16, register: u16, kind: Kind) -> Option<()> {
    (value_kind(state.get(register, registers)?)? == kind).then_some(())
}

/// A field access's operands and result: its reference, its index where it is an array's, and its
/// data, which a read gives its result register and a write must hold of the field's kind.
fn transfer_field(registers: u16, instruction: WideInstruction, state: &mut AnalysisState) -> Option<()> {
    let field = GcField::from_tag(instruction.imm())?;
    let scalar = field_kind(field)?;
    let write = GcStep::of(instruction.op())?.field()?.1 == GcFieldAccess::Write;
    let (reference, index, data) = field_registers(instruction, write, structure_access(instruction.op()));
    operand(state, registers, reference, Kind::Reference)?;
    if let Some(index) = index {
        operand(state, registers, index, Kind::I32)?;
    }
    if write {
        operand(state, registers, data, scalar)
    } else {
        state.set(data, registers, Slot::Value(scalar))
    }
}

impl Analysis<'_> {
    /// Checks a GC step and gives its result the kind its operation returns: references are
    /// `Reference` words, a reference test's and a length's operands as named, and a field write's
    /// value the kind of its storage. An allocating operation checks its own operands
    /// (`transfer_allocating`).
    pub(super) fn transfer_gc(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        op: GcOp,
    ) -> Option<()> {
        self.callouts.then_some(())?;
        let registers = function.registers;
        let reference = |state: &AnalysisState, register: u16| operand(state, registers, register, Kind::Reference);
        match op {
            GcOp::Field => transfer_field(registers, instruction, state),
            GcOp::ArrayLength => {
                reference(state, instruction.b())?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            GcOp::Null => {
                reference(state, instruction.b())?;
                if GcStep::of(instruction.op()) == Some(GcStep::AsNonNull) {
                    NonNullCheck::from_tag(instruction.imm())?;
                }
                let result = if GcStep::of(instruction.op()) == Some(GcStep::IsNull) { Kind::I32 } else { Kind::Reference };
                state.set(instruction.a(), registers, Slot::Value(result))
            }
            GcOp::Equal => {
                reference(state, instruction.b())?;
                reference(state, instruction.c())?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            GcOp::Test => {
                self.vm.programs.wasm_signatures(self.program)?;
                reference(state, instruction.b())?;
                let result = if GcStep::of(instruction.op()) == Some(GcStep::Test { cast: false, descriptor: false }) { Kind::I32 } else { Kind::Reference };
                state.set(instruction.a(), registers, Slot::Value(result))
            }
            GcOp::Allocating => self.transfer_allocating(function, instruction, state),
        }
    }
}

impl Emitter<'_, '_> {
    /// Lowers a GC step as its staged rule, over the frame's registers, or an allocating step as
    /// its callout (`gc_allocate`).
    pub(super) fn gc(&mut self, pc: usize, instruction: WideInstruction, op: GcOp) {
        match op {
            GcOp::Field => self.gc_field(instruction),
            GcOp::ArrayLength => {
                let reference = self.read(instruction.b(), Kind::Reference);
                let length = infallible(array_length(self, reference));
                let length = self.convert(Conversion::WrapI64, length);
                self.write(instruction.a(), Kind::I32, length);
            }
            GcOp::Null => self.gc_null(instruction),
            GcOp::Equal => {
                let left = self.read(instruction.b(), Kind::Reference);
                let right = self.read(instruction.c(), Kind::Reference);
                let equal = self.operate(Binary::Equal, Width::I64, left, right);
                self.write(instruction.a(), Kind::I32, equal);
            }
            GcOp::Test => self.gc_test(instruction),
            GcOp::Allocating => self.gc_allocate(pc, instruction),
        }
    }

    /// A struct or array access, as `field_read` or `field_write` runs it.
    fn gc_field(&mut self, instruction: WideInstruction) {
        let op = instruction.op();
        let access = GcStep::of(op).and_then(GcStep::field).expect("planned field access").1;
        let field = GcField::from_tag(instruction.imm()).expect("planned field operand");
        let kind = field_kind(field).expect("planned scalar field");
        let write = access == GcFieldAccess::Write;
        let structure = structure_access(op);
        let offset = if structure && !write { static_offset(instruction.c()) } else { None };
        let (reference, index, data) = field_registers(instruction, write, structure);
        let aggregate = if structure { Aggregate::Struct } else { Aggregate::Array };
        let reference = self.read(reference, Kind::Reference);
        let index = match index {
            Some(index) => {
                let index = self.read(index, Kind::I32);
                self.zero_extend(index)
            }
            None => self.constant(Width::I64, i64::from(field.index())),
        };
        if write {
            let value = match kind {
                Kind::I32 => {
                    let value = self.read(data, Kind::I32);
                    self.zero_extend(value)
                }
                Kind::I64 | Kind::Reference => self.read(data, kind),
            };
            infallible(field_write(self, aggregate, reference, index, field, value, offset));
        } else {
            let word = infallible(field_read(self, aggregate, reference, index, field, access, offset));
            let value = match kind {
                Kind::I32 => self.convert(Conversion::WrapI64, word),
                Kind::I64 | Kind::Reference => word,
            };
            self.write(data, kind, value);
        }
    }

    /// `ref.is_null` (a flag) and `ref.as_non_null` (the reference, or its null trap).
    fn gc_null(&mut self, instruction: WideInstruction) {
        let reference = self.read(instruction.b(), Kind::Reference);
        if GcStep::of(instruction.op()) == Some(GcStep::IsNull) {
            let flag = self.is_null(reference);
            self.write(instruction.a(), Kind::I32, flag);
            return;
        }
        let check = NonNullCheck::from_tag(instruction.imm()).expect("validated non-null check");
        let word = infallible(non_null(self, reference, check.trap()));
        self.write(instruction.a(), Kind::Reference, word);
    }

    /// `ref.test` (a flag) and `ref.cast` (the reference, or a cast failure), against the
    /// running program's declarations.
    fn gc_test(&mut self, instruction: WideInstruction) {
        let analysis = self.analysis;
        let declarations = analysis
            .vm
            .programs
            .wasm_signatures(analysis.program)
            .map(|signatures| &signatures.declarations)
            .expect("the analysis admits a reference test only with declarations");
        let reference = self.read(instruction.b(), Kind::Reference);
        let target = ReferenceTarget::validated(instruction.imm());
        if GcStep::of(instruction.op()) == Some(GcStep::Test { cast: false, descriptor: false }) {
            let admitted = self.admits(reference, target, declarations);
            let flag = self.convert(Conversion::WrapI64, admitted);
            self.write(instruction.a(), Kind::I32, flag);
        } else {
            let word = infallible(reference_cast(self, reference, target, declarations));
            self.write(instruction.a(), Kind::Reference, word);
        }
    }
}
