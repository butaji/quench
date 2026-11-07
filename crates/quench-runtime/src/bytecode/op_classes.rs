//! Named classes of opcodes and control-flow layouts, each decided once and asked by every
//! analysis that needs it.

use super::{ControlFlowLayout, FieldLayout, Op};

impl Op {
    /// A JavaScript branch: the unconditional jump or one of its conditional tests.
    pub(crate) const fn is_js_jump(self) -> bool {
        matches!(self, Op::Jump) || self.is_js_conditional_jump()
    }

    /// A JavaScript conditional branch on a value or a binary comparison.
    pub(crate) const fn is_js_conditional_jump(self) -> bool {
        matches!(self, Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse)
    }

    /// A suspension point: an `await`, or a `yield` of a generator.
    pub(crate) const fn suspends(self) -> bool {
        matches!(self, Op::Await | Op::Yield | Op::YieldStar)
    }

    /// A binary operation whose arithmetic `binary_operator` names.
    pub(crate) const fn is_binary_arithmetic(self) -> bool {
        matches!(self, Op::Binary | Op::NumericAdd | Op::NumericMultiply)
    }

    /// An access that reads its object register as an indexed receiver.
    pub(crate) const fn is_indexed_access(self) -> bool {
        matches!(self, Op::GetIndex | Op::SetIndex | Op::RequireObjectCoercible)
    }

    /// An instruction that names a frame or environment local slot.
    pub(crate) const fn addresses_local_slot(self) -> bool {
        self.loads_frame_local() || self.addresses_env_local()
    }

    /// A load of a binding by name.
    pub(crate) const fn loads_name(self) -> bool {
        self.is_throwing_name_load() || matches!(self, Op::LoadNameTypeof)
    }

    /// A load by name that throws for an unresolvable binding (every one but `typeof`'s).
    pub(crate) const fn is_throwing_name_load(self) -> bool {
        matches!(self, Op::LoadName | Op::LoadNameCall)
    }

    /// A read of the activation's passed arguments.
    pub(crate) const fn reads_arguments(self) -> bool {
        matches!(self, Op::ArgumentsLength | Op::ArgumentGet)
    }

    /// A call through a callee register, with a window or a spread array of arguments.
    pub(crate) const fn is_ordinary_call(self) -> bool {
        matches!(self, Op::Call | Op::CallArray)
    }

    /// An instruction that captures the current environment: a closure or an environment clone.
    pub(crate) const fn captures_environment(self) -> bool {
        matches!(self, Op::MakeClosure | Op::CloneEnv)
    }

    /// A conditional branch on a register's truthiness.
    pub(crate) const fn is_truthiness_jump(self) -> bool {
        matches!(self, Op::JumpFalse | Op::JumpTrue)
    }

    /// A load or store of a captured slot.
    pub(crate) const fn addresses_capture(self) -> bool {
        matches!(self, Op::LoadCapture | Op::StoreCapture)
    }

    /// A load or store of an environment-resident local.
    pub(crate) const fn addresses_env_local(self) -> bool {
        matches!(self, Op::LoadEnvLocal | Op::StoreEnvLocal)
    }

    /// A load of a frame-resident local.
    pub(crate) const fn loads_frame_local(self) -> bool {
        matches!(self, Op::LoadLocal | Op::LoadLocalPlain)
    }

    /// A frame local access that capture layout can move into the environment.
    pub(crate) const fn is_relocatable_local_access(self) -> bool {
        matches!(self, Op::LoadLocal | Op::StoreLocal)
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
