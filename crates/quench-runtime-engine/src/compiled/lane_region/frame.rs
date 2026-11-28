//! The lane specializer's register frame: `staged` register operations over the SSA homes.
use quench_runtime::staged::{Float64, Int32};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;
#[path = "frame_calls.rs"] pub(crate) mod calls;
#[path = "frame_call_out.rs"] pub(crate) mod call_out;
#[path = "frame_truthiness.rs"] pub(crate) mod truthiness;
#[path = "frame_operands.rs"] pub(crate) mod operands;
pub(crate) use operands::{held_long, number_word, operand_int, operand_long, operand_number, operand_to_int32, integral_index_value, register_int_value, register_long_value, register_number_value};
use truthiness::register_truthiness;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The lane's register file.
pub(crate) const FRAME: quench_runtime::staged::Access = quench_runtime::staged::Region::Frame.mutable();

/// The control edges a `step` transfers to, and the exit a value's cell truthiness leaves at.
#[derive(Clone, Copy)]
pub(crate) enum Edges {
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

/// What a step's slow cases need of the lowering, where it made them: a call-out's record, a
/// capture access's reach.
#[derive(Clone, Copy, Default)]
pub(crate) struct Sites<'a> {
    pub(crate) call_out: Option<&'a call_out::CallOutSite>,
    pub(crate) capture: Option<&'a super::captures::CaptureSite>,
    /// The Stable `prototype` link of an `instanceof`'s constructor (`Lowering::hold_instanceof`).
    pub(crate) prototype: Option<u32>,
}

/// The lane specializer's register frame (`quench_runtime::staged::Frame`): the SSA homes and the facts
/// `State` keeps, with the stage's builder emitting each operation's IR.
pub(crate) struct EmitFrame<'a> {
    pub(crate) registers: &'a mut Registers,
    pub(crate) written: &'a mut quench_runtime::dataflow::BitSet,
    pub(crate) state: &'a mut State,
    pub(crate) edges: Edges,
    pub(crate) constants: &'a [quench_runtime::bytecode::Constant],
    /// The entry's `this` binding, where the lowering has entry facts.
    pub(crate) this: Option<Variable>,
    /// Whether this region may read or write the activation's local slots
    /// (`JsStepDispatch::native_locals`); a numeric region's activation has none.
    pub(crate) locals: bool,
    /// The running body's dispatch payload, which semantic helpers take.
    pub(crate) payload: ir::Value,
    /// The helper table a callout addresses.
    pub(crate) helpers: LaneHelpers,
    /// The poll instructions a fault-polling body records for later patching.
    pub(crate) polls: &'a mut smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// The heap identities the emitted code names (`FoldedRoots::embed_heap_identity`).
    pub(crate) roots: &'a mut quench_runtime::folded_roots::FoldedRoots,
    /// The Number return edges, joined at the end of the body.
    pub(crate) number_returns: &'a mut smallvec::SmallVec<[NumberReturn; 1]>,
    /// What a named field access is proved under (`PropertyFrame`); none for any other step.
    pub(crate) fields: Option<super::frame_fields::FieldFacts<'a>>,
    /// The record a call-out calls for, where the lowering made one (`call_outs`).
    pub(crate) call_out: Option<&'a call_out::CallOutSite>,
    /// The view of a loop's hoisted dense array the element row reads through, proved at the loop's
    /// entering edge (`ElementFrame::kept_view`).
    pub(crate) held: Option<super::frame_elements::Held>,
    /// Whether a read through `held` leaves at a hole or past the backing instead of calling
    /// the VM's read, whose call would spill every value live across the loop.
    pub(crate) hole_leaves: bool,
    /// Whether no operation of the unit may move an array's backing or the record table
    /// (`fixed_arrays`), so the table's base word is read-only.
    pub(crate) arrays_fixed: bool,
    /// The call or return a step of the call family runs for (`call_frames`).
    pub(crate) site: Option<super::call_frames::Site<'a>>,
    /// Whether the call `call_entry` last planned passes arguments proved to have its callee's proven entry's
    /// parameter types (`call_frames::EmitFrame::arguments_proven`), which `call_enter` reads.
    pub(crate) proven_arguments: std::cell::Cell<bool>,
    /// Whether that call reads its closure and captures from the callee's bound words (`DirectCallee::bound`).
    pub(crate) rebinds: std::cell::Cell<bool>,
    /// What a capture access reaches, where the lowering made it (`captures`).
    pub(crate) capture: Option<&'a super::captures::CaptureSite>,
    /// Whether an IsHTMLDDA object can exist (`VmFacts::html_dda`).
    pub(crate) html_dda: bool,
    /// The Stable `prototype` link of the step's `instanceof` constructor.
    pub(crate) prototype: Option<u32>,
}

