//! The Wasm GC opcodes of the shared vocabulary: operations of register values alone, with no
//! frame state, no control flow and no guest code, whose trap writes nothing. The general step
//! and the lane make them with the same step (`Vm::wasm_gc_value_step`).

use super::super::*;
use super::super::wasm_gc::{ArrayInitialization, GcFieldIndex};
use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind, GcField, GcFieldAccess, StructConstruction};
use crate::wasm::reference::ReferenceTarget;

/// Applies `$then!` to the opcodes `Vm::wasm_gc_value_step` makes: the one list that
/// `wasm_gc_value_op` and the lane's handler of each opcode derive from.
macro_rules! wasm_gc_value_ops {
    ($then:ident) => {
        $then!(WasmRefIsNull WasmRefAsNonNull WasmRefEq WasmI31 WasmExternalConversion
            WasmArrayGet WasmArrayGetS WasmArrayGetU WasmArraySet WasmArrayLen
            WasmArrayNewData WasmArrayNewElem WasmArrayInitData WasmArrayInitElem WasmArrayFill WasmArrayCopy
            WasmArrayNew WasmArrayNewDefault WasmArrayNewFixed
            WasmStructGet WasmStructGetS WasmStructGetU WasmStructSet
            WasmStructNew WasmStructNewDefault WasmStructNewDesc WasmStructNewDefaultDesc WasmRefGetDesc
            WasmRefTest WasmRefCast WasmDescriptorTest WasmDescriptorCast WasmIndirectTarget WasmRefFunc)
    };
}
pub(in crate::vm) use wasm_gc_value_ops;

/// Whether `op` is one of the operations `Vm::wasm_gc_value_step` makes.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_value_op(op: Op) -> bool {
    macro_rules! listed {
        ($($name:ident)*) => { matches!(op, $(Op::$name)|*) };
    }
    wasm_gc_value_ops!(listed)
}

/// Whether `op`, a `wasm_gc_value_op`, can allocate a heap cell, which can make a collection
/// due: every one but the field accesses, the array length and the reference tests, which only
/// read or write cells that exist.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_allocates(op: Op) -> bool {
    !(wasm_gc_field_op(op)
        || matches!(
            op,
            Op::WasmArrayLen | Op::WasmRefIsNull | Op::WasmRefAsNonNull | Op::WasmRefEq | Op::WasmRefTest | Op::WasmRefCast
        ))
}

/// Whether `op`, a `wasm_gc_value_op`, reads or writes a struct field or an array element.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_field_op(op: Op) -> bool {
    matches!(
        op,
        Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet
            | Op::WasmArrayGet | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArraySet
    )
}

/// A field access's operands (`Vm::wasm_field_operands`): the object, the field or element, its
/// storage and access, and the value a write stores.
struct FieldOperands {
    reference: Value,
    location: GcFieldIndex,
    field: GcField,
    access: GcFieldAccess,
    value: Option<Value>,
}

/// Whether `op`, a `wasm_gc_value_op`, reads the running instance's environment: the
/// operations that name one of its functions or tables' targets.
#[inline(always)] pub(in crate::vm) const fn wasm_gc_reads_instance(op: Op) -> bool {
    matches!(op, Op::WasmIndirectTarget | Op::WasmRefFunc)
}

impl<H: Host> Vm<H> {
    /// One of the `wasm_gc_value_op`s of `program`'s instance `env`, of the registers `read` answers: the value
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
        match i.op() {
            op if wasm_gc_field_op(op) => self.wasm_field_step(i, read),
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
            Op::WasmIndirectTarget | Op::WasmRefFunc => self.wasm_call_target_step(program, env, i, read).map(Some),
            _ => self.wasm_reference_step(i, read),
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
        if i.op() == Op::WasmIndirectTarget {
            return self.wasm_indirect_target_of_operand(program, read(self, i.register_b()), read(self, i.register_c()), i.imm());
        }
        let residual = self.programs.get(program).ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
        self.wasm_function_reference(&residual, i.imm(), env)
    }

    /// The declarations of `program`, a Wasm module's.
    fn wasm_module_declarations(&self, program: ProgramId) -> Result<crate::WasmTypes, JsError> {
        self.programs.wasm_signatures(program)
            .map(|pool| pool.declarations.clone())
            .ok_or_else(|| JsError::validation("Wasm GC operation requires module declarations".into()))
    }

