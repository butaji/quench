//! The Wasm GC operations of the u-jit: the staged rules of `staged::wasm_gc` emitted over the
//! compiled frame, so the checks, their order and their traps are the rules'. The primitives that
//! read or write the VM's heap are callouts that run `HeapStage`, the interpreter's stage, so the
//! two tiers share the heap's operations. A reference is its `I64` Value bits, as every callout
//! passes it; a field, element or value is the Wasm bits of its kind (`GcFieldValue`): an `I32`
//! zero-extended, an `I64` number's bits, or a reference's bits. The interpreter's `HeapStage`
//! works on Value bits, so each callout converts at its boundary. Allocating operations stay
//! uncompiled: an allocation can make a collection due, and compiled frames do not root the
//! reference bits they hold yet.

use super::wasm_instance_stage::value_kind;
use super::wasm_stage::infallible;
use super::{Analysis, AnalysisState, CallContext, Emitter, Host, JsError, Kind, Slot, Value, Vm, finish_callout};
use crate::bytecode::{Function, Op, WideInstruction};
use crate::staged::machine::{MachineAccess, MachineFlow, MachineStage};
use crate::staged::wasm::Width;
use crate::staged::wasm_gc::{Aggregate, GcOp, WasmGc, array_length, field_read, field_write, non_null, reference_cast};
use crate::vm::wasm_gc::stage::HeapStage;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::gc::{GcField, GcFieldAccess, GcFieldValue};
use crate::wasm::integer::I64BinaryOperator as Binary;
use crate::wasm::reference::{NonNullCheck, ReferenceTarget};
use crate::WasmTypes;
use cranelift_codegen::ir;
use std::convert::Infallible;

/// The VM of a callout's context. The compiled code that calls the callout suspends its VM in it.
///
/// # Safety
/// `context` is the live context of the call that runs the callout.
unsafe fn machine<'a, H: Host>(context: *const CallContext) -> &'a mut Vm<H> {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    unsafe { &mut *(*context).vm.cast::<Vm<H>>() }
}

/// The field a callout's tag names; the emitted code passes the tag of a planned access.
fn field_of_tag(tag: u64) -> GcField {
    GcField::from_tag(tag as u32).expect("planned field operand")
}

/// The Wasm bits of `stored`, the word `HeapStage::read_slot` returned for `field`: a packed
/// field's integer, an `i32`'s integer, a number's bits (boxed where the payload is in the tag
/// space), or a reference's bits.
fn stored_bits<H: Host>(vm: &Vm<H>, field: GcField, stored: u64) -> u64 {
    match field.value() {
        GcFieldValue::Narrow if field.packed_bits().is_some() => stored,
        GcFieldValue::Narrow => u64::from(Value::from_bits(stored).as_int().expect("validated Wasm i32 field") as u32),
        GcFieldValue::Wide => vm.decode_bits64(Value::from_bits(stored)).expect("validated Wasm 64-bit field"),
        GcFieldValue::Reference => stored,
        GcFieldValue::Vector => unreachable!("the analysis admits no vector field"),
    }
}

/// The value a field of `class` stores for the Wasm bits `bits`, the inverse of `stored_bits`.
fn value_of_bits<H: Host>(vm: &mut Vm<H>, class: GcFieldValue, bits: u64) -> Value {
    match class {
        GcFieldValue::Narrow => Value::integer(bits as u32 as i32),
        GcFieldValue::Wide => vm.encode_bits64(bits),
        GcFieldValue::Reference => Value::from_bits(bits),
        GcFieldValue::Vector => unreachable!("the analysis admits no vector field"),
    }
}

/// The heap primitives of `WasmGc`, one callout each; every one returns a word, zero where the
/// primitive has none. The rules have checked each argument before the callout runs.
unsafe extern "C" fn gc_is_object<H: Host>(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.is_object(reference)
}

unsafe extern "C" fn gc_field_slot<H: Host>(context: *mut CallContext, reference: u64, index: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.field_slot(reference, index)
}

unsafe extern "C" fn gc_field_count<H: Host>(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.field_count(reference)
}

unsafe extern "C" fn gc_read_slot<H: Host>(context: *mut CallContext, slot: u64, tag: u64) -> u64 {
    let vm = unsafe { machine::<H>(context) };
    let field = field_of_tag(tag);
    let stored = HeapStage { vm: &mut *vm }.read_slot(slot, field);
    stored_bits(vm, field, stored)
}

unsafe extern "C" fn gc_write_slot<H: Host>(context: *mut CallContext, slot: u64, tag: u64, bits: u64) -> u64 {
    let vm = unsafe { machine::<H>(context) };
    let field = field_of_tag(tag);
    let value = value_of_bits(vm, field.value(), bits);
    HeapStage { vm: &mut *vm }.write_slot(slot, field, value.to_bits());
    0
}

unsafe extern "C" fn gc_array_mutable<H: Host>(context: *mut CallContext, reference: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.array_mutable(reference)
}

