//! The shape of a call (`CallShape`) and its form (`CallForm`), and the shape of a method call (`method_shape`).
//! Split from `call_plan.rs` for its size.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// How a body makes a call, by the instruction's operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallForm {
    /// `Call`, through its callee register and argument window: direct through the callee's entry
    /// cell where its site recorded a stable callee, else the call helper (`body_call_helper`).
    Plain,
    /// `Construct` over registers: the construct helper (`body_construct_helper`), which plans the
    /// receiver's fields or activates the constructor as the lane's own construction does.
    Construct,
    /// Every other form (method, this-method, array, spread and forwarding calls; a `super`
    /// or array-argument construction): the interpreter's own step made in place by the step helper,
    /// the call it makes running the callee as any call from a helper does. These sites record no
    /// callee to call by its entry cell, and a method call's callee is the method cache's lookup,
    /// which no register holds for a call row to read.
    Stepped,
    /// `CallMethod` with a window of consecutive argument registers (`method_shape`): a site read writes the
    /// method's value to a scratch register, which the checked direct row compares with the recorded callee.
    Method,
}

/// A method call's site read (`plan::method_read`): the cache entry that answers the method, and the scratch register
/// the read writes the callee to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Method {
    pub(crate) cache: u16,
    pub(crate) scratch: Register,
}

impl CallForm {
    /// The form of the call `instruction` is, if a body's call chain makes it: a call form with no
    /// result flag left but the `this` binding (the return flag is `cover`'s). `CallKnown` is the Wasm
    /// compiler's call, which no JavaScript function holds, so it has no form. A call that binds the caller's
    /// `this` (`SET_THIS_REGISTER`, a `super()` call) or has an `eval` flag (which declares bindings in the
    /// caller's scope) is the step helper's own step made in place, a helper call row whatever its form: the
    /// helper binds the frame's `this` and publishes the receiver, or runs the eval in the frame.
    pub(crate) fn of(instruction: WideInstruction) -> Option<Self> {
        let bound_this = if instruction.writes_current_this() { quench_runtime::bytecode::SET_THIS_REGISTER } else { 0 };
        let op = instruction.op();
        let calls = op.makes_script_call() && !crate::compiled::meaning_compiled::is_call_known(instruction) && instruction.result_flags() & !bound_this == 0;
        calls.then(|| Self::shaped(instruction))
    }

    /// The form of a script call `instruction` (`of`): the staged `Invocation` of it, a plain call through a callee register
    /// and a window of arguments, or a construction of the same operands. A record that binds the caller's `this` or resolves
    /// a dynamic scope (an eval) is the step helper's own step made in place, whatever its operands; every other form is stepped.
    fn shaped(instruction: WideInstruction) -> Self {
        use quench_runtime::staged::{Arguments, Callee, DecodeContext, Invocation, Receiver};
        let in_place = instruction.writes_current_this() || instruction.resolves_dynamic_scope();
        match Invocation::of(instruction, DecodeContext::EMPTY) {
            Some(Invocation { callee: Callee::Register(_), receiver: Receiver::Register(_), arguments: Arguments::Window { .. }, forwards: false, .. }) if !in_place => Self::Plain,
            Some(Invocation { receiver: Receiver::Allocated, arguments: Arguments::Window { .. }, .. }) if !in_place => Self::Construct,
            _ => Self::Stepped,
        }
    }

    /// The helper whose row makes the call.
    pub(crate) fn helper(self) -> LaneHelper {
        match self {
            Self::Plain => LaneHelper::Call,
            Self::Construct => LaneHelper::ConstructCall,
            Self::Stepped | Self::Method => LaneHelper::Step,
        }
    }
}

/// What a call instruction names, decoded once: the record the helper takes (`call_pc`), the
/// register that receives the result and the form. A plain call also names the callee and receiver
/// registers, the first argument register and the argument count (zero for the other forms, whose
/// helpers read their own operands from the record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallShape {
    pub(crate) call_pc: u32,
    pub(crate) callee: Register,
    pub(crate) this: Register,
    pub(crate) window: Register,
    pub(crate) argc: u16,
    pub(crate) result: Register,
    /// The call completes its caller's activation with its result (`return f(x)`): the row that
    /// follows it does (`steps::completing`), and nothing is live after it.
    pub(crate) returning: bool,
    pub(crate) form: CallForm,
    /// The method site read a `Method` call makes before its direct row (`None` for the other forms).
    pub(crate) method: Option<Method>,
    /// The opcode of the call record, which keys the fast-row counter of a planned direct call (`note_fast`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) op: quench_runtime::bytecode::Op,
}

impl CallShape {
    pub(crate) fn of(call_pc: u32, instruction: WideInstruction, form: CallForm) -> Self {
        let imm = instruction.imm();
        let (result, returning, op) = (instruction.result_register(), instruction.returns_from_frame(), instruction.op());
        match form {
            CallForm::Plain => Self {
                call_pc,
                callee: instruction.register_b(),
                this: instruction.register_c(),
                window: ImmediateLayout::call_window_base(imm),
                argc: ImmediateLayout::argument_count(imm),
                result,
                returning,
                form,
                method: None,
                op,
            },
            CallForm::Construct | CallForm::Stepped | CallForm::Method => Self { call_pc, callee: 0, this: 0, window: 0, argc: 0, result, returning, form, method: None, op },
        }
    }
}

/// The shape of the method call `instruction` at `call_pc` (`CallMethod`, a receiver register and a method site) as a
/// `Method` chain: its receiver is the direct row's `this`, its callee the `scratch` register the site read writes,
/// its arguments a window of consecutive registers the direct row reads. `None` for any other call, or one whose
/// arguments are not consecutive (`Arguments::List` is a list of registers, not a window).
pub(crate) fn method_shape(program: &quench_runtime::bytecode::ResidualProgram, instruction: WideInstruction, call_pc: u32, scratch: Register, returning: bool) -> Option<CallShape> {
    if !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::MethodCall) || instruction.writes_current_this() || instruction.resolves_dynamic_scope()
        || instruction.direct_eval() || instruction.parameter_eval() {
        return None;
    }
    let context = quench_runtime::staged::DecodeContext::of(program);
    let invocation = quench_runtime::staged::Invocation::of(instruction, context)?;
    let quench_runtime::staged::Receiver::Register(receiver) = invocation.receiver else { return None };
    let quench_runtime::staged::Arguments::List(list) = invocation.arguments else { return None };
    let start = list.start as usize;
    let registers = program.method_arguments.get(start..start + usize::from(list.count))?;
    let window = registers.first().copied().unwrap_or(0);
    if !registers.iter().enumerate().all(|(k, &register)| register == window + k as Register) {
        return None;
    }
    let quench_runtime::staged::Callee::Method { site, .. } = invocation.callee else { return None };
    let cache = context.method_site(site as usize)?.cache;
    Some(CallShape {
        call_pc, callee: scratch, this: receiver, window, argc: list.count, result: instruction.result_register(), returning,
        form: CallForm::Method, method: Some(Method { cache, scratch }), op: instruction.op(),
    })
}