/// The address of the activation's local `slot` (`JsStepDispatch::native_locals`), which an
/// instruction or a handler's entry reads or writes.
pub(crate) fn local_slot_address(builder: &mut FunctionBuilder<'_>, payload: ir::Value, slot: u16) -> ir::Value {
    let locals = super::super::emit_js_payload_word(builder, payload,
        std::mem::offset_of!(super::super::JsStepDispatch, head.native_locals));
    emit(builder).raw_binary_imm(Binary::Add, locals, i64::from(slot) * std::mem::size_of::<Value>() as i64)
}

impl quench_runtime::staged::Frame<Emit<'_, '_>> for EmitFrame<'_> {
    type Fact = RegisterFact;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> RegisterFact {
        self.state.values[usize::from(register)]
    }

    #[inline(always)]
    fn register_type(&self, register: Register) -> Option<Type> {
        Some(self.state.values[usize::from(register)].value.ty)
    }

    /// Kept beside the state and applied to the successor the jump's edge reaches (`State::narrow`).
    #[inline(always)]
    fn refine(&mut self, register: Register, ty: Type, edge: quench_runtime::staged::Edge) {
        self.state.refinements.push((edge, register, ty));
    }

    #[inline(always)]
    fn read(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        register_read(stage.builder, self.registers, self.state, register)
    }

    #[inline(always)]
    fn tagged(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        let word = register_tagged(stage.builder, self.registers, self.state, register);
        // The register's type at this point is a fact of the word for this step (R2): a row's class
        // test of it the fact decides is a constant (`Stage::fact`).
        let word = stage.builder.func.dfg.resolve_aliases(word);
        stage.note_fact(word, self.state.values[usize::from(register)].value.ty);
        word
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
        self.state.proofs.receivers.insert(destination);
    }

    #[inline(always)]
    fn constant(&mut self, stage: &mut Emit<'_, '_>, constant: quench_runtime::staged::Constant)
        -> (ir::Value, RegisterFact) {
        let (representation, value) = if let Some(int) = constant.value.as_int() {
            (Representation::Int32, stage.raw_const(Lane::I32, i64::from(int)))
        } else if let Some(number) = constant.value.as_number() {
            (Representation::Float64, stage.raw_const(Lane::F64, number.to_bits() as i64))
        } else {
            (Representation::Tagged, stage.raw_const(Lane::I64, self.roots.embed_heap_identity(constant.value) as i64))
        };
        let fact = RegisterFact::admit(Fact { ty: constant.ty, representation,
            binding: BindingTime::Static, proof: Proof::Static, projection: Projection::Exact })
            .expect("constant's canonical semantic type");
        (value, fact)
    }

    /// A row's slow case: the lane's own helper makes the VM's operation in place (`call_out::vm_step`).
    #[inline(always)]
    fn vm_step(&mut self, stage: &mut Emit<'_, '_>, operation: quench_runtime::staged::CallOut, result: Option<Register>)
        -> Option<(ir::Value, RegisterFact)> {
        call_out::vm_step(self, stage, operation, result)
    }

    /// A capture read: the site's fast and slow cases (`captures::read_capture`), whose slow case
    /// finds a root's projected binding itself; a miss leaves at the step's miss edge.
    #[inline(always)]
    fn capture_slot(&mut self, stage: &mut Emit<'_, '_>, depth: u16, slot: u16, _root: bool) -> Option<(ir::Value, RegisterFact)> {
        let site = self.capture?;
        let Edges::ValueGuarded { miss } = self.edges else { return None };
        let value = super::captures::read_capture(stage.builder, (self.payload, self.helpers), site, (depth, slot), miss);
        Some((value, RegisterFact::definition(Type::ANY, Representation::Tagged)))
    }

    /// A capture store by the lane's own store, whose refusal leaves at the miss edge
    /// (`captures::store_capture`).
    #[inline(always)]
    fn store_capture(&mut self, stage: &mut Emit<'_, '_>, depth: u16, slot: u16, value: ir::Value) -> Option<()> {
        let site = self.capture?;
        let Edges::ValueGuarded { miss } = self.edges else { return None };
        super::captures::store_capture(stage.builder, (self.payload, self.helpers), site, (depth, slot), value, miss);
        Some(())
    }

    #[inline(always)]
    fn index_write(&mut self, stage: &mut Emit<'_, '_>, index: ir::Value, value: ir::Value,
        proof: EmitProof) -> Option<()> {
        let EmitProof::Index { view, absent, in_bounds } = proof else { return None };
        let (address, old) = super::super::emit_dense_element(stage.builder, view, index, absent, in_bounds);
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

    /// An empty object, object or array literal, constant array, closure, template or concatenation: the lane's own allocator.
    #[inline(always)]
    fn allocate_value(&mut self, stage: &mut Emit<'_, '_>, destination: Register, allocation: quench_runtime::staged::Allocation,
        proof: EmitProof) -> Option<ir::Value> { calls::allocate_value(self, stage, destination, allocation, proof) }

    /// The heap's array record table, where the frame reads live array records. The engine's frame offers
    /// none, so a row that needs the table takes its call-out (as `places.rs` `heap_records` does in bjit).
    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut Emit<'_, '_>) -> Option<ir::Value> { None }

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
        stage.branch_if(condition, (target, &[]), (following, &[]));
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Emit<'_, '_>) {
        debug_assert!(matches!(self.edges, Edges::Jump));
    }

    #[inline(always)]
    fn boolean(&mut self, stage: &mut Emit<'_, '_>, holds: ir::Value)
        -> (ir::Value, RegisterFact) {
        let value = super::boolean_word(stage.builder, holds);
        (value, RegisterFact::definition(Type::language(quench_runtime::ValueType::Boolean),
            Representation::Tagged))
    }

    /// The activation completes with `register`: by status at the record the lowering made a
    /// site for (`call_frames::complete_activation`), else a Number return of a restartable scalar
    /// entry, which proves the register a Number first.
    #[inline(always)]
    fn ret(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> Option<()> {
        if matches!(self.site, Some(super::call_frames::Site::Return(_))) {
            let value = <Self as quench_runtime::staged::Frame<Emit<'_, '_>>>::tagged(self, stage, register);
            return super::call_frames::complete_activation(self, stage, value);
        }
        let Edges::ValueGuarded { miss } = self.edges else { return None };
        quench_runtime::staged::NumericFrame::prove_number(self, stage, register, miss)?;
        calls::number_return(self, stage, register)
    }

    #[inline(always)]
    fn load_local(&mut self, stage: &mut Emit<'_, '_>, slot: u16, _miss: Option<ir::Block>)
        -> Option<(ir::Value, RegisterFact)> {
        let address = self.locals.then(|| local_slot_address(stage.builder, self.payload, slot))?;
        let value = stage.raw_load(Lane::I64, regions::frame(), address, 0);
        Some((value, RegisterFact::canonical()))
    }

    #[inline(always)]
    fn store_local(&mut self, stage: &mut Emit<'_, '_>, slot: u16, value: ir::Value,
        _miss: Option<ir::Block>) -> bool {
        let Some(address) = self.locals.then(|| local_slot_address(stage.builder, self.payload, slot)) else { return false };
        stage.store_word(regions::frame(), value, address, 0);
        true
    }
}