    /// The operands of the field access `i` of a struct or an array, read from its registers.
    #[inline(always)]
    fn wasm_field_operands(
        &self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<FieldOperands, JsError> {
        let access = GcFieldAccess::from_op(i.op()).expect("field opcode");
        let write = access == GcFieldAccess::Write;
        let field = GcField::from_tag(i.imm()).expect("validated field operand");
        if matches!(i.op(), Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet) {
            let reference = read(self, if write { i.register_a() } else { i.register_b() });
            let value = write.then(|| read(self, i.register_b()));
            return Ok(FieldOperands { reference, location: GcFieldIndex::Struct(field.index()), field, access, value });
        }
        let (reference, index) =
            if write { (i.register_a(), i.register_b()) } else { (i.register_b(), i.register_c()) };
        let location = GcFieldIndex::Array(Self::wasm_u32_operand(read(self, index))?);
        let value = write.then(|| read(self, i.register_c()));
        Ok(FieldOperands { reference: read(self, reference), location, field, access, value })
    }

    /// The field access `i` of a GC object, as a struct or array op makes it
    /// (`wasm_field_access`).
    #[inline(always)]
    pub(in crate::vm) fn wasm_gc_field_hit(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Option<Value>>, JsError> {
        let FieldOperands { reference, location, field, access, value } = self.wasm_field_operands(i, read)?;
        Ok(self.wasm_field_access(reference, location, field, access, value))
    }

    /// One field access of a GC object: its value if it reads, `None` if the reference is
    /// null, not an object, or past the element bound.
    #[inline(always)]
    pub(in crate::vm) fn wasm_field_access(
        &mut self,
        reference: Value,
        location: GcFieldIndex,
        field: GcField,
        access: GcFieldAccess,
        value: Option<Value>,
    ) -> Option<Option<Value>> {
        let (GcFieldIndex::Struct(index) | GcFieldIndex::Array(index)) = location;
        let index = index as usize;
        // A read takes the cell without the write barrier, which would remember every old
        // object a read touches for the next young collection to scan.
        let Some(value) = value else {
            let Some(Cell::WasmGc { fields, .. }) = self.heap.get(reference) else { return None };
            return fields.get(index).map(|slot| Some(field.load(access, *slot)));
        };
        let Some(Cell::WasmGc { fields, .. }) = self.heap.get_mut(reference) else { return None };
        fields.get_mut(index).map(|slot| {
            *slot = field.store(value);
            None
        })
    }

    /// `wasm_gc_field_hit`, raising the trap a miss makes. Validation proved the field's
    /// storage and access for the reference's static type, which every subtype shares, so
    /// only null and the element bound remain to check.
    #[inline(always)]
    fn wasm_field_step(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value + Copy,
    ) -> Result<Option<Value>, JsError> {
        self.wasm_gc_field_hit(i, read)?.ok_or_else(|| {
            let operands = self.wasm_field_operands(i, read).expect("a field access that read its operands");
            Self::wasm_gc_field_trap(operands.reference, operands.location)
        })
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
        let operands = window.filter(|_| !mode.defaulted()).map(|window| window.base);
        let field = |vm: &Self, index: usize| read(vm, operands.unwrap_or_default() + index as u16);
        let values = operands.map(|_| (usize::from(fields), &field as &dyn Fn(&Self, usize) -> Value));
        self.wasm_struct_new(&declarations, i.imm(), values, descriptor).map(Some)
    }

    #[inline(always)]
    fn wasm_cast_step(
        &mut self,
        program: ProgramId,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Option<Value>, JsError> {
        let target = ReferenceTarget::validated(i.imm());
        let reference = read(self, i.register_b());
        let fast = (i.op() == Op::WasmRefTest || i.op() == Op::WasmRefCast)
            .then(|| self.wasm_gc_object_cast(program, target, reference))
            .flatten();
        let (valid, failure) = match fast {
            Some(valid) => (valid, crate::WasmTrap::CastFailure),
            None => self.wasm_cast_checked(program, i, target, reference, read)?,
        };
        match i.op() {
            Op::WasmRefTest | Op::WasmDescriptorTest => Ok(Some(Value::integer(i32::from(valid)))),
            _ if valid => Ok(Some(reference)),
            _ => Err(JsError::wasm_trap_error(failure)),
        }
    }

    /// Whether `reference` passes the cast `i` to `target`, and the trap it raises if not: a
    /// descriptor cast, which reads its descriptor operand, or a reference the object display
    /// does not decide.
    #[inline(never)]
    fn wasm_cast_checked(
        &self,
        program: ProgramId,
        i: WideInstruction,
        target: ReferenceTarget,
        reference: Value,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<(bool, crate::WasmTrap), JsError> {
        let declarations = self.wasm_module_declarations(program)?;
        if matches!(i.op(), Op::WasmDescriptorTest | Op::WasmDescriptorCast) {
            let valid = self.wasm_descriptor_matches(&declarations, target, reference, read(self, i.register_c()))?;
            return Ok((valid, crate::WasmTrap::DescriptorCastFailure));
        }
        let target = target.reference_type().expect("validated reference target");
        let ty = declarations.callable_value_type(wasmparser::ValType::Ref(target))
            .ok_or_else(|| JsError::validation("invalid reference cast declaration".into()))?;
        Ok((self.wasm_reference_valid_in(reference, ty, Some(&declarations)), crate::WasmTrap::CastFailure))
    }

    /// Whether a cast to `target` admits `reference` when that is null, an i31 or a GC
    /// object, read from the object's supertype display or kind; `None` otherwise.
    #[inline(always)]
    fn wasm_gc_object_cast(&self, program: ProgramId, target: ReferenceTarget, reference: Value) -> Option<bool> {
        use crate::wasm::reference::TargetHeap;
        let heap = target.heap()?;
        if reference.is_null() {
            return Some(target.is_nullable());
        }
        let object = match self.heap.get(reference) {
            Some(Cell::WasmGc { declarations: owner, ty, .. }) => Some((owner, *ty)),
            _ if crate::wasm::i31::bits(reference).is_some() => None,
            _ => return None,
        };
        match (heap, object) {
            (TargetHeap::Module { index, exact }, Some((owner, ty))) => {
                let declarations = &self.programs.wasm_signatures(program)?.declarations;
                Some(if exact {
                    owner.equivalent(ty as usize, declarations, index as usize)
                } else {
                    owner.is_subtype(ty as usize, declarations, index as usize)
                })
            }
            (TargetHeap::Module { .. }, None) => Some(false),
            (TargetHeap::Abstract(abstract_type), object) => Self::wasm_abstract_cast(abstract_type, object),
        }
    }

    /// Whether an abstract cast target admits a GC object of `object`'s type, or an i31 when
    /// `object` is `None`; `None` for a target outside the internal hierarchy.
    fn wasm_abstract_cast(target: wasmparser::AbstractHeapType, object: Option<(&crate::WasmTypes, u32)>) -> Option<bool> {
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
