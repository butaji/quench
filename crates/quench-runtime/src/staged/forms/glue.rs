//! Runtime glue of `forms`: the decode of a call's form from its record (`Invocation::of`, `CallRecord::of`)
//! and the plain call row constant.
#![allow(unused_imports)]
use super::*;

/// The operands of a call instruction, read from its record and the tables of its `DecodeContext`.
pub trait InvocationFormsExt: Sized {
    fn of(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self>;

    fn operands(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Callee, Receiver, Arguments)>;

    fn call(instruction: WideInstruction) -> Option<(Callee, Receiver, Arguments)>;

    fn known(instruction: WideInstruction) -> (Callee, Receiver, Arguments);

    fn construction(instruction: WideInstruction) -> Option<(Callee, Receiver, Arguments)>;

    fn method(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Callee, Receiver, Arguments)>;
}

impl InvocationFormsExt for Invocation {
    #[cfg(not(stencil_build))]
    /// The operands of `instruction`, read with the tables of `context`; `None` for any form no
    /// row runs: an eval, which the host decides, a super construction, and an array of arguments.
    #[inline(always)]
    fn of(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
        let (callee, receiver, arguments) = Self::operands(instruction, context)?;
        matches!(instruction.result_flags(), 0 | RETURN_REGISTER).then(|| Self { callee, receiver, arguments,
            result: instruction.result_register(), returns: instruction.returns_from_frame(),
            forwards: instruction.op() == Op::CallForwardingArguments })
    }

    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn operands(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Callee, Receiver, Arguments)> {
        match instruction.op() {
            Op::Call | Op::CallForwardingArguments => Self::call(instruction),
            Op::CallKnown => Some(Self::known(instruction)),
            Op::CallMethod | Op::CallThisMethod => Self::method(instruction, context),
            Op::Construct => Self::construction(instruction),
            _ => None,
        }
    }

    #[cfg(not(stencil_build))]
    /// A call through registers: the callee, receiver and a window of arguments; `None` for an
    /// eval, which the host decides.
    #[inline(always)]
    fn call(instruction: WideInstruction) -> Option<(Callee, Receiver, Arguments)> {
        let imm = instruction.imm();
        (!instruction.direct_eval() && !instruction.parameter_eval()).then(|| (Callee::Register(instruction.register_b()),
            Receiver::Register(instruction.register_c()),
            Arguments::Window { first: ImmediateLayout::call_window_base(imm), count: ImmediateLayout::argument_count(imm) }))
    }

    #[cfg(not(stencil_build))]
    /// A call of a function the record names: no receiver, the window the record names.
    fn known(instruction: WideInstruction) -> (Callee, Receiver, Arguments) {
        let window = instruction.call_window();
        (Callee::Known(instruction.known_function_index()), Receiver::Undefined, Arguments::Window { first: window.base, count: window.count })
    }

    #[cfg(not(stencil_build))]
    /// A construction with its arguments in registers; `None` for `super(...)` and an array of arguments.
    fn construction(instruction: WideInstruction) -> Option<(Callee, Receiver, Arguments)> {
        match instruction.construct_arguments() {
            ConstructArguments::Registers(window) if !instruction.is_super_construct() => Some((Callee::Register(instruction.register_b()),
                Receiver::Allocated, Arguments::Window { first: window.base, count: window.count })),
            _ => None,
        }
    }

    #[cfg(not(stencil_build))]
    /// A method call: the receiver and the method atom come from the method site, the arguments
    /// from its call record.
    fn method(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<(Callee, Receiver, Arguments)> {
        let index = instruction.method_site_index();
        let site = context.method_site(index)?;
        let receiver = match instruction.op() {
            Op::CallMethod => Receiver::Register(instruction.register_b()),
            _ => Receiver::This { path: site.receiver_path.get() },
        };
        Some((Callee::Method { atom: site.atom, site: u32::try_from(index).ok()? }, receiver,
            Arguments::List(RegisterList { start: site.argument_start, count: site.argument_count })))
    }
}

/// The row of a plain call: a call through a callee register, a receiver register and a window
/// of arguments. `CallRecord::of` reads its record as `CallRecord::Plain` (the call arm), and
/// `Invocation::operands` reads the same record's callee and receiver.
#[cfg(not(stencil_build))]
pub const PLAIN_CALL_ROW: u16 = Op::Call as u16;

/// The record of a call-family instruction (`CallRecord::of`).
pub trait CallRecordFormsExt: Sized {
    fn of(instruction: WideInstruction) -> Option<Self>;
}

impl CallRecordFormsExt for CallRecord {
    /// The record of the call-family instruction `instruction`, whatever its result flags; `None` for a
    /// record of no call family.
    fn of(instruction: WideInstruction) -> Option<Self> {
        match instruction.op() {
            Op::Construct if instruction.is_super_construct() => Some(Self::Super),
            op => Self::of_op(op),
        }
    }
}

/// `const` queries of a call-family record: the opcode runners (`vm::dispatch::runner_of`) decide by them in
/// const context, so they are inherent (a trait method cannot be `const`).
impl CallRecord {
    /// The record of the call-family opcode `op`, a construction's plain record (`of` reads the super
    /// flag); `None` for an opcode of no call family. `const`, so the opcode runners decide by it.
    pub const fn of_op(op: Op) -> Option<Self> {
        Some(match op {
            Op::Call => Self::Plain,
            Op::CallForwardingArguments => Self::Forwarding,
            Op::CallKnown => Self::Known,
            Op::CallMethod => Self::Method,
            Op::CallThisMethod => Self::ThisMethod,
            Op::Construct => Self::Construct,
            Op::CallArray => Self::Array,
            Op::CallSpread => Self::Spread,
            Op::CallRest => Self::Rest,
            Op::CallFirstChunk => Self::FirstChunk,
            _ => return None,
        })
    }

    /// Whether the VM runs the record's general form (`vm::dispatch`'s call runner, which the staged
    /// row declines to). A rest view's call has none: its staged rows answer it alone.
    pub const fn has_general_form(self) -> bool {
        !matches!(self, Self::Rest)
    }
}
