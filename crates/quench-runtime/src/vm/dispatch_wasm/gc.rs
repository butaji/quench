//! The Wasm GC opcodes of the shared vocabulary: operations of register values alone, with no
//! frame state, no control flow and no guest code, whose trap writes nothing. The general step
//! and the lane make them with the same step (`Vm::wasm_gc_value_step`).

use super::super::*;
use super::super::wasm_gc::{ArrayInitialization, GcFieldIndex};
use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind, GcField, GcFieldAccess};
use crate::wasm::reference::ReferenceTarget;
use crate::staged::wasm_gc::{Aggregate, ArrayConstruction, GcStep, WasmGc, array_copy, array_fill, array_length, field_read, field_write, non_null};
use crate::staged::wasm_call::Call;
use super::super::wasm_gc::stage::HeapStage;

/// The one list of the GC value opcodes (`staged::wasm_gc::wasm_gc_table!`), which the lane's
/// handler of each opcode derives from.
pub(in crate::vm) use crate::staged::wasm_gc::wasm_gc_value_ops;

/// Whether the GC value opcode `op` can allocate a heap cell, which can make a collection due:
/// its step's group's `allocates`. A call target has no step and is taken to allocate, as before.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_allocates(op: Op) -> bool {
    match GcStep::of(op) {
        Some(step) => step.group().allocates(),
        None => true,
    }
}

/// Whether the GC value opcode `op` reads or writes a struct field or an array element.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_field_op(op: Op) -> bool {
    match GcStep::of(op) {
        Some(step) => step.field().is_some(),
        None => false,
    }
}

/// A field access's operands (`Vm::wasm_field_operands`): the object, the field or element, its
/// storage and access, and the value a write stores.
pub(in crate::vm) struct FieldOperands {
    pub(in crate::vm) reference: Value,
    pub(in crate::vm) location: GcFieldIndex,
    pub(in crate::vm) field: GcField,
    pub(in crate::vm) access: GcFieldAccess,
    pub(in crate::vm) value: Option<Value>,
    /// Whether the access is a `*Raw` form: the data register holds the field's Wasm bits as a raw
    /// word (`compile::liveness::raw_webs`), which moves to or from the storage untouched.
    pub(in crate::vm) raw: bool,
}

/// The operands of a bulk array step (`array.fill`, `array.copy`), decoded from the registers: the
/// destination and source references, the element count, and the output and input indices (`input`
/// is only read by a copy).
pub(in crate::vm) struct ArrayBulkOperands {
    pub(in crate::vm) destination: Value,
    pub(in crate::vm) source: Value,
    pub(in crate::vm) count: u64,
    pub(in crate::vm) output: u64,
    pub(in crate::vm) input: u64,
}

impl<H: Host> Vm<H> {
    /// The operands of a bulk array step of `i`'s registers, as `wasm_array_bulk_step` and the lane
    /// decode them (`copy` for `array.copy`, else `array.fill`).
    #[inline(always)]
    pub(in crate::vm) fn wasm_array_bulk_operands(
        &self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
        copy: bool,
    ) -> Result<ArrayBulkOperands, JsError> {
        let (destination, source) = (read(self, i.register_a()), read(self, i.register_b()));
        let count = u64::from(Self::wasm_u32_operand(read(self, i.register_c()))?);
        let (output, input) = i.register_pair();
        let output = u64::from(Self::wasm_u32_operand(read(self, output))?);
        let input = if copy { u64::from(Self::wasm_u32_operand(read(self, input))?) } else { 0 };
        Ok(ArrayBulkOperands { destination, source, count, output, input })
    }
}

/// Whether the GC value opcode `op` reads the running instance's environment: the
/// operations that name one of its functions or tables' targets.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_reads_instance(op: Op) -> bool {
    matches!(Call::of(op), Some(Call::IndirectTarget | Call::RefFunc))
}

impl<H: Host> Vm<H> {
    /// One of the GC value opcodes of `program`'s instance `env`, of the registers `read` answers: the value
    /// it writes to its result register, if any. Inlined, so a lane handler of one opcode
    /// keeps only that opcode's arm.
    #[inline(always)]
    pub(in crate::vm) fn wasm_gc_value_step(
        &mut self,
        program: ProgramId,
        env: Value,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        let Some(step) = GcStep::of(i.op()) else {
            return self.wasm_call_target_step(program, env, i, read).map(Some);
        };
        match step {
            GcStep::StructGet { .. } | GcStep::StructSet | GcStep::ArrayGet { .. } | GcStep::ArraySet => {
                self.wasm_field_step(step, i, read)
            }
            GcStep::ArrayLen | GcStep::ArrayFill | GcStep::ArrayCopy => self.wasm_array_bulk_step(step, i, read),
            GcStep::IsNull | GcStep::AsNonNull | GcStep::Equal | GcStep::I31 | GcStep::ExternConversion => {
                self.wasm_reference_step(step, i, read)
            }
            GcStep::Test { cast, descriptor } => self.wasm_cast_step(program, cast, descriptor, i, read),
            GcStep::ArrayNew { construction } => self.wasm_array_new_step(program, construction, i, read),
            GcStep::ArraySegment { kind, init } => self.wasm_array_segment_step(program, kind, init, i, read),
            GcStep::StructNew { default, descriptor } => {
                self.wasm_struct_new_step(program, default, descriptor, i, read)
            }
            GcStep::GetDescriptor => self.wasm_get_descriptor_step(program, i, read),
        }
    }

