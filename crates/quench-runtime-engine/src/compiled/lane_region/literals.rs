//! Heap values a lane body names: string and BigInt constants, and the operators that take them.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

impl Lowering<'_, '_> {
    /// A constant's value and type: an immediate, or in online code the string or BigInt the
    /// VM made of it (`OnlineFacts::constants`).
    pub(crate) fn constant_value(&self, index: usize) -> Option<(Value, Type)> {
        let constant = self.program.constants.get(index)?;
        let value = super::super::facts::immediate(constant).or_else(|| self.vm.online.as_ref()?.constants.get(index).copied())?;
        Some((value, super::super::facts::constant_type(constant)))
    }

    /// `LoadConst` of `constant` at `pc`: a string a frozen image holds, read from the image
    /// under its invalidation word, or the constant's value (`constant_value`).
    pub(crate) fn load_constant(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        constant: &quench_runtime::bytecode::Constant) -> Option<bool> {
        let index = instruction.constant_index();
        // An image names the string by its recipe; the loader allocates it once (`HeapValueRecipe::String`).
        if super::super::facts::immediate(constant).is_none()
            && let Some(key) = self.vm.ahead.and_then(|ahead| ahead.strings.get(&index).copied())
        {
            let value = self.reloc(RelocKind::HeapValue, key, Value::UNDEFINED.to_bits());
            self.define(state, instruction.result_register(), value,
                RegisterFact::narrowest(super::super::facts::constant_type(constant)));
            return Some(true);
        }
        if super::super::facts::immediate(constant).is_none()
            && matches!(constant, quench_runtime::bytecode::Constant::String(_))
            && let Some(base) = self.vm.frozen_constants
        {
            // The image materializes the string once at load; the read is checked
            // against the image's invalidation word like every other frozen fact.
            let exit = self.guard_exit(state, pc as u32);
            self.read_invalidation(state, exit);
            let address = self.reloc(RelocKind::ConstantPool, index as u32, (base + index * std::mem::size_of::<Value>()) as u64);
            let value = self.raw_load(Lane::I64, regions::constant(), address, 0);
            self.define(state, instruction.result_register(), value,
                RegisterFact::narrowest(super::super::facts::constant_type(constant)));
            return Some(true);
        }
        self.constant_value(index).map(|(value, ty)| {
            self.step(state, frame::Edges::Value, quench_runtime::staged::Operation::LoadConst {
                destination: instruction.result_register(), constant: quench_runtime::staged::Constant { value, ty } });
            true
        })
    }

    /// The result type of a binary operator a string or BigInt operand takes
    /// (`facts::heap_primitive_binary`), by its operands' facts here.
    pub(crate) fn heap_primitive_binary(&self, state: &State, instruction: WideInstruction) -> Option<Type> {
        let operand = |operand| super::super::facts::operand_type(&self.program.constants, operand, |register|
            state.values.get(usize::from(register)).map_or(Type::ANY, |fact| fact.value.ty));
        super::super::facts::heap_primitive_binary(instruction.binary_operator(), operand(instruction.operand_b()), operand(instruction.operand_c()))
            .filter(|_| super::super::facts::heap_operator(instruction))
    }

    /// A binary operator a string or BigInt operand takes, by the interpreter's definition
    /// (`compiled::binary_in_lane`); an object operand, a throw or a due collection leaves first.
    /// A `+` whose result is a string is the concatenation of its two operands (`concat_pair_in_lane`),
    /// which runs no generic operator.
    pub(crate) fn binary_by_definition(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, ty: Type) -> Option<bool> {
        let left = self.operand_tagged(state, instruction.operand_b())?;
        let right = self.operand_tagged(state, instruction.operand_c())?;
        let exit = self.guard_exit(state, pc as u32);
        let concatenation = instruction.binary_operator() == oxc_ast::ast::BinaryOperator::Addition as u32
            && ty.is_subtype(Type::language(quench_runtime::ValueType::String));
        let value = if concatenation {
            self.helper_call(LaneHelper::ConcatPair, &[self.payload, left, right])
        } else {
            let (payload, operator) = (self.payload, self.raw_const(Lane::I64, i64::from(instruction.binary_operator())));
            self.helper_call(LaneHelper::Binary, &[payload, operator, left, right])
        };
        let made = self.raw_compare_imm(Relation::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, made, exit);
        self.allocated(state);
        self.define(state, instruction.result_register(), value, RegisterFact::narrowest(ty));
        Some(true)
    }

    /// A numeric binary operator: by the interpreter's definition where a string or BigInt
    /// operand takes it, natively on Numbers otherwise (`Lowering::binary`).
    pub(crate) fn operator(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A frameless Number region has no VM helper capability (`LaneHelpers::Numeric`): a string operand leaves there.
        let heap = (!self.helpers.is_numeric()).then(|| self.heap_primitive_binary(state, instruction)).flatten();
        match heap.filter(|_| !instruction.returns_from_frame()) {
            Some(ty) => self.binary_by_definition(state, pc, instruction, ty),
            None => self.binary(state, pc, instruction),
        }
    }

    /// Whether `register` holds `ty` at an edge to `target`: proved, or guarded there as a
    /// string or BigInt (`guard_heap_primitive`).
    pub(crate) fn proves(&mut self, state: &mut State, register: Register, ty: Type, target: usize) -> bool {
        if state.values[usize::from(register)].value.ty.is_subtype(ty.machine()) { return true; }
        let exit = self.guard_exit(state, target as u32);
        self.guard_heap_primitive(state, register, ty, exit)
    }

    /// Guards a string or BigInt fact an edge does not prove (`Lowering::conform`) by the VM's
    /// own `typeof` test (`compiled::typeof_is_in_lane`); `false` for any other fact.
    pub(crate) fn guard_heap_primitive(&mut self, state: &mut State, register: Register, ty: Type, exit: ir::Block) -> bool {
        let Some(name) = [(quench_runtime::ValueType::String, "string"), (quench_runtime::ValueType::BigInt, "bigint")].into_iter()
            .find(|(kind, _)| ty != Type::NEVER && ty.is_subtype(Type::language(*kind))).map(|(_, name)| name) else { return false };
        let bits = self.tagged(state, register);
        let index = self.raw_const(Lane::I64, quench_runtime::bytecode::typeof_name_index(name).expect("a typeof name") as i64);
        let named = self.helper_call(LaneHelper::TypeofIs, &[bits, index]);
        let holds = self.raw_compare_imm(Relation::NotEqual, named, 0);
        emit_js_guard(&mut self.builder, holds, exit);
        self.define(state, register, bits, RegisterFact::narrowest(ty));
        true
    }
}

