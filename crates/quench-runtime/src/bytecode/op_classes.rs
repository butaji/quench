//! Named classes of opcodes and control-flow layouts, each decided once and asked by every
//! analysis that needs it.

use super::{ControlFlowLayout, Decoder, FieldLayout, GenericStep, Op, Traits};

impl Op {
    /// A leaf record that only moves or computes an immediate value.
    pub(crate) const fn is_leaf_value(self) -> bool {
        self.traits().has(Traits::LEAF_VALUE)
    }

    /// A branch a leaf may hold.
    pub(crate) const fn is_leaf_jump(self) -> bool {
        self.traits().has(Traits::LEAF_JUMP)
    }

    /// A binary record a leaf admits with register or constant operands.
    pub(crate) const fn is_leaf_binary(self) -> bool {
        self.traits().has(Traits::LEAF_BINARY)
    }

    /// A by-name operation, the set the frame-entry diagnostics list.
    #[cfg_attr(not(feature = "profile-lane-exits"), allow(dead_code))]
    pub(crate) const fn is_name_operation(self) -> bool {
        self.traits().has(Traits::NAME_OPERATION)
    }

    /// A record that makes a closure.
    #[cfg_attr(not(feature = "profile-lane-exits"), allow(dead_code))]
    pub(crate) const fn makes_closure(self) -> bool {
        self.traits().has(Traits::MAKES_CLOSURE)
    }

    /// A JavaScript conditional branch on a value or a binary comparison.
    pub(crate) const fn is_js_conditional_jump(self) -> bool {
        self.is_truthiness_jump() || self.compares_and_jumps()
    }

    /// A branch on a binary comparison of two operands.
    pub(crate) const fn compares_and_jumps(self) -> bool {
        self.traits().has(Traits::FUSED_BINARY_JUMP)
    }

    /// A copy of register B into the result register, and nothing else.
    pub(crate) const fn copies_register(self) -> bool {
        self.traits().has(Traits::REGISTER_COPY)
    }

    /// A general step a b-jit body's helper makes in place (`body_general_step`): one that only continues,
    /// jumps or throws, on the frame's own registers, bindings and environment, with no call, return,
    /// suspension or `this` binding (the catalog's `GenericStep`). The lane's own in-place steps
    /// (`steps_in_lane`) are among them; the rest may replace the frame's environment, which the lane's
    /// own dispatch caches, so only a body, whose helper drops those caches after the step, makes them.
    pub(crate) const fn steps_in_body(self) -> bool {
        self.generic_step().is_some()
    }

    /// A general step the lane makes in place: it reads and writes only the frame, its registers and
    /// heap values, runs guest code only through nested calls, and continues or throws.
    pub(crate) const fn steps_in_lane(self) -> bool {
        matches!(self.generic_step(), Some(GenericStep::Lane | GenericStep::LaneResult))
    }

    /// A lane step that is a call-out row (`Operation::call_out`) whose result the lane writes back to
    /// the frame (`Vm::call_out`).
    pub(crate) const fn steps_with_result(self) -> bool {
        matches!(self.generic_step(), Some(GenericStep::LaneResult))
    }

    /// A step that reads and writes only the variables of the activation that runs it (its locals,
    /// captures and names) and the registers it addresses, so a frameless activation makes it over
    /// its window (`Vm::owner_step`) as a frame does: the local, capture and name loads and stores.
    pub(crate) const fn steps_on_owner(self) -> bool {
        self.owned_step().is_some()
    }

    /// An op whose step binds the frame's `this` (`super()`).
    pub(crate) const fn binds_this(self) -> bool {
        self.traits().has(Traits::BINDS_THIS)
    }

    /// An op an inlined callee's unit holds by its own rewrite or an exit rather than by renaming.
    pub(crate) const fn is_unit_rewritten(self) -> bool {
        self.traits().has(Traits::UNIT_REWRITTEN)
    }

    /// An op whose result may be an arbitrary boxed value (a capture, field, element or global name read).
    pub(crate) const fn boxes_result(self) -> bool {
        self.traits().has(Traits::BOXED_RESULT)
    }

    /// An op that always completes abruptly, raising its operand: no row follows it, and a handler
    /// of its function may take it.
    pub(crate) const fn raises(self) -> bool {
        self.traits().has(Traits::RAISES)
    }

    /// A call form that activates a script function: the plain call, the method, known, array, spread and
    /// forwarding calls, and the construction (the catalog's `Call` control flow).
    pub(crate) const fn makes_script_call(self) -> bool {
        matches!(self.control_flow_layout(), ControlFlowLayout::Call)
    }

    /// A suspension point: an `await`, or a `yield` (or `yield*`) of a generator.
    pub(crate) const fn suspends(self) -> bool {
        self.traits().has(Traits::SUSPENSION_POINT)
    }

    /// A binary operation whose arithmetic `binary_operator` names.
    #[allow(dead_code)]
    pub(crate) const fn is_binary_arithmetic(self) -> bool {
        self.traits().has(crate::bytecode::Traits::ARITHMETIC_BINARY)
    }

    /// An access that reads its object register as an indexed receiver.
    pub(crate) const fn is_indexed_access(self) -> bool {
        self.traits().has(Traits::INDEXED_RECEIVER)
    }

    /// A store to an indexed element, which a compiled loop lowers in place or leaves.
    pub(crate) const fn stores_element(self) -> bool {
        self.traits().has(Traits::ELEMENT_STORE)
    }

    /// An instruction that names a frame or environment local slot.
    pub(crate) const fn addresses_local_slot(self) -> bool {
        self.loads_frame_local() || self.addresses_env_local()
    }