unsafe extern "C" fn gc_storage_admits<H: Host>(context: *mut CallContext, source: u64, destination: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.storage_admits(source, destination)
}

unsafe extern "C" fn gc_fields_exceed<H: Host>(context: *mut CallContext, reference: u64, index: u64, extent: u64) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.fields_exceed(reference, index, extent)
}

/// Stores the Wasm bits `bits` in `extent` elements of the array; the element storage the array
/// holds gives the value's form.
unsafe extern "C" fn gc_fill_fields<H: Host>(context: *mut CallContext, reference: u64, index: u64, extent: u64, bits: u64) -> u64 {
    let vm = unsafe { machine::<H>(context) };
    let storage = HeapStage { vm: &mut *vm }.array_element(reference).expect("the rules fill an array they checked");
    let class = GcField::new(0, storage).expect("every storage has a class").value();
    let value = value_of_bits(vm, class, bits);
    HeapStage { vm: &mut *vm }.fill_fields(reference, index, extent, value.to_bits());
    0
}

unsafe extern "C" fn gc_copy_fields<H: Host>(
    context: *mut CallContext,
    destination: u64,
    output: u64,
    source: u64,
    input: u64,
    extent: u64,
) -> u64 {
    HeapStage { vm: unsafe { machine::<H>(context) } }.copy_fields(destination, output, source, input, extent);
    0
}