    /// `wasm_gc_value_step` in a frame of its own, for the general loop, whose frame a
    /// recursion of guest calls multiplies: the steps' temporaries stay out of it.
    #[inline(never)]
    pub(in crate::vm) fn wasm_gc_general_step(
        &mut self,
        program: ProgramId,
        env: Value,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        self.wasm_gc_value_step(program, env, i, read)
    }

    /// The function a call through a reference calls: a `call_indirect`'s table element, once
    /// its signature matches, or `ref.func`'s reference to a function of instance `env`.
    fn wasm_call_target_step(
        &mut self,
        program: ProgramId,
        env: Value,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Value, JsError> {
        if matches!(Call::of(i.op()), Some(Call::IndirectTarget)) {
            return self.wasm_indirect_target_of_operand(program, read(self, i.register_b()), read(self, i.register_c()), i.imm());
        }
        let residual = self.programs.get(program).ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
        self.wasm_function_reference(&residual, i.imm(), env)
    }

    /// The declarations of `program`, a Wasm module's.
    pub(in crate::vm) fn wasm_module_declarations(&self, program: ProgramId) -> Result<crate::WasmTypes, JsError> {
        self.programs.wasm_signatures(program)
            .map(|pool| pool.declarations.clone())
            .ok_or_else(|| JsError::validation("Wasm GC operation requires module declarations".into()))
    }

    /// The operands of the field step `step` (`struct.get`/`set`, `array.get`/`set`) of the
    /// instruction `i`, read from its registers.
    #[inline(always)]
    pub(in crate::vm) fn wasm_field_operands(
        &self,
        step: GcStep,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<FieldOperands, JsError> {
        let (aggregate, access) = step.field().expect("field step");
        let write = access == GcFieldAccess::Write;
        let field = GcField::from_tag(i.imm()).expect("validated field operand");
        let raw = i.op().traits().has(crate::bytecode::Traits::RAW_FORM);
        if aggregate == Aggregate::Struct {
            let reference = read(self, if write { i.register_a() } else { i.register_b() });
            let value = write.then(|| read(self, i.register_b()));
            return Ok(FieldOperands { reference, location: GcFieldIndex::Struct(field.index()), field, access, value, raw });
        }
        let (reference, index) =
            if write { (i.register_a(), i.register_b()) } else { (i.register_b(), i.register_c()) };
        let location = GcFieldIndex::Array(Self::wasm_u32_operand(read(self, index))?);
        let value = write.then(|| read(self, i.register_c()));
        Ok(FieldOperands { reference: read(self, reference), location, field, access, value, raw })
    }

    /// The field access the GC rules make on the heap (`staged::wasm_gc`): the value it reads, or
    /// the trap or validation error a miss raises.
    #[inline(always)]
    fn wasm_field_rule(
        &mut self,
        reference: Value,
        location: GcFieldIndex,
        field: GcField,
        access: GcFieldAccess,
        value: Option<Value>,
        raw: bool,
    ) -> Result<Option<Value>, JsError> {
        let (aggregate, index) = match location {
            GcFieldIndex::Struct(index) => (Aggregate::Struct, index),
            GcFieldIndex::Array(index) => (Aggregate::Array, index),
        };
        let (reference, index) = (reference.to_bits(), u64::from(index));
        let mut stage = HeapStage { vm: self };
        match value {
            Some(value) => {
                let bits = if raw { value.to_bits() } else { stage.vm.wasm_stored_bits(field, value) };
                field_write(&mut stage, aggregate, reference, index, field, bits).map(|()| None)
            }
            None => {
                let word = field_read(&mut stage, aggregate, reference, index, field, access)?;
                Ok(Some(if raw { Value::from_bits(word) } else { stage.vm.wasm_field_value(field, access, word) }))
            }
        }
    }

    /// The field step `step`, raising the trap a miss makes.
    #[inline(always)]
    fn wasm_field_step(
        &mut self,
        step: GcStep,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        let FieldOperands { reference, location, field, access, value, raw } = self.wasm_field_operands(step, i, read)?;
        self.wasm_field_rule(reference, location, field, access, value, raw)
    }

    /// The bulk operations of an array: `array.len`, `array.fill` and `array.copy`, whose operands
    /// are decoded from their registers before the rule runs on the heap.
    fn wasm_array_bulk_step(
        &mut self,
        step: GcStep,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        match step {
            GcStep::ArrayLen => {
                let reference = read(self, i.register_b()).to_bits();
                let length = array_length(&mut HeapStage { vm: self }, reference)?;
                Ok(Some(Value::integer(length as i32)))
            }
            GcStep::ArrayFill => {
                let ops = self.wasm_array_bulk_operands(i, read, false)?;
                let stage = &mut HeapStage { vm: self };
                array_fill(stage, ops.destination.to_bits(), ops.output, ops.source.to_bits(), ops.count)?;
                Ok(None)
            }
            _ => {
                let ops = self.wasm_array_bulk_operands(i, read, true)?;
                let stage = &mut HeapStage { vm: self };
                array_copy(stage, ops.destination.to_bits(), ops.output, ops.source.to_bits(), ops.input, ops.count)?;
                Ok(None)
            }
        }
    }

    fn wasm_array_new_step(
        &mut self,
        program: ProgramId,
        construction: ArrayConstruction,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        let initialization = match construction {
            ArrayConstruction::Fixed => {
                let window = i.register_window();
                let too_large = || JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge);
                ArrayInitialization::Fixed(self.wasm_window_values(window.base, window.count, read, too_large)?)
            }
            ArrayConstruction::Repeated => ArrayInitialization::Repeated {
                count: Self::wasm_u32_operand(read(self, i.register_c()))?,
                value: read(self, i.register_b()),
            },
            ArrayConstruction::Default => {
                ArrayInitialization::Default(Self::wasm_u32_operand(read(self, i.register_b()))?)
            }
        };
        self.wasm_array_new(&declarations, i.imm(), initialization).map(Some)
    }

    fn wasm_array_segment_step(
        &mut self,
        program: ProgramId,
        kind: ArraySegmentKind,
        init: bool,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        if !init {
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
        default: bool,
        descriptor: bool,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        // A defaulted struct without a descriptor names no register window.
        let window = (!default || descriptor).then(|| i.register_window());
        let fields = window.map_or(0, |window| window.count - u16::from(descriptor));
        let descriptor_value = window.filter(|_| descriptor).map(|window| read(self, window.base + fields));
        let operands = window.filter(|_| !default).map(|window| window.base);
        let field = |vm: &Self, index: usize| read(vm, operands.unwrap_or_default() + index as u16);
        let values = operands.map(|_| (usize::from(fields), &field as &dyn Fn(&Self, usize) -> Value));
        self.wasm_struct_new(&declarations, i.imm(), values, descriptor_value).map(Some)
    }

    /// `ref.get_desc`: the descriptor of the reference operand.
    fn wasm_get_descriptor_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        self.wasm_ref_get_desc(&declarations, i.imm(), read(self, i.register_b())).map(Some)
    }

    /// `ref.test` and `ref.cast` of a GC or i31 reference, and the descriptor forms.
    #[inline(always)]
    fn wasm_cast_step(
        &mut self,
        program: ProgramId,
        cast: bool,
        descriptor: bool,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        let target = ReferenceTarget::validated(i.imm());
        let reference = read(self, i.register_b());
        if descriptor {
            return self.wasm_descriptor_cast_step(program, cast, i, target, reference, read);
        }
        let bits = self.wasm_ref_test_or_cast(program, target, reference.to_bits(), cast)?;
        Ok(Some(if cast { Value::from_bits(bits) } else { Value::integer(bits as i32) }))
    }

    /// A descriptor test or cast: the reference's descriptor is the operand's, which its
    /// declarations check.
    #[inline(never)]
    fn wasm_descriptor_cast_step(
        &mut self,
        program: ProgramId,
        cast: bool,
        i: WideInstruction,
        target: ReferenceTarget,
        reference: Value,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        let valid = self.wasm_descriptor_matches(&declarations, target, reference, read(self, i.register_c()))?;
        if !cast {
            return Ok(Some(Value::integer(i32::from(valid))));
        }
        if valid {
            Ok(Some(reference))
        } else {
            Err(JsError::wasm_trap_error(crate::WasmTrap::DescriptorCastFailure))
        }
    }

    /// The reference steps the GC rules make: `ref.is_null`, `ref.as_non_null`, and the steps that
    /// stay with the general reference step (`ref.eq`, `i31`, external conversions).
    fn wasm_reference_step(
        &mut self,
        step: GcStep,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let value = read(self, i.register_b());
        Ok(Some(match step {
            GcStep::IsNull => Value::integer(HeapStage { vm: self }.is_null(value.to_bits()) as i32),
            GcStep::Equal => Value::integer(i32::from(value == read(self, i.register_c()))),
            GcStep::AsNonNull => {
                let check = crate::wasm::reference::NonNullCheck::from_tag(i.imm()).expect("validated non-null check");
                Value::from_bits(non_null(&mut HeapStage { vm: self }, value.to_bits(), check.trap())?)
            }
            GcStep::I31 => {
                let operator = crate::wasm::i31::I31Operator::from_tag(i.imm()).expect("validated i31 operator");
                let value = self.decode_wasm_value(value, operator.input_type())?;
                self.encode_wasm_value(operator.apply(value).map_err(JsError::wasm_trap_error)?)
            }
            GcStep::ExternConversion => {
                let conversion = crate::wasm::reference::ExternalConversion::from_tag(i.imm())
                    .expect("validated external conversion");
                self.decode_wasm_value(value, conversion.input_type())?;
                self.wasm_external_conversion(conversion, value)
            }
            step => unreachable!("{step:?} is no reference step"),
        }))
    }
}
