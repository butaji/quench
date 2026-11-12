//! Named classes of opcodes and control-flow layouts, each decided once and asked by every
//! analysis that needs it.

use super::{ControlFlowLayout, FieldLayout, Op};

impl Op {
    /// A leaf record that only moves or computes an immediate value.
    pub(crate) const fn is_leaf_value(self) -> bool {
        self.traits().has(crate::bytecode::Traits::LEAF_VALUE)
    }

    /// A branch a leaf may hold.
    pub(crate) const fn is_leaf_jump(self) -> bool {
        self.traits().has(crate::bytecode::Traits::LEAF_JUMP)
    }

    /// A binary record a leaf admits with register or constant operands.
    pub(crate) const fn is_leaf_binary(self) -> bool {
        self.traits().has(crate::bytecode::Traits::LEAF_BINARY)
    }

    /// A by-name operation, the set the frame-entry diagnostics list.
    #[cfg_attr(not(feature = "profile-lane-exits"), allow(dead_code))]
    pub(crate) const fn is_name_operation(self) -> bool {
        self.traits().has(crate::bytecode::Traits::NAME_OPERATION)
    }

    /// A record that makes a closure.
    #[cfg_attr(not(feature = "profile-lane-exits"), allow(dead_code))]
    pub(crate) const fn makes_closure(self) -> bool {
        self.traits().has(crate::bytecode::Traits::MAKES_CLOSURE)
    }

    /// A JavaScript branch: the unconditional jump or one of its conditional tests.
    pub(crate) const fn is_js_jump(self) -> bool {
        matches!(self, Op::Jump) || self.is_js_conditional_jump()
    }

    /// A JavaScript conditional branch on a value or a binary comparison.
    pub(crate) const fn is_js_conditional_jump(self) -> bool {
        matches!(self, Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse)
    }

    /// A copy of register B into the result register, and nothing else.
    pub(crate) const fn copies_register(self) -> bool {
        matches!(self, Op::Move)
    }

    /// A general step a b-jit body's helper makes in place (`lane_body_step`): one that only continues,
    /// jumps or throws, on the frame's own registers, bindings and environment, with no call, return,
    /// suspension or `this` binding. The lane's own in-place steps (`LANE_IN_PLACE`) are among them;
    /// the rest may replace the frame's environment, which the lane's own dispatch caches, so only a
    /// body, whose helper drops those caches after the step, makes them: the scope, operator,
    /// constant, name, literal, element, field and class steps.
    pub(crate) const fn steps_in_body(self) -> bool {
        self.traits().has(crate::bytecode::Traits::LANE_IN_PLACE) || matches!(
            self,
            Op::CloneEnv | Op::InitializeTdz | Op::LoadLocal | Op::StoreLocal | Op::LoadEnvLocal | Op::StoreEnvLocal | Op::StoreVarBinding
                | Op::LoadConst | Op::Unary | Op::Binary | Op::JumpBinaryFalse | Op::Throw
                | Op::LoadName | Op::LoadNameTypeof | Op::ResolveName | Op::LoadResolvedName | Op::DeleteName | Op::StoreName | Op::StoreResolvedName
                | Op::LoadCallee | Op::LoadImportMeta | Op::LoadCapture | Op::StoreCapture
                | Op::MakeClosure | Op::MakeArray | Op::MakeConstArray | Op::MakeObject | Op::MakeObject2 | Op::MakeObjectLiteral
                | Op::MakeLiteralTemplate | Op::CreateRegExpLiteral | Op::CacheTemplateObject | Op::LoadCachedTemplateObject
                | Op::StringCaseIndex | Op::StringConcat | Op::Delete | Op::GetAsyncIterator
                | Op::GetField | Op::GetIndex | Op::SetIndex | Op::SetField | Op::SetThisField | Op::SetFieldStrict | Op::SetThisFieldStrict
                | Op::ValidateClassHeritage | Op::SuperCallCheck
        )
    }

    /// An op that always completes abruptly, raising its operand: no row follows it, and a handler
    /// of its function may take it.
    pub(crate) const fn raises(self) -> bool {
        self.traits().has(crate::bytecode::Traits::RAISES)
    }

    /// A suspension point: an `await`, or a `yield` of a generator.
    pub(crate) const fn suspends(self) -> bool {
        matches!(self, Op::Await | Op::Yield | Op::YieldStar)
    }

    /// A binary operation whose arithmetic `binary_operator` names.
    #[allow(dead_code)]
    pub(crate) const fn is_binary_arithmetic(self) -> bool {
        matches!(self, Op::Binary | Op::NumericAdd | Op::NumericMultiply)
    }

    /// An access that reads its object register as an indexed receiver.
    pub(crate) const fn is_indexed_access(self) -> bool {
        matches!(self, Op::GetIndex | Op::SetIndex | Op::RequireObjectCoercible)
    }

    /// A store to an indexed element, which a compiled loop lowers in place or leaves.
    pub(crate) const fn stores_element(self) -> bool {
        matches!(self, Op::SetIndex)
    }

    /// An instruction that names a frame or environment local slot.
    pub(crate) const fn addresses_local_slot(self) -> bool {
        self.loads_frame_local() || self.addresses_env_local()
    }

    /// A store to a binding by name.
    pub(crate) const fn stores_name(self) -> bool {
        matches!(self, Op::StoreName)
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
