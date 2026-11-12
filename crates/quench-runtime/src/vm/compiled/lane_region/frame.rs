//! The lane specializer's register frame: `staged` register operations over the SSA homes.
use super::*;
#[path = "frame_calls.rs"] pub(super) mod calls;
#[path = "frame_truthiness.rs"] mod truthiness;
#[path = "frame_operands.rs"] mod operands;
pub(in crate::vm) use operands::{operand_int, operand_number};
use truthiness::register_truthiness;

/// The control edges a `step` transfers to, and the exit a value's cell truthiness leaves at.
#[derive(Clone, Copy)]
pub(super) enum Edges {
    /// A value operation: no control transfer.
    Value,
    /// A value operation whose operand proofs and guards leave at `miss`.
    ValueGuarded { miss: ir::Block },
    /// The jump target and the following instruction; a value's cell truthiness leaves at
    /// `miss`.
    Branch { target: ir::Block, following: ir::Block, miss: ir::Block },
    /// The jump target, which the lowering continues at itself.
    Jump,
}

/// The lane specializer's register frame (`crate::staged::Frame`): the SSA homes and the facts
/// `State` keeps, with the stage's builder emitting each operation's IR.
pub(super) struct EmitFrame<'a> {
    pub(super) registers: &'a mut Registers,
    pub(super) written: &'a mut CompoundBitSet,
    pub(super) state: &'a mut State,
    pub(super) edges: Edges,
    pub(super) constants: &'a [crate::bytecode::Constant],
    /// The entry's `this` binding, where the lowering has entry facts.
    pub(super) this: Option<Variable>,
    /// Whether this region may read or write the activation's local slots
    /// (`JsStepDispatch::native_locals`); a numeric region's activation has none.
    pub(super) locals: bool,
    /// The running body's dispatch payload, which semantic helpers take.
    pub(super) payload: ir::Value,
    /// The helper table a callout addresses.
    pub(super) helpers: LaneHelpers,
    /// The poll instructions a fault-polling body records for later patching.
    pub(super) polls: &'a mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
}

impl EmitFrame<'_> {
    /// The activation's local slots, read where an instruction uses them.
    fn native_locals(&self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        self.locals.then(|| super::super::emit_js_payload_word(stage.builder, self.payload,
            std::mem::offset_of!(super::super::JsStepDispatch, native_locals)))
    }
}

impl EmitFrame<'_> {
    /// A literal's allocation by the lane's own `helper`, whose DELETED answer leaves at the proof's miss edge.
    fn allocate(&mut self, stage: &mut Emit<'_, '_>, helper: LaneHelper, destination: Register, arguments: &[ir::Value],
        proof: EmitProof) -> Option<ir::Value> {
        let EmitProof::Allocation { miss } = proof else { return None };
        let payload = self.payload;
        let address = self.helpers.address(&mut stage.builder, helper);
        let value = super::super::emit_lane_helper_call(stage.builder, payload, address, &[&[payload], arguments].concat());
        calls::finish_allocation(stage, self, destination, value, miss);
        Some(value)
    }
}