    /// A store to a binding by name.
    pub(crate) const fn stores_name(self) -> bool {
        self.traits().has(Traits::NAME_STORE)
    }

    /// A load of a binding by name.
    pub(crate) const fn loads_name(self) -> bool {
        self.traits().has(Traits::NAME_LOAD)
    }

    /// A load by name that throws for an unresolvable binding (every one but `typeof`'s).
    pub(crate) const fn is_throwing_name_load(self) -> bool {
        matches!(self.staged(), Some(Decoder::Proved))
    }

    /// A read of the activation's passed arguments.
    pub(crate) const fn reads_arguments(self) -> bool {
        self.traits().has(Traits::ARGUMENTS_READ)
    }

    /// A call through a callee register, with a window or a spread array of arguments.
    pub(crate) const fn is_ordinary_call(self) -> bool {
        self.traits().has(Traits::ORDINARY_CALL)
    }

    /// An instruction that captures the current environment: a closure or an environment clone.
    pub(crate) const fn captures_environment(self) -> bool {
        self.traits().has(Traits::ENVIRONMENT_CAPTURE)
    }

    /// A conditional branch on a register's truthiness.
    pub(crate) const fn is_truthiness_jump(self) -> bool {
        self.traits().has(Traits::JUMPS_ON_FALSE) || self.traits().has(Traits::JUMPS_ON_TRUE)
    }

    /// A load or store of a captured slot.
    pub(crate) const fn addresses_capture(self) -> bool {
        self.traits().has(Traits::CAPTURE_ACCESS)
    }

    /// A load or store of an environment-resident local.
    pub(crate) const fn addresses_env_local(self) -> bool {
        self.traits().has(Traits::ENV_LOCAL_ACCESS)
    }

    /// A load of a frame-resident local.
    pub(crate) const fn loads_frame_local(self) -> bool {
        self.traits().has(Traits::FRAME_LOCAL_LOAD)
    }

    /// A frame local access that capture layout can move into the environment.
    pub(crate) const fn is_relocatable_local_access(self) -> bool {
        self.traits().has(Traits::RELOCATABLE_LOCAL)
    }
}

impl ControlFlowLayout {
    /// Whether execution may continue at the next instruction.
    pub(crate) const fn falls_through(self) -> bool {
        !matches!(self, ControlFlowLayout::Jump | ControlFlowLayout::Terminal | ControlFlowLayout::BranchTable)
    }

    /// Whether the instruction may continue at its jump target.
    pub(crate) const fn jumps(self) -> bool {
        matches!(self, ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump)
    }

    /// Whether execution continues at the next instruction and nowhere else.
    pub(crate) const fn only_falls_through(self) -> bool {
        matches!(self, ControlFlowLayout::Fallthrough | ControlFlowLayout::Call)
    }
}

impl FieldLayout {
    /// Whether the field holds a tagged operand (a register, local, constant or field).
    pub(crate) const fn holds_operand(self) -> bool {
        matches!(self, FieldLayout::Operand | FieldLayout::NumericIndexOperand)
    }
}

#[cfg(test)]
mod role_tests {
    use super::super::{Op, Traits};

    /// The ops a predicate selects over the whole catalog.
    fn selected(predicate: impl Fn(Op) -> bool) -> Vec<Op> {
        (0..Op::COUNT).filter_map(Op::from_index).filter(|op| predicate(*op)).collect()
    }

    fn assert_selects(predicate: impl Fn(Op) -> bool, expected: &[Op]) {
        let found = selected(predicate);
        assert_eq!(found.len(), expected.len(), "selected {found:?}, expected {expected:?}");
        assert!(expected.iter().all(|op| found.contains(op)), "selected {found:?}, expected {expected:?}");
    }

    #[test]
    fn op_class_roles_select_the_ops_they_replaced() {
        assert_selects(Op::compares_and_jumps, &[Op::JumpBinaryFalse]);
        assert_selects(Op::copies_register, &[Op::Move]);
        assert_selects(Op::suspends, &[Op::Await, Op::Yield, Op::YieldStar]);
        assert_selects(Op::is_binary_arithmetic, &[Op::Binary, Op::NumericAdd, Op::NumericMultiply]);
        assert_selects(Op::is_indexed_access, &[Op::GetIndex, Op::SetIndex, Op::RequireObjectCoercible]);
        assert_selects(Op::stores_element, &[Op::SetIndex]);
        assert_selects(Op::stores_name, &[Op::StoreName]);
        assert_selects(Op::loads_name, &[Op::LoadName, Op::LoadNameCall, Op::LoadNameTypeof]);
        assert_selects(Op::is_throwing_name_load, &[Op::LoadName, Op::LoadNameCall]);
        assert_selects(Op::reads_arguments, &[Op::ArgumentsLength, Op::ArgumentGet]);
        assert_selects(Op::is_ordinary_call, &[Op::Call, Op::CallArray]);
        assert_selects(Op::captures_environment, &[Op::MakeClosure, Op::CloneEnv]);
        assert_selects(Op::is_truthiness_jump, &[Op::JumpFalse, Op::JumpTrue]);
        assert_selects(Op::addresses_capture, &[Op::LoadCapture, Op::StoreCapture]);
        assert_selects(Op::addresses_env_local, &[Op::LoadEnvLocal, Op::StoreEnvLocal]);
        assert_selects(Op::loads_frame_local, &[Op::LoadLocal, Op::LoadLocalPlain]);
        assert_selects(Op::is_relocatable_local_access, &[Op::LoadLocal, Op::StoreLocal]);
        assert_selects(|op| op.traits().has(Traits::PLAIN_CALL), &[Op::Call]);
    }
}
