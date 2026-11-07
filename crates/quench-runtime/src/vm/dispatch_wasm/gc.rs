//! The Wasm GC opcodes of the shared vocabulary: operations of register values alone, with no
//! frame state, no control flow and no guest code, whose trap writes nothing. The general step
//! and the lane make them with the same step (`Vm::wasm_gc_value_step`).

use super::super::*;
use super::super::wasm_gc::{ArrayInitialization, GcFieldIndex};
use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind, GcFieldAccess, StructConstruction};
use crate::wasm::reference::ReferenceTarget;

/// Whether `op` is one of the operations `Vm::wasm_gc_value_step` makes.
pub(in crate::vm) const fn wasm_gc_value_op(op: Op) -> bool {
    matches!(
        op,
        Op::WasmRefIsNull | Op::WasmRefAsNonNull | Op::WasmRefEq | Op::WasmI31 | Op::WasmExternalConversion
            | Op::WasmArrayGet | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArraySet | Op::WasmArrayLen
            | Op::WasmArrayNewData | Op::WasmArrayNewElem | Op::WasmArrayInitData | Op::WasmArrayInitElem
            | Op::WasmArrayFill | Op::WasmArrayCopy
            | Op::WasmArrayNew | Op::WasmArrayNewDefault | Op::WasmArrayNewFixed
            | Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet
            | Op::WasmStructNew | Op::WasmStructNewDefault | Op::WasmStructNewDesc | Op::WasmStructNewDefaultDesc
            | Op::WasmRefGetDesc
            | Op::WasmRefTest | Op::WasmRefCast | Op::WasmDescriptorTest | Op::WasmDescriptorCast
    )
}