impl crate::staged::Frame<Emit<'_, '_>> for EmitFrame<'_> {
    type Fact = RegisterFact;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> RegisterFact {
        self.state.values[usize::from(register)]
    }

    #[inline(always)]
    fn read(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        register_read(stage.builder, self.registers, self.state, register)
    }

    #[inline(always)]
    fn tagged(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        register_tagged(stage.builder, self.registers, self.state, register)
    }

    #[inline(always)]
    fn write(&mut self, stage: &mut Emit<'_, '_>, register: Register, value: ir::Value, fact: RegisterFact) {
        register_define(stage.builder, self.registers, self.written, self.state, register, value, fact);
    }

    #[inline(always)]
    fn move_register(&mut self, stage: &mut Emit<'_, '_>, destination: Register, source: Register) -> ir::Value {
        register_copy(stage.builder, self.registers, self.written, self.state, destination, source)
    }

    #[inline(always)]
    fn this(&mut self, stage: &mut Emit<'_, '_>) -> ir::Value {
        stage.builder.use_var(self.this.expect("a this read has entry facts"))
    }

    /// The entry's `this` and the facts the entry proved of it, kept beside the registers'.
    #[inline(always)]
    fn define_this(&mut self, stage: &mut Emit<'_, '_>, destination: Register, this: ir::Value) {
        register_define(stage.builder, self.registers, self.written, self.state, destination, this,
            RegisterFact::definition(Type::ANY, Representation::Tagged));
        let (index, entry) = (usize::from(destination), self.state.values.len());
        self.state.proofs.headers[index] = self.state.proofs.headers[entry];
        let readable = self.state.proofs.readable.contains(entry);
        self.state.proofs.set_readable(index, readable);
        self.state.proofs.receivers.insert(destination);
    }

    #[inline(always)]
    fn constant(&mut self, stage: &mut Emit<'_, '_>, constant: crate::staged::Constant)
        -> (ir::Value, RegisterFact) {
        let (representation, value) = if let Some(int) = constant.value.as_int() {
            (Representation::Int32, stage.builder.ins().iconst(types::I32, i64::from(int)))
        } else if let Some(number) = constant.value.as_number() {
            (Representation::Float64, stage.builder.ins().f64const(number))
        } else {
            (Representation::Tagged, stage.builder.ins().iconst(types::I64, constant.value.to_bits() as i64))
        };
        let fact = RegisterFact::admit(Fact { ty: constant.ty, representation,
            binding: BindingTime::Static, proof: Proof::Static, projection: Projection::Exact })
            .expect("constant's canonical semantic type");
        (value, fact)
    }

    /// A proven-slot read: the `CheckedObject` projection carries the layout and the slot
    /// read, so the frame adds no second decode.
    #[inline(always)]
    fn field_read(&mut self, stage: &mut Emit<'_, '_>, _base: Register, slot: u16, ty: Type,
        proof: EmitProof) -> Option<(ir::Value, RegisterFact)> {
        let EmitProof::Field(proof) = proof else { return None };
        Some((proof.read(stage.builder, slot),
            RegisterFact::definition(ty, Representation::Tagged)))
    }

    /// A proven-slot write: the projection's arena address, with the miss edge its layout
    /// guard uses.
    #[inline(always)]
    fn field_write(&mut self, stage: &mut Emit<'_, '_>, _base: Register, slot: u16, _ty: Type,
        value: ir::Value, proof: EmitProof) -> Option<()> {
        let EmitProof::Field(proof) = proof else { return None };
        let (Edges::ValueGuarded { miss } | Edges::Branch { miss, .. }) = self.edges else { return None };
        let address = proof.arena_address(stage.builder, slot, miss)?;
        stage.builder.ins().store(MemFlagsData::trusted(), value, address, 0);
        Some(())
    }

    /// A dense element under the view proof; a hole or an index past the view continues at
    /// the proof's cold block, exactly as the lowering's cold path did.
    #[inline(always)]
    fn index_read(&mut self, stage: &mut Emit<'_, '_>, index: ir::Value, proof: EmitProof) -> Option<ir::Value> {
        let EmitProof::Index { view, absent } = proof else { return None };
        Some(super::super::emit_dense_element(stage.builder, view, index, absent).1)
    }

    #[inline(always)]
    fn index_write(&mut self, stage: &mut Emit<'_, '_>, index: ir::Value, value: ir::Value,
        proof: EmitProof) -> Option<()> {
        let EmitProof::Index { view, absent } = proof else { return None };
        let (address, old) = super::super::emit_dense_element(stage.builder, view, index, absent);
        super::super::emit_dense_element_replace(stage.builder, view, address, old, value);
        Some(())
    }

    /// The VM field-store hit, whose false answer leaves at the miss edge.
    #[inline(always)] fn field_store_miss(&mut self, stage: &mut Emit<'_, '_>, object: ir::Value, atom: u32, site: u16, value: ir::Value, proof: EmitProof) -> Option<()> { calls::field_store_miss(self, stage, object, atom, site, value, proof) }

    /// The element store's host helper: its nonzero answer means the store landed.
    #[inline(always)]
    fn index_set_helper(&mut self, stage: &mut Emit<'_, '_>, base: ir::Value, key: ir::Value,
        value: ir::Value, proof: EmitProof) -> Option<()> {
        calls::index_set_helper(self, stage, base, key, value, proof)
    }


    /// The interrupt/collection safepoint: a fault-polling body records its poll instruction,
    /// any other leaves at the miss edge when the flag is set.
    #[inline(always)]
    fn safepoint(&mut self, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
        calls::safepoint(self, stage, proof)
    }

    /// An object literal: the lane's own allocator, whose DELETED answer (a due collection)
    /// leaves at the proof's miss edge.
    #[inline(always)]
    fn allocate_object(&mut self, stage: &mut Emit<'_, '_>, destination: Register, proof: EmitProof) -> Option<ir::Value> {
        self.allocate(stage, LaneHelper::NewObject, destination, &[], proof)
    }

    /// An array literal: the lane's own allocator, with its holes or its written elements.
    #[inline(always)]
    fn allocate_array(&mut self, stage: &mut Emit<'_, '_>, destination: Register, length: ir::Value,
        elements: ir::Value, proof: EmitProof) -> Option<ir::Value> {
        self.allocate(stage, LaneHelper::NewArray, destination, &[length, elements], proof)
    }

    /// An array literal of program constants (`lane_new_const_array`).
    #[inline(always)]
    fn allocate_const_array(&mut self, stage: &mut Emit<'_, '_>, destination: Register, constant: ir::Value,
        count: ir::Value, proof: EmitProof) -> Option<ir::Value> {
        self.allocate(stage, LaneHelper::NewConstArray, destination, &[constant, count], proof)
    }

    /// A two-property object literal (`lane_new_object_pair`).
    #[inline(always)]
    fn allocate_object_pair(&mut self, stage: &mut Emit<'_, '_>, destination: Register, site: u32, first: ir::Value,
        second: ir::Value, proof: EmitProof) -> Option<ir::Value> {
        let site = stage.builder.ins().iconst(types::I64, i64::from(site));
        self.allocate(stage, LaneHelper::NewObjectPair, destination, &[site, first, second], proof)
    }

    /// A typed array element: the kind's load, as a Number word.
    #[inline(always)]
    fn typed_index_read(&mut self, stage: &mut Emit<'_, '_>, index: ir::Value, proof: EmitProof) -> Option<ir::Value> {
        let EmitProof::TypedIndex { values, length, kind, absent } = proof else { return None };
        Some(super::typed_element(stage.builder, values, length, kind, index, absent, super::typed_read))
    }

    /// A typed array element store: the kind's conversion, then its width.
    #[inline(always)]
    fn typed_index_write(&mut self, stage: &mut Emit<'_, '_>, index: ir::Value, value: ir::Value,
        proof: EmitProof) -> Option<()> {
        let EmitProof::TypedIndex { values, length, kind, absent } = proof else { return None };
        super::typed_element(stage.builder, values, length, kind, index, absent, |builder, load, address| {
            super::typed_write(builder, load, address, value, absent);
            value
        });
        Some(())
    }

    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> Option<ir::Value> {
        let (Edges::Branch { miss, .. } | Edges::ValueGuarded { miss }) = self.edges else { unreachable!("truthiness needs a miss edge") };
        Some(register_truthiness(stage.builder, self.registers, self.state, register, (self.payload, self.helpers), miss))
    }

    #[inline(always)]
    fn branch(&mut self, stage: &mut Emit<'_, '_>, condition: ir::Value) {
        let Edges::Branch { target, following, .. } = self.edges else {
            unreachable!("a branch needs both edges")
        };
        stage.builder.ins().brif(condition, target, &[], following, &[]);
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Emit<'_, '_>) {
        debug_assert!(matches!(self.edges, Edges::Jump));
    }

    #[inline(always)]
    fn boolean(&mut self, stage: &mut Emit<'_, '_>, holds: ir::Value)
        -> (ir::Value, RegisterFact) {
        let (yes, no) = (stage.builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64),
            stage.builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
        let value = stage.builder.ins().select(holds, yes, no);
        (value, RegisterFact::definition(Type::language(crate::ValueType::Boolean),
            Representation::Tagged))
    }

    #[inline(always)]
    fn load_local(&mut self, stage: &mut Emit<'_, '_>, slot: u16, _miss: Option<ir::Block>)
        -> Option<(ir::Value, RegisterFact)> {
        let locals = self.native_locals(stage)?;
        let address = stage.builder.ins().iadd_imm_s(locals,
            i64::from(slot) * std::mem::size_of::<Value>() as i64);
        let value = stage.builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
        Some((value, RegisterFact::canonical()))
    }

    #[inline(always)]
    fn store_local(&mut self, stage: &mut Emit<'_, '_>, slot: u16, value: ir::Value,
        _miss: Option<ir::Block>) -> bool {
        let Some(locals) = self.native_locals(stage) else { return false };
        let address = stage.builder.ins().iadd_imm_s(locals,
            i64::from(slot) * std::mem::size_of::<Value>() as i64);
        stage.builder.ins().store(MemFlagsData::trusted(), value, address, 0);
        true
    }
}


