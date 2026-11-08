//! The lane specializer's register frame: `staged` register operations over the SSA homes.

use super::*;

/// The lane specializer's register frame (`crate::staged::Frame`): the SSA homes and the facts
/// `State` keeps, with the stage's builder emitting each operation's IR.
pub(super) struct EmitFrame<'a> {
    pub(super) registers: &'a mut Registers,
    pub(super) written: &'a mut CompoundBitSet,
    pub(super) state: &'a mut State,
}

impl crate::staged::Frame<Emit<'_, '_>> for EmitFrame<'_> {
    type Fact = RegisterFact;

    #[inline(always)]
    fn fact(&self, register: Register) -> RegisterFact {
        self.state.values[usize::from(register)]
    }

    #[inline(always)]
    fn read(&mut self, stage: &mut Emit<'_, '_>, register: Register) -> ir::Value {
        register_read(stage.builder, self.registers, self.state, register)
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