impl<H: Host> Vm<H> {
    /// One of the `wasm_gc_value_op`s of `program`, of the registers `read` answers: the value
    /// it writes to its result register, if any.
    pub(in crate::vm) fn wasm_gc_value_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        match i.op() {
            Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet => {
                self.wasm_struct_field_step(i, read)
            }
            Op::WasmArrayGet | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArraySet => {
                self.wasm_array_field_step(i, read)
            }
            Op::WasmArrayLen | Op::WasmArrayFill | Op::WasmArrayCopy => self.wasm_array_bulk_step(i, read),
            Op::WasmArrayNew | Op::WasmArrayNewDefault | Op::WasmArrayNewFixed => {
                self.wasm_array_new_step(program, i, read)
            }
            Op::WasmArrayNewData | Op::WasmArrayNewElem | Op::WasmArrayInitData | Op::WasmArrayInitElem => {
                self.wasm_array_segment_step(program, i, read)
            }
            Op::WasmStructNew | Op::WasmStructNewDefault | Op::WasmStructNewDesc
            | Op::WasmStructNewDefaultDesc | Op::WasmRefGetDesc => self.wasm_struct_new_step(program, i, read),
            Op::WasmRefTest | Op::WasmRefCast | Op::WasmDescriptorTest | Op::WasmDescriptorCast => {
                self.wasm_cast_step(program, i, read)
            }
            _ => self.wasm_reference_step(i, read),
        }
    }

    /// The declarations of `program`, a Wasm module's.
    fn wasm_module_declarations(&self, program: ProgramId) -> Result<crate::WasmTypes, JsError> {
        self.programs.wasm_signatures(program)
            .map(|pool| pool.declarations.clone())
            .ok_or_else(|| JsError::validation("Wasm GC operation requires module declarations".into()))
    }

    fn wasm_struct_field_step(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let access = GcFieldAccess::from_op(i.op()).expect("struct field opcode");
        let write = access == GcFieldAccess::Write;
        let reference = read(self, if write { i.register_a() } else { i.register_b() });
        // Validation proved the field a value field of the reference's static type, which
        // every subtype shares, and a value field stores the value a read gives.
        if access == GcFieldAccess::Read
            && let Some(Cell::WasmGc { fields, .. }) = self.heap.get(reference)
            && let Some(stored) = fields.get(i.imm() as usize)
        {
            return Ok(Some(*stored));
        }
        let value = write.then(|| read(self, i.register_b()));
        let result = self.wasm_gc_field(reference, GcFieldIndex::Struct(i.imm()), access, value)?;
        Ok((!write).then_some(result))
    }

    fn wasm_array_field_step(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let access = GcFieldAccess::from_op(i.op()).expect("array field opcode");
        let write = access == GcFieldAccess::Write;
        let (reference, index) =
            if write { (i.register_a(), i.register_b()) } else { (i.register_b(), i.register_c()) };
        let (reference, index) = (read(self, reference), Self::wasm_u32_operand(read(self, index))?);
        if access == GcFieldAccess::Read
            && let Some(Cell::WasmGc { fields, .. }) = self.heap.get(reference)
            && let Some(stored) = fields.get(index as usize)
        {
            return Ok(Some(*stored));
        }
        let value = write.then(|| read(self, i.register_c()));
        let result = self.wasm_gc_field(reference, GcFieldIndex::Array(index), access, value)?;
        Ok((!write).then_some(result))
    }

    fn wasm_array_bulk_step(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        if i.op() == Op::WasmArrayLen {
            let length = self.wasm_array_len(read(self, i.register_b()))?;
            return Ok(Some(Value::integer(length as i32)));
        }
        let (destination, source) = (read(self, i.register_a()), read(self, i.register_b()));
        let count = Self::wasm_u32_operand(read(self, i.register_c()))?;
        let (output, input) = i.register_pair();
        let output = Self::wasm_u32_operand(read(self, output))?;
        if i.op() == Op::WasmArrayFill {
            self.wasm_array_fill(destination, output, source, count)?;
        } else {
            let input = Self::wasm_u32_operand(read(self, input))?;
            self.wasm_array_copy(destination, source, output, input, count)?;
        }
        Ok(None)
    }

    fn wasm_array_new_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        let initialization = match i.op() {
            Op::WasmArrayNewFixed => {
                let window = i.register_window();
                let too_large = || JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge);
                ArrayInitialization::Fixed(self.wasm_window_values(window.base, window.count, read, too_large)?)
            }
            Op::WasmArrayNew => ArrayInitialization::Repeated {
                count: Self::wasm_u32_operand(read(self, i.register_c()))?,
                value: read(self, i.register_b()),
            },
            _ => ArrayInitialization::Default(Self::wasm_u32_operand(read(self, i.register_b()))?),
        };
        self.wasm_array_new(&declarations, i.imm(), initialization).map(Some)
    }

    fn wasm_array_segment_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let kind = ArraySegmentKind::from_op(i.op()).expect("array segment opcode");
        if matches!(i.op(), Op::WasmArrayNewData | Op::WasmArrayNewElem) {
            let base = i.register_b();
            let input = Self::wasm_u32_operand(read(self, base + ArraySegmentInput::Offset as u16))?;
            let count = Self::wasm_u32_operand(read(self, base + ArraySegmentInput::Count as u16))?;
            let source = read(self, base + ArraySegmentInput::Segment as u16);
            let declarations = self.wasm_module_declarations(program)?;
            return self.wasm_array_segment_new(&declarations, i.imm(), kind, source, input, count).map(Some);
        }
        let (reference, source) = (read(self, i.register_a()), read(self, i.register_b()));
        let count = Self::wasm_u32_operand(read(self, i.register_c()))?;
        let (output, input) = i.register_pair();
        let output = Self::wasm_u32_operand(read(self, output))?;
        let input = Self::wasm_u32_operand(read(self, input))?;
        self.wasm_array_segment_init(reference, kind, source, output, input, count)?;
        Ok(None)
    }

    fn wasm_struct_new_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        if i.op() == Op::WasmRefGetDesc {
            return self.wasm_ref_get_desc(&declarations, i.imm(), read(self, i.register_b())).map(Some);
        }
        let mode = StructConstruction::from_op(i.op()).expect("struct constructor opcode");
        // A defaulted struct without a descriptor names no register window.
        let window = (!mode.defaulted() || mode.described()).then(|| i.register_window());
        let fields = window.map_or(0, |window| window.count - u16::from(mode.described()));
        let descriptor = window.filter(|_| mode.described()).map(|window| read(self, window.base + fields));
        let values = match window.filter(|_| !mode.defaulted()) {
            Some(window) => {
                let failed = || JsError::validation("Wasm struct allocation failed".into());
                Some(self.wasm_window_values(window.base, fields, read, failed)?)
            }
            None => None,
        };
        self.wasm_struct_new(&declarations, i.imm(), values, descriptor).map(Some)
    }

    fn wasm_cast_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let target = ReferenceTarget::from_tag(i.imm()).expect("validated reference target");
        let reference = read(self, i.register_b());
        let declarations = self.wasm_module_declarations(program)?;
        let (valid, failure) = if matches!(i.op(), Op::WasmDescriptorTest | Op::WasmDescriptorCast) {
            let valid = self.wasm_descriptor_matches(&declarations, target, reference, read(self, i.register_c()))?;
            (valid, crate::WasmTrap::DescriptorCastFailure)
        } else {
            let target = target.reference_type().expect("validated reference target");
            let ty = declarations.callable_value_type(wasmparser::ValType::Ref(target))
                .ok_or_else(|| JsError::validation("invalid reference cast declaration".into()))?;
            (self.wasm_reference_valid_in(reference, ty, Some(&declarations)), crate::WasmTrap::CastFailure)
        };
        match i.op() {
            Op::WasmRefTest | Op::WasmDescriptorTest => Ok(Some(Value::integer(i32::from(valid)))),
            _ if valid => Ok(Some(reference)),
            _ => Err(JsError::wasm_trap_error(failure)),
        }
    }

    fn wasm_reference_step(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let value = read(self, i.register_b());
        Ok(Some(match i.op() {
            Op::WasmRefIsNull => Value::integer(i32::from(value.is_null())),
            Op::WasmRefEq => Value::integer(i32::from(value == read(self, i.register_c()))),
            Op::WasmRefAsNonNull if value.is_null() => {
                let check = crate::wasm::reference::NonNullCheck::from_tag(i.imm()).expect("validated non-null check");
                return Err(JsError::wasm_trap_error(check.trap()));
            }
            Op::WasmRefAsNonNull => value,
            Op::WasmI31 => {
                let operator = crate::wasm::i31::I31Operator::from_tag(i.imm()).expect("validated i31 operator");
                let value = self.decode_wasm_value(value, operator.input_type())?;
                self.encode_wasm_value(operator.apply(value).map_err(JsError::wasm_trap_error)?)
            }
            Op::WasmExternalConversion => {
                let conversion = crate::wasm::reference::ExternalConversion::from_tag(i.imm())
                    .expect("validated external conversion");
                self.decode_wasm_value(value, conversion.input_type())?;
                self.wasm_external_conversion(conversion, value)
            }
            op => unreachable!("{op:?} is no Wasm GC value step"),
        }))
    }
}