/// `register`'s int32 view (`Registers::int`).
#[inline(always)]
pub(super) fn register_int(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    register: usize) -> Variable {
    *registers.int[register].get_or_insert_with(|| builder.declare_var(types::I32))
}

/// `register`'s float64 view (`Registers::float`).
#[inline(always)]
pub(super) fn register_float(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    register: usize) -> Variable {
    *registers.float[register].get_or_insert_with(|| builder.declare_var(types::F64))
}

/// `register`'s tagged bits, materializing them from its unboxed view where needed.
#[inline(always)]
pub(super) fn register_tagged(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index];
    if let TaggedView::Value(value) = fact.tagged { return value; }
    if !fact.tagged.current() {
        let bits = match fact.value.representation {
            Representation::Int32 => {
                let int = { let variable = register_int(builder, registers, index); builder.use_var(variable) };
                int_tagged(builder, int)
            }
            Representation::Float64 => {
                let number = { let variable = register_float(builder, registers, index); builder.use_var(variable) };
                emit_js_box_number(builder, number)
            }
            Representation::Tagged => unreachable!("tagged view is current"),
        };
        builder.def_var(registers.tagged[index], bits);
        state.values[index].tagged = TaggedView::Value(bits);
        return bits;
    }
    let bits = builder.use_var(registers.tagged[index]);
    state.values[index].tagged = TaggedView::Value(bits);
    bits
}

