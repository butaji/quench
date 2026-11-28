use super::*;

impl<'a, 'f> Lowering<'a, 'f> {
    pub(super) fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        frame::register_tagged(&mut self.builder, &mut self.registers, state, register)
    }

    pub(super) fn define(&mut self, state: &mut State, register: Register, value: ir::Value, fact: RegisterFact) {
        frame::register_define(&mut self.builder, &mut self.registers, &mut self.written, state, register, value, fact);
    }

    /// Whether `pc` is the entry function's own record: an inlined callee's records read that
    /// callee's activation, which no window of the entry's holds.
    pub(super) fn caller_record(&self, pc: usize) -> bool {
        matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))
    }

    pub(super) fn set_tagged(&mut self, state: &mut State, register: Register, bits: ir::Value) {
        self.define(state, register, bits, RegisterFact::narrowest(Type::ANY));
    }

    /// The tagged Boolean a predicate `holds` selects.
    pub(super) fn boolean(&mut self, holds: ir::Value) -> ir::Value {
        boolean_word(&mut self.builder, holds)
    }

    pub(super) fn set_boolean(&mut self, state: &mut State, register: Register, boolean: ir::Value) {
        self.define(state, register, boolean, RegisterFact::narrowest(Type::language(quench_runtime::ValueType::Boolean)));
    }

    pub(super) fn set_int(&mut self, state: &mut State, register: Register, int: ir::Value) {
        self.define(state, register, int, RegisterFact::narrowest(Type::INT32));
    }

    pub(super) fn set_number(&mut self, state: &mut State, register: Register, number: ir::Value) {
        self.define(state, register, number, RegisterFact::narrowest(Type::NUMBER));
    }

    /// Prove an exact int32 view once; a Number already in a float register
    /// needs no canonical encoding or tag test to establish that projection.
    pub(super) fn int(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        frame::register_int_value(&mut self.builder, &mut self.registers, state, register, otherwise).word()
    }

    /// Prove Number only for a new untrusted value; later uses read its unboxed view.
    pub(super) fn number(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        frame::register_number_value(&mut self.builder, &mut self.registers, state, register, otherwise).word()
    }

    pub(super) fn canonicalize(&mut self, state: &mut State, pc: usize) {
        for register in 0..state.values.len() {
            // A kept receiver's register holds no object until it is materialized (`virtual_objects`).
            if self.is_kept(state, register as Register) { continue; }
            if self.is_live(pc, register as Register) && !state.values[register].tagged.current() {
                self.tagged(state, register as Register);
            }
        }
    }

    pub(super) fn operand_number(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_number(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise).map(quench_runtime::staged::Float64::word)
    }

    /// An operand's int32 value, statically where it is a constant int32.
    pub(super) fn operand_int(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_int(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise).map(quench_runtime::staged::Int32::word)
    }

    /// An operand's tagged bits: its register's, or its immediate constant's.
    pub(super) fn operand_tagged(&mut self, state: &mut State, operand: Operand) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.tagged(state, operand.register_index()?)),
            OperandKind::Constant => {
                let (value, _) = self.constant_value(usize::from(operand.payload()))?;
                Some(self.identity_constant(value))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    pub(super) fn operand_supported(&self, operand: Operand) -> bool {
        match operand.kind() {
            Some(OperandKind::Register) => operand.register_index().is_some_and(|r| r < self.unit.registers),
            Some(OperandKind::Constant) => self
                .program
                .constants
                .get(usize::from(operand.payload()))
                .and_then(super::facts::immediate)
                .is_some(),
            _ => false,
        }
    }
}
