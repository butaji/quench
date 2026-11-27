//! A function's call plan: the facts about its activation derived once from its static layout, packed as
//! flag bits so an activation reads one cached word instead of re-testing each flag.

/// `CallPlan`'s flags as bits, in this order; the struct literal names every field, so a
/// field left out of the list does not compile.
macro_rules! call_plan_bits {
    ($($field:ident),* $(,)?) => {
        pub const CALL_PLAN_FLAG_COUNT: u32 = [$(stringify!($field)),*].len() as u32;

        impl CallPlan {
            pub(super) fn to_bits(self) -> u32 {
                let mut bits = 0;
                let mut index = 0;
                $(bits |= u32::from(self.$field) << index; index += 1;)*
                let _ = index;
                bits
            }

            #[inline(always)]
            pub(super) fn from_bits(bits: u32) -> Self {
                let mut index = 0;
                $(let $field = bits & (1 << index) != 0; index += 1;)*
                let _ = index;
                Self { $($field),* }
            }
        }
    };
}
call_plan_bits!(
    plain,
    in_loop,
    settles_in_loop,
    legacy_reflection,
    live_parameters,
    addresses_locals,
    forwards_arguments,
    keeps_arguments,
    reads_passed_arguments,
    registers_only,
    environment_activation,
    suspended_registers,
    suspended_environment,
    admits_direct_eval,
    tail_callee,
);

#[derive(Clone, Copy, Debug)]
pub struct CallPlan {
    /// `plain_activation`.
    pub(crate) plain: bool,
    /// The dispatch loop may push the activation in place: an ordinary body that is no
    /// generator, async function, class constructor or field initializer and raises no
    /// parameter-scope `arguments` error.
    pub in_loop: bool,
    /// An async function whose body never suspends (`Function::suspends`): a plain call may
    /// push it in place as `in_loop` bodies are pushed, settling the promise it returns with
    /// the body's completion (`PendingGeneralCall::settles`).
    pub settles_in_loop: bool,
    /// Legacy `caller`/`arguments` reflection may observe the activation
    /// (`!has_restricted_legacy_caller_access`).
    pub legacy_reflection: bool,
    /// `reflects_live_parameters`.
    pub(crate) live_parameters: bool,
    /// Some instruction can reach the frame's local slots: one names a slot
    /// (`addressed_local_slots`), creates a closure or clones an environment. A plain
    /// activation without that needs no local storage, since promoted bindings live in
    /// registers and nothing else reads its locals.
    pub addresses_locals: bool,
    /// The activation passes its `arguments` binding only straight into
    /// `CallForwardingArguments` calls, so it keeps the argument values instead of an
    /// `arguments` object until a call needs one (`Function::kept_arguments`).
    pub forwards_arguments: bool,
    /// `forwards_arguments`, or a frameless activation answers the binding from the values
    /// it kept and makes the object only at its last read (`KeptArguments::Passes`). Its
    /// frame makes the object on entry.
    pub keeps_arguments: bool,
    /// `Function::reads_passed_arguments_only`: an environment activation keeps every value
    /// passed after its locals, past its parameters too.
    pub reads_passed_arguments: bool,
    /// The activation's whole state is its registers, plus the values it was passed when it
    /// forwards them: it reaches no local but a forwarded `arguments` binding, and legacy
    /// reflection, if any, reads its parameter registers or the forwarded values. A lane
    /// may run such a body without a frame: an ordinary body (`in_loop`), a base class
    /// constructor's construction, or a `settles_in_loop` body with no rest parameter or
    /// `arguments` binding, whose return settles its call's promise (`CallEntry::framed(Framed::Settles)`).
    pub registers_only: bool,
    /// The activation's state is its registers, its frame locals and the environment its
    /// closures capture, which it may make on entry: an ordinary body but for the closures
    /// it makes and the iteration copies of its environment, with no `arguments`, binding
    /// site, dynamic scope access or superinstruction, whose every local access is a plain or
    /// environment load, store or dead-zone opening of a selectively captured layout. A lane
    /// may run such a body without a frame (`CallEntry::framed(Framed::Environment)`).
    pub environment_activation: bool,
    /// A function that suspends (a generator, an async function or an async generator) laid out
    /// as its registers: no locals, `arguments`, closures or dynamic scope, so what its
    /// suspended frame holds is its registers, receiver and closure environment (its implicit
    /// bindings are read through the frame an opcode that needs them makes of the record). A
    /// lane resumes it as an activation whose window is the frame's register file, and its
    /// `yield` or `await` suspends it back into that file.
    pub(crate) suspended_registers: bool,
    /// A function that suspends whose frame locals and environment ride a window past its
    /// registers (`locals_ride_window`), which a lane resumes as an activation over the
    /// frame's register file, binding the window from the record as a call binds a fresh one
    /// (`FrameRecord::open_environment_window`) and storing it back at the suspension. Its
    /// names may resolve through `with` objects and dynamic bindings, which its record carries
    /// and its window's handlers read (`scoped_names`).
    pub suspended_environment: bool,
    /// A function whose window activations run the direct evals and the closures at binding
    /// sites it holds in the lane (`evals_in_scope`): its window rides (`holds_window_scope`),
    /// and the eval promotes the window's environment into the activation's record, which owns
    /// the eval's variable environment (`Vm::eval_owner`).
    pub admits_direct_eval: bool,
    /// An ordinary body (neither async nor a generator), which a proper tail call binds over the caller's
    /// frame (`Vm::tail_call_reuses_frame`); a suspending body keeps its own activation.
    pub tail_callee: bool,
}

impl CallPlan {
    /// Whether the function's activations run over an environment window (an ordinary body's or a
    /// suspended frame's), where names resolve through the activation's scope (`scoped_names`).
    pub fn holds_window_scope(self) -> bool {
        self.environment_activation || self.suspended_environment
    }
}