/// `register`'s word in its fact's representation.
#[inline(always)]
pub(super) fn register_read(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register) -> ir::Value {
    let index = usize::from(register);
    match state.values[index].value.representation {
        Representation::Tagged => register_tagged(builder, registers, state, register),
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            builder.use_var(variable)
        }
        Representation::Float64 => {
            let variable = register_float(builder, registers, index);
            builder.use_var(variable)
        }
    }
}

/// Define `register` with `value` and `fact`, retiring the proofs it carried.
#[inline(always)]
pub(super) fn register_define(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    written: &mut CompoundBitSet, state: &mut State, register: Register, value: ir::Value,
    mut fact: RegisterFact) {
    let index = usize::from(register);
    written.insert(index);
    let variable = match fact.value.representation {
        Representation::Tagged => registers.tagged[index],
        Representation::Int32 => register_int(builder, registers, index),
        Representation::Float64 => register_float(builder, registers, index),
    };
    builder.def_var(variable, value);
    if fact.value.representation == Representation::Tagged { fact.tagged = TaggedView::Value(value); }
    state.values[index] = fact;
    state.proofs.headers[index] = None;
    state.proofs.layouts.retain(|(owner, _), _| *owner != Some(register));
    state.proofs.receivers.remove(&register);
    state.proofs.fields.retain(|field| field.owner != Some(register));
}

/// `source`'s value into `destination`, with every fact proved of it.
#[inline(always)]
pub(super) fn register_copy(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    written: &mut CompoundBitSet, state: &mut State, destination: Register, source: Register) -> ir::Value {
    let source_index = usize::from(source);
    let fact = state.values[source_index];
    let value = register_read(builder, registers, state, source);
    let tagged = (fact.tagged.current() && fact.value.representation != Representation::Tagged)
        .then(|| register_tagged(builder, registers, state, source));
    let header = state.proofs.headers[source_index];
    let receiver = state.proofs.receivers.contains(&source);
    let layouts: Vec<_> = state.proofs.layouts.iter().filter_map(|((owner, shape), projection)|
        (*owner == Some(source)).then_some((*shape, *projection))).collect();
    let fields: smallvec::SmallVec<[_; INLINE_FIELD_BINDINGS]> = state.proofs.fields.iter()
        .filter(|field| field.owner == Some(source))
        .map(|field| FieldValue { owner: Some(destination), ..*field }).collect();
    register_define(builder, registers, written, state, destination, value, fact);
    if let Some(bits) = tagged {
        builder.def_var(registers.tagged[usize::from(destination)], bits);
        state.values[usize::from(destination)].tagged = TaggedView::Value(bits);
    }
    state.proofs.headers[usize::from(destination)] = header;
    state.proofs.set_readable(usize::from(destination), state.proofs.readable.contains(source_index));
    for (shape, projection) in layouts { state.proofs.layouts.insert((Some(destination), shape), projection); }
    if receiver { state.proofs.receivers.insert(destination); }
    state.proofs.fields.extend(fields);
    value
}

