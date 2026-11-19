//! The decode of an instruction into its staged `Operation` (`Operation::decode`): each catalog decoder
//! (`Op::staged`) builds its family's operation from the record's fields. Absent from the stencil build,
//! which has no catalog: a stencil row builds its operation from its holes.

use super::*;

/// Decoding reads instructions and the opcode catalog, which the stencil build (`quench-bjit`) does
/// not have: a stencil row builds its operation from its holes instead.
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes and the
    /// tables of `context` its operands index, by the decoder the catalog names for its opcode
    /// (`Op::staged`). `None` where the opcode has no staged operation, or where its constant or
    /// method site is not available to this tier: the caller then takes its own exact path. The
    /// heap operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    pub(crate) fn decode(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        match instruction.op().staged()? {
            Decoder::Flow => Self::decode_flow(instruction, context),
            Decoder::Local => Self::decode_local(instruction),
            Decoder::Generic => Self::decode_generic(instruction),
            Decoder::Value => Self::decode_value(instruction),
            Decoder::Property => Self::decode_property(instruction),
            Decoder::Element => Self::decode_element(instruction),
            Decoder::Call => Self::decode_call(instruction, context),
            Decoder::Names => names::decode(instruction),
            Decoder::Proved => None,
        }
    }

    /// The Wasm i32 jumps of `decode` (`Decoder::WasmJump`), from any record's wide fields
    /// (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_wasm_jump(instruction: WideInstruction) -> Option<Self> {
        WasmJump::decode(instruction).map(Self::WasmJump).or_else(|| BranchTable::decode(instruction).map(Self::BranchTable))
    }

    /// The staged operation `instruction` makes apart from how its result completes the frame
    /// (its result flags: a return, a `this` write), and those flags. A tier that completes a
    /// record's result itself asks this instead of naming the opcode; `decode` alone stays the
    /// operation with no completion.
    pub(crate) fn decode_completed(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Self, crate::bytecode::Register)> {
        use crate::bytecode::{RETURN_REGISTER, SET_THIS_REGISTER};
        // A numeric local target is where the result goes, not how it completes: it stays.
        // (It shares its bit with the `this` write: only `ReturnableAndThis` reads that bit as one.)
        let layout = instruction.op().result_layout();
        let completing = (layout.allowed_flags() & RETURN_REGISTER)
            | if matches!(layout, crate::bytecode::ResultLayout::ReturnableAndThis) { SET_THIS_REGISTER } else { 0 };
        let flags = instruction.result_flags() & completing;
        if instruction.result_flags() & !layout.allowed_flags() != 0 { return None; }
        let plain = if flags != 0 { instruction.without_result_flags() } else { instruction };
        Some((Self::decode(plain, context)?, flags))
    }

    /// The destination and constant index of an instruction `decode_flow` reads as `LoadConst`,
    /// for a tier that holds its constants in a table of its own (the b-jit's Numbers): the
    /// same decode, with the constant left for the tier to look up.
    #[inline(always)]
    pub(crate) fn decode_constant_load(instruction: WideInstruction) -> Option<(Register, usize)> {
        (instruction.op() == Op::LoadConst).then(|| (instruction.result_register(), instruction.constant_index()))
    }

    /// The environment, global and iteration operations of `decode` (`Decoder::Names`), from a
    /// record's wide fields (`Fields::wide`).
    #[inline(always)]
    pub(crate) fn decode_names(instruction: WideInstruction) -> Option<Self> {
        names::decode(instruction)
    }

    /// The register moves and control transfers of `decode`, from any record a tier holds
    /// (`Fields`): the one flow decoder, the bytecode's and the lane's alike.
    #[inline(always)]
    pub(crate) fn decode_flow<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        Some(match instruction.op() {
            Op::Await | Op::Yield | Op::YieldStar => return Self::decode_suspend(instruction.wide()),
            Op::Move => Self::Move {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::LoadConst => Self::LoadConst {
                destination: instruction.result_register(),
                constant: instruction.constant(context)? },
            Op::Nop => Self::Nop,
            Op::Throw => Self::Throw { source: instruction.register_a() },
            Op::Jump => Self::Jump,
            Op::JumpFalse => Self::JumpIf {
                register: instruction.register_a(), taken: false },
            Op::JumpTrue => Self::JumpIf {
                register: instruction.register_a(), taken: true },
            _ => Self::Return { source: instruction.register_a() },
        })
    }

    /// The call operations of `decode`: a call or construction through its forms (`Invocation`),
    /// except `super(...)`, whose arguments the VM gathers (`spread`).
    fn decode_call(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        if instruction.op() == Op::Construct && instruction.is_super_construct() {
            return spread::decode(instruction);
        }
        Some(Self::Call(CallOperation::Site(Invocation::of(instruction, context)?)))
    }

    /// The property operations of `decode`: a plain named read, a named store and `delete`.
    #[inline(always)]
    pub(crate) fn decode_property(instruction: WideInstruction) -> Option<Self> {
        match instruction.op() {
            Op::GetField => Self::decode_get_field(instruction),
            Op::Delete => Some(Self::Delete { destination: instruction.result_register(), base: instruction.register_b(),
                key: instruction.register_c(), strict: instruction.boolean_flag()? }),
            op if op.is_field_store() => Some(Self::SetField { value: instruction.register_a(),
                base: instruction.field_store_register(), atom: instruction.atom_index(),
                site: instruction.cache_site_index(), strict: op.is_strict_field_store() }),
            _ => None,
        }
    }

    /// A named read that completes no frame and writes no `this`; a nested lookup's base is the
    /// tier's own (`FieldBase::NESTED`).
    #[inline(always)]
    fn decode_get_field(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        let lookup = instruction.field_lookup()?;
        let base = match lookup {
            FieldLookup::Atom { base, .. } => base,
            FieldLookup::Site(_) => FieldBase(FieldBase::NESTED),
        };
        Some(Self::GetField { base, lookup })
    }

    /// The value-level operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_value(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadThis => Self::LoadThis {
                destination: instruction.result_register() },
            Op::RequireObjectCoercible => Self::RequireObjectCoercible {
                register: instruction.register_b() },
            Op::RequireIteratorResult => Self::RequireIteratorResult {
                register: instruction.register_b() },
            Op::ToNumeric => Self::Number(NumberOperation::ToNumeric {
                destination: instruction.result_register(), source: instruction.register_b() }),
            Op::IncDec => match instruction.numeric_local_target() {
                None => Self::Number(NumberOperation::IncDec {
                    destination: instruction.result_register(), source: instruction.register_b(),
                    decrement: instruction.boolean_flag()?, overflow: Overflow::Exit }),
                Some(local) => Self::IncDecLocal { local, source: instruction.register_b(),
                    decrement: instruction.boolean_flag()? },
            },
            Op::LoadCapture if instruction.result_flags() == 0 => Self::LoadCapture {
                destination: instruction.result_register(), depth: instruction.capture_depth(), slot: instruction.capture_slot(),
                root: false },
            Op::StoreCapture => Self::StoreCapture {
                source: instruction.register_a(), depth: instruction.capture_depth(), slot: instruction.capture_slot() },
            Op::Unary => return Self::decode_unary(instruction),
            _ => return None,
        })
    }

    /// The unary operators of `decode_value`.
    #[inline(always)]
    fn decode_unary(instruction: WideInstruction) -> Option<Self> {
        let (destination, source) = (instruction.result_register(), instruction.register_b());
        let operator = instruction.unary_operator();
        Some(match operator {
            _ if operator == oxc_ast::ast::UnaryOperator::LogicalNot as u32 => Self::LogicalNot { destination, source },
            _ if operator == crate::bytecode::UNARY_TYPEOF_INDEX => Self::TypeofIndex { destination, source },
            _ if operator == oxc_ast::ast::UnaryOperator::BitwiseNot as u32 =>
                Self::Number(NumberOperation::BitwiseNot { destination, source: Operand::register(source) }),
            _ if operator == oxc_ast::ast::UnaryOperator::Void as u32 && !instruction.returns_from_frame() =>
                Self::Void { destination },
            _ => return None,
        })
    }

    /// The local-slot operations of `decode`.
    #[inline(always)]
    pub(crate) fn decode_local(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: u16::try_from(instruction.local_slot()).ok()? },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
                result: instruction.optional_register_b() },
            Op::LoadLocal => Self::LoadLocalChecked {
                destination: instruction.result_register(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
                numeric: instruction.writes_numeric_local(),
            },
            Op::StoreLocal => Self::StoreLocalChecked {
                source: instruction.register_a(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
                result: instruction.optional_register_b(),
                initializing: instruction.boolean_field(crate::bytecode::InstructionField::C) == Some(true),
            },
            Op::InitializeTdz => Self::InitializeLocal { slot: u16::try_from(instruction.local_slot()).ok()? },
            _ => return None,
        })
    }
}