/// `register`'s int32 view (`Registers::int`).
#[inline(always)]
pub(crate) fn register_int(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    register: usize) -> Variable {
    *registers.int[register].get_or_insert_with(|| builder.declare_var(types::I32))
}

/// `register`'s 64-bit integer view (`Registers::long`).
#[inline(always)]
pub(crate) fn register_long(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    register: usize) -> Variable {
    *registers.long[register].get_or_insert_with(|| builder.declare_var(types::I64))
}

/// `register`'s float64 view (`Registers::float`).
#[inline(always)]
pub(crate) fn register_float(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    register: usize) -> Variable {
    *registers.float[register].get_or_insert_with(|| builder.declare_var(types::F64))
}

/// `register`'s tagged bits, materializing them from its unboxed view where needed.
#[inline(always)]
pub(crate) fn register_tagged(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register) -> ir::Value {
    let index = usize::from(register);
    let fact = state.values[index];
    let mut current = fact.tagged.current();
    if let TaggedView::Value(value) = fact.tagged {
        if defined_here(builder, value) { return value; }
        // A word made in another block may be a sibling path's, which this block does not see: a tagged
        // register's variable is its view everywhere, an unboxed one boxes its word again here.
        current = fact.value.representation == Representation::Tagged;
    }
    if !current {
        let bits = super::number_domains::box_unboxed(builder, registers, fact.value.representation, index);
        builder.def_var(registers.tagged[index], bits);
        state.values[index].tagged = TaggedView::Value(bits);
        return bits;
    }
    let bits = builder.use_var(registers.tagged[index]);
    state.values[index].tagged = TaggedView::Value(bits);
    bits
}