impl crate::staged::NumericFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn int_operand(&mut self, stage: &mut Emit<'_, '_>, operand: crate::bytecode::Operand,
        miss: ir::Block) -> ir::Value {
        operand_int(stage.builder, self.registers, self.state, self.constants, operand, miss)
            .expect("a numeric operand is a register or an immediate")
    }

    fn number_operand(&mut self, stage: &mut Emit<'_, '_>, operand: crate::bytecode::Operand,
        miss: ir::Block) -> ir::Value {
        operand_number(stage.builder, self.registers, self.state, self.constants, operand, miss)
            .expect("a numeric operand is a register or an immediate")
    }

    fn prove_number(&mut self, stage: &mut Emit<'_, '_>, register: Register, miss: ir::Block) -> Option<()> {
        // A register its facts do not prove a Number is taken for an int32, tested once.
        if !self.state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            register_int_value(stage.builder, self.registers, self.state, register, miss);
        }
        Some(())
    }

    fn miss(&self) -> ir::Block {
        match self.edges {
            Edges::ValueGuarded { miss } => miss,
            _ => unreachable!("a numeric operation has a miss edge"),
        }
    }

    fn local_miss(&self) -> Option<ir::Block> {
        matches!(self.edges, Edges::ValueGuarded { .. }).then(|| self.miss())
    }

    fn int_fact(&self) -> RegisterFact {
        RegisterFact::definition(Type::INT32, Representation::Int32)
    }

    fn number_fact(&self) -> RegisterFact {
        RegisterFact::definition(Type::NUMBER, Representation::Float64)
    }
}

/// `register`'s int32 value, proving it where the fact does not.
pub(super) fn register_int_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index];
    let int = match fact.value.representation {
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            return builder.use_var(variable);
        }
        Representation::Float64 if !fact.tagged.current() => {
            let variable = register_float(builder, registers, index);
            let number = builder.use_var(variable);
            let (int, exact) = emit_js_exact_int32(builder, number);
            if !fact.value.ty.is_subtype(Type::INT32) {
                emit_js_guard(builder, exact, otherwise);
            }
            int
        }
        Representation::Tagged | Representation::Float64 => {
            let bits = register_tagged(builder, registers, state, register);
            if !fact.value.ty.is_subtype(Type::INT32) {
                let integer = is_int_tagged(builder, bits);
                emit_js_guard(builder, integer, otherwise);
            }
            builder.ins().ireduce(types::I32, bits)
        }
    };
    let variable = register_int(builder, registers, index);
    builder.def_var(variable, int);
    state.values[index] = RegisterFact { tagged: state.values[index].tagged,
        ..RegisterFact::definition(Type::INT32, Representation::Int32) };
    int
}

/// `register`'s binary64 value, proving Number where the fact does not.
pub(super) fn register_number_value(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index].value;
    match fact.representation {
        Representation::Float64 => {
            let variable = register_float(builder, registers, index);
            builder.use_var(variable)
        }
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            let int = builder.use_var(variable);
            builder.ins().fcvt_from_sint(types::F64, int)
        }
        Representation::Tagged => {
            let bits = register_tagged(builder, registers, state, register);
            let number = if fact.ty.is_subtype(Type::NUMBER) {
                emit_known_number(builder, bits)
            } else {
                emit_js_tagged_number(builder, bits, otherwise)
            };
            let variable = register_float(builder, registers, index);
            builder.def_var(variable, number);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::NUMBER, Representation::Float64) };
            number
        }
    }
}