/// Whether `reference` passes a cast to the target `target` (a `ReferenceTarget` tag), by the
/// declarations of the running program.
unsafe extern "C" fn gc_admits<H: Host>(context: *mut CallContext, reference: u64, target: u64) -> u64 {
    let vm = unsafe { machine::<H>(context) };
    let program = unsafe { (*context).program };
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

impl<H: Host> WasmGc for Emitter<'_, '_, H> {
    fn is_null(&mut self, reference: ir::Value) -> ir::Value {
        let null = self.constant(Width::I64, Value::NULL.to_bits() as i64);
        self.operate(Binary::Equal, Width::I64, reference, null)
    }

    fn is_object(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_is_object::<H> as *const () as usize, &[context, reference])
    }

    fn field_slot(&mut self, reference: ir::Value, index: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_field_slot::<H> as *const () as usize, &[context, reference, index])
    }

    fn field_count(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_field_count::<H> as *const () as usize, &[context, reference])
    }

    fn read_slot(&mut self, slot: ir::Value, field: GcField) -> ir::Value {
        let context = self.body.core.context;
        let tag = self.constant(Width::I64, i64::from(field.tag()));
        self.call_leaf(gc_read_slot::<H> as *const () as usize, &[context, slot, tag])
    }

    fn write_slot(&mut self, slot: ir::Value, field: GcField, value: ir::Value) {
        let context = self.body.core.context;
        let tag = self.constant(Width::I64, i64::from(field.tag()));
        self.call_leaf(gc_write_slot::<H> as *const () as usize, &[context, slot, tag, value]);
    }

    fn array_mutable(&mut self, reference: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_array_mutable::<H> as *const () as usize, &[context, reference])
    }

    fn storage_admits(&mut self, source: ir::Value, destination: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_storage_admits::<H> as *const () as usize, &[context, source, destination])
    }

    fn fields_exceed(&mut self, reference: ir::Value, index: ir::Value, extent: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        self.call_leaf(gc_fields_exceed::<H> as *const () as usize, &[context, reference, index, extent])
    }

    fn fill_fields(&mut self, reference: ir::Value, index: ir::Value, extent: ir::Value, value: ir::Value) {
        let context = self.body.core.context;
        self.call_leaf(gc_fill_fields::<H> as *const () as usize, &[context, reference, index, extent, value]);
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
        self.call_leaf(gc_copy_fields::<H> as *const () as usize, &arguments);
    }

    /// The cast's declarations are the running program's, which the callout reads itself; the
    /// emitted operation names the same program.
    fn admits(&mut self, reference: ir::Value, target: ReferenceTarget, _declarations: &WasmTypes) -> ir::Value {
        let context = self.body.core.context;
        let tag = self.constant(Width::I64, i64::from(target.tag()));
        self.call_leaf(gc_admits::<H> as *const () as usize, &[context, reference, tag])
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

/// The register kind a field's word takes; `None` for a vector, which no tier reads.
fn field_kind(field: GcField) -> Option<Kind> {
    match field.value() {
        GcFieldValue::Narrow => Some(Kind::I32),
        GcFieldValue::Wide | GcFieldValue::Reference => Some(Kind::I64),
        GcFieldValue::Vector => None,
    }
}

/// Whether a field access names a struct's field, whose index its tag holds, rather than an
/// array element, whose index is a register.
fn structure_access(op: Op) -> bool {
    matches!(op, Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet)
}

/// The registers of a field access, as the interpreter reads them: the reference, the array index
/// (`None` for a struct) and the data register, which a write stores and a read writes.
fn field_registers(instruction: WideInstruction, write: bool, structure: bool) -> (u16, Option<u16>, u16) {
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
    let write = GcFieldAccess::from_op(instruction.op())? == GcFieldAccess::Write;
    let (reference, index, data) = field_registers(instruction, write, structure_access(instruction.op()));
    operand(state, registers, reference, Kind::I64)?;
    if let Some(index) = index {
        operand(state, registers, index, Kind::I32)?;
    }
    if write {
        operand(state, registers, data, scalar)
    } else {
        state.set(data, registers, Slot::Value(scalar))
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Checks a GC step and gives its result the kind its operation returns: references are `I64`
    /// words, a reference test's and a length's operands as named, and a field write's value the
    /// kind of its storage. An allocating operation is not admitted, so its function stays
    /// uncompiled.
    pub(super) fn transfer_gc(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        op: GcOp,
    ) -> Option<()> {
        self.callouts.then_some(())?;
        let registers = function.registers;
        let reference = |state: &AnalysisState, register: u16| operand(state, registers, register, Kind::I64);
        match op {
            GcOp::Field => transfer_field(registers, instruction, state),
            GcOp::ArrayLength => {
                reference(state, instruction.b())?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            GcOp::Null => {
                reference(state, instruction.b())?;
                if instruction.op() == Op::WasmRefAsNonNull {
                    NonNullCheck::from_tag(instruction.imm())?;
                }
                let result = if instruction.op() == Op::WasmRefIsNull { Kind::I32 } else { Kind::I64 };
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
                let result = if instruction.op() == Op::WasmRefTest { Kind::I32 } else { Kind::I64 };
                state.set(instruction.a(), registers, Slot::Value(result))
            }
            GcOp::Allocating => None,
        }
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// Lowers a GC step as its staged rule, over the frame's registers. The analysis admits no
    /// allocating operation (`transfer_gc`).
    pub(super) fn gc(&mut self, instruction: WideInstruction, op: GcOp) {
        match op {
            GcOp::Field => self.gc_field(instruction),
            GcOp::ArrayLength => {
                let reference = self.read(instruction.b(), Kind::I64);
                let length = infallible(array_length(self, reference));
                let length = self.convert(Conversion::WrapI64, length);
                self.write(instruction.a(), Kind::I32, length);
            }
            GcOp::Null => self.gc_null(instruction),
            GcOp::Equal => {
                let left = self.read(instruction.b(), Kind::I64);
                let right = self.read(instruction.c(), Kind::I64);
                let equal = self.operate(Binary::Equal, Width::I64, left, right);
                self.write(instruction.a(), Kind::I32, equal);
            }
            GcOp::Test => self.gc_test(instruction),
            GcOp::Allocating => unreachable!("transfer_gc leaves allocating operations uncompiled"),
        }
    }

    /// A struct or array access, as `field_read` or `field_write` runs it.
    fn gc_field(&mut self, instruction: WideInstruction) {
        let op = instruction.op();
        let access = GcFieldAccess::from_op(op).expect("planned field access");
        let field = GcField::from_tag(instruction.imm()).expect("planned field operand");
        let kind = field_kind(field).expect("planned scalar field");
        let write = access == GcFieldAccess::Write;
        let structure = structure_access(op);
        let (reference, index, data) = field_registers(instruction, write, structure);
        let aggregate = if structure { Aggregate::Struct } else { Aggregate::Array };
        let reference = self.read(reference, Kind::I64);
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
                Kind::I64 => self.read(data, Kind::I64),
            };
            infallible(field_write(self, aggregate, reference, index, field, value));
        } else {
            let word = infallible(field_read(self, aggregate, reference, index, field, access));
            let value = match kind {
                Kind::I32 => self.convert(Conversion::WrapI64, word),
                Kind::I64 => word,
            };
            self.write(data, kind, value);
        }
    }

    /// `ref.is_null` (a flag) and `ref.as_non_null` (the reference, or its null trap).
    fn gc_null(&mut self, instruction: WideInstruction) {
        let reference = self.read(instruction.b(), Kind::I64);
        if instruction.op() == Op::WasmRefIsNull {
            let flag = self.is_null(reference);
            self.write(instruction.a(), Kind::I32, flag);
            return;
        }
        let check = NonNullCheck::from_tag(instruction.imm()).expect("validated non-null check");
        let word = infallible(non_null(self, reference, check.trap()));
        self.write(instruction.a(), Kind::I64, word);
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
        let reference = self.read(instruction.b(), Kind::I64);
        let target = ReferenceTarget::validated(instruction.imm());
        if instruction.op() == Op::WasmRefTest {
            let admitted = self.admits(reference, target, declarations);
            let flag = self.convert(Conversion::WrapI64, admitted);
            self.write(instruction.a(), Kind::I32, flag);
        } else {
            let word = infallible(reference_cast(self, reference, target, declarations));
            self.write(instruction.a(), Kind::I64, word);
        }
    }
}