/// Whether `value` is defined in the block the builder is in: there it dominates every later use.
fn defined_here(builder: &FunctionBuilder<'_>, value: ir::Value) -> bool {
    let block = match builder.func.dfg.value_def(value) {
        ir::ValueDef::Result(inst, _) => builder.func.layout.inst_block(inst),
        ir::ValueDef::Param(block, _) => Some(block),
        ir::ValueDef::Union(..) => None,
    };
    block.is_some() && block == builder.current_block()
}

/// `register`'s word in its fact's representation.
#[inline(always)]
pub(crate) fn register_read(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    state: &mut State, register: Register) -> ir::Value {
    let index = usize::from(register);
    match state.values[index].value.representation {
        Representation::Tagged => register_tagged(builder, registers, state, register),
        Representation::Int32 => {
            let variable = register_int(builder, registers, index);
            builder.use_var(variable)
        }
        Representation::Int64 => {
            let variable = register_long(builder, registers, index);
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
pub(crate) fn register_define(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    written: &mut quench_runtime::dataflow::BitSet, state: &mut State, register: Register, value: ir::Value,
    mut fact: RegisterFact) {
    let index = usize::from(register);
    written.insert(index);
    // The definition names another value: a kept receiver aliasing the register lets go of it.
    super::virtual_objects::aliases::release_alias(&mut state.virtuals, register);
    let variable = match fact.value.representation {
        Representation::Tagged => registers.tagged[index],
        Representation::Int32 => register_int(builder, registers, index),
        Representation::Int64 => register_long(builder, registers, index),
        Representation::Float64 => register_float(builder, registers, index),
    };
    // Each representation has its own variable (`Registers`), so a value whose lane disagrees with
    // the fact cannot be stored there: the body's accounting missed a path and is declined
    // (`note_mismatch`, `lower`). The value is converted only so the builder stays well-formed for
    // the rest of the discarded lowering.
    let expected = match fact.value.representation {
        Representation::Tagged | Representation::Int64 => types::I64,
        Representation::Int32 => types::I32,
        Representation::Float64 => types::F64,
    };
    let actual = builder.func.dfg.value_type(value);
    let value = if actual == expected { value } else {
        super::note_mismatch();
        match (actual.is_int(), expected.is_int()) {
            (true, true) if actual.bits() > expected.bits() => builder.ins().ireduce(expected, value),
            (true, true) => builder.ins().uextend(expected, value),
            (false, true) => builder.ins().fcvt_to_sint_sat(expected, value),
            (true, false) => builder.ins().fcvt_from_sint(types::F64, value),
            (false, false) => builder.ins().fpromote(expected, value),
        }
    };
    builder.def_var(variable, value);
    if fact.value.representation == Representation::Tagged { fact.tagged = TaggedView::Value(value); }
    // The definition's word takes the shape its type names: a fact of that object (`kept_shapes`).
    let kept = fact.value.ty.object_shape().filter(|_| fact.value.representation == Representation::Tagged);
    state.values[index] = fact;
    state.proofs.homed.remove(index);
    state.proofs.receivers.remove(&register);
    if let Some(shape) = kept { state.proofs.kept_shapes.insert(builder.func.dfg.resolve_aliases(value), shape); }
    state.proofs.typed.retain(|view| view.register != register);
}

/// `source`'s value into `destination`, with every fact proved of it.
#[inline(always)]
pub(crate) fn register_copy(builder: &mut FunctionBuilder<'_>, registers: &mut Registers,
    written: &mut quench_runtime::dataflow::BitSet, state: &mut State, destination: Register, source: Register) -> ir::Value {
    let source_index = usize::from(source);
    let fact = state.values[source_index];
    let value = register_read(builder, registers, state, source);
    let tagged = (fact.tagged.current() && fact.value.representation != Representation::Tagged)
        .then(|| register_tagged(builder, registers, state, source));
    let receiver = state.proofs.receivers.contains(&source);
    let typed: smallvec::SmallVec<[_; 2]> = state.proofs.typed.iter().filter(|view| view.register == source)
        .map(|view| super::TypedProof { register: destination, ..*view }).collect();
    register_define(builder, registers, written, state, destination, value, fact);
    if let Some(bits) = tagged {
        builder.def_var(registers.tagged[usize::from(destination)], bits);
        state.values[usize::from(destination)].tagged = TaggedView::Value(bits);
    }
    if receiver { state.proofs.receivers.insert(destination); }
    state.proofs.typed.extend(typed);
    value
}

impl quench_runtime::staged::NumericFrame<Emit<'_, '_>> for EmitFrame<'_> {
    /// A row the VM's operator answers (`Operation::Binary`, `Unary`) takes the VM's step: `vm_step`
    /// calls the helper where the lowering made the record a call-out site, and leaves at the miss
    /// edge where it made none. Answering `false` left the row `Unresolved`, which a lane body ran as no
    /// instruction at all: `10 ** (b + 1)` returned its result register's last value.
    fn general_operators(&self) -> bool { true }

    /// A register's tagged bits, or an immediate constant's.
    fn operand_word(&mut self, stage: &mut Emit<'_, '_>, operand: quench_runtime::bytecode::Operand) -> Option<ir::Value> {
        use quench_runtime::bytecode::OperandKind;
        match operand.kind()? {
            OperandKind::Register => Some(register_tagged(stage.builder, self.registers, self.state, operand.register_index()?)),
            OperandKind::Constant => {
                let value = self.constants.get(usize::from(operand.payload())).and_then(super::super::facts::immediate)?;
                Some(stage.raw_const(Lane::I64, value.to_bits() as i64))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn int_operand(&mut self, stage: &mut Emit<'_, '_>, operand: quench_runtime::bytecode::Operand,
        miss: ir::Block) -> Option<Int32<ir::Value>> {
        Some(operand_int(stage.builder, self.registers, self.state, self.constants, operand, miss)
            .expect("a numeric operand is a register or an immediate"))
    }

    /// A register its facts prove an int32: its int view, whose test (if any) never fails.
    fn held_int(&mut self, stage: &mut Emit<'_, '_>, operand: quench_runtime::bytecode::Operand) -> Option<Int32<ir::Value>> {
        let register = operand.register_index()?;
        // Only a register its type proves: `register_int_value` reads it by that fact, with no test.
        quench_runtime::staged::IntFact::of_type(self.state.values[usize::from(register)].value.ty)?;
        let miss = quench_runtime::staged::NumericFrame::miss(self);
        Some(register_int_value(stage.builder, self.registers, self.state, register, miss))
    }

    fn number_operand(&mut self, stage: &mut Emit<'_, '_>, operand: quench_runtime::bytecode::Operand,
        miss: ir::Block) -> Option<Float64<ir::Value>> {
        Some(operand_number(stage.builder, self.registers, self.state, self.constants, operand, miss)
            .expect("a numeric operand is a register or an immediate"))
    }

    fn int32_operand(&mut self, stage: &mut Emit<'_, '_>, operand: quench_runtime::bytecode::Operand,
        miss: ir::Block) -> Option<Int32<ir::Value>> {
        operand_to_int32(stage.builder, self.registers, self.state, self.constants, operand, miss)
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
            Edges::ValueGuarded { miss } | Edges::Branch { miss, .. } => miss,
            _ => unreachable!("a numeric operation has a miss edge"),
        }
    }

    fn local_miss(&self) -> Option<ir::Block> {
        matches!(self.edges, Edges::ValueGuarded { .. }).then(|| self.miss())
    }

    fn html_dda_possible(&self) -> bool {
        self.html_dda
    }

    fn stable_prototype(&mut self, stage: &mut Emit<'_, '_>, _constructor: Register) -> Option<ir::Value> {
        self.prototype.map(|link| stage.raw_const(Lane::I64, i64::from(link)))
    }

    fn int_fact(&self) -> RegisterFact {
        RegisterFact::definition(Type::INT32, Representation::Int32)
    }

    fn number_fact(&self) -> RegisterFact {
        RegisterFact::definition(Type::NUMBER, Representation::Float64)
    }
}
