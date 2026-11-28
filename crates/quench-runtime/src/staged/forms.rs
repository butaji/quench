//! The forms of a call instruction as data: where its callee, receiver and arguments live and
//! where its result goes. A form is decoded once from the record and the function's tables
//! (`DecodeContext`); the call protocol (`protocol`, `invoke`) reads these and never the opcode:
//!
//! - a callee is a register, a function the record names (`CallKnown`), or a method the site
//!   names, which the receiver's property rows load;
//! - a receiver is a register, the activation's `this` (through an optional field path), the
//!   `undefined` of a known call, or the object a construction's plan allocates;
//! - the arguments are a window of consecutive registers, or a list of registers the call
//!   records keep.

#[cfg(not(stencil_build))]
use super::DecodeContext;
#[cfg(not(stencil_build))]
use crate::bytecode::{ConstructArguments, ImmediateLayout, Op, WideInstruction, RETURN_REGISTER};
use crate::bytecode::Register;

/// The row of a plain call: a call through a callee register, a receiver register and a window
/// of arguments. `CallForm::of` reads its record as `CallForm::Plain` (the call arm), and
/// `Invocation::operands` reads the same record's callee and receiver.
#[cfg(not(stencil_build))]
pub const PLAIN_CALL_ROW: u16 = Op::Call as u16;

/// A function of the running program, by its index (`CallKnown`).
pub type FunctionId = u16;

/// The registers a method call passes: `count` entries of the program's method arguments from
/// `start`, in the order the arguments are passed, not necessarily consecutive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisterList {
    pub start: u32,
    pub count: u16,
}

impl RegisterList {
    /// The registers the list names in `records`; none where the records do not hold them.
    pub fn registers(self, records: &[Register]) -> &[Register] {
        let start = self.start as usize;
        records.get(start..start + usize::from(self.count)).unwrap_or(&[])
    }
}

/// Where a call's arguments are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arguments {
    /// `count` consecutive registers from `first`.
    Window { first: Register, count: u16 },
    /// The registers of a call record's list.
    List(RegisterList),
}

impl Arguments {
    /// How many arguments the call passes.
    #[inline]
    pub const fn count(self) -> u16 {
        match self {
            Self::Window { count, .. } | Self::List(RegisterList { count, .. }) => count,
        }
    }

    /// The registers the arguments name: a window's consecutive ones, or the list's in `records`.
    pub(crate) fn registers(self, records: &[Register]) -> impl Iterator<Item = Register> + '_ {
        let (window, listed) = match self {
            Self::Window { first, count } => (first..first + count, &[][..]),
            Self::List(list) => (0..0, list.registers(records)),
        };
        window.chain(listed.iter().copied())
    }

    /// The first register past a window; a list's registers are the call records' to name.
    pub(crate) const fn window_end(self) -> u32 {
        match self {
            Self::Window { first, count } => first as u32 + count as u32,
            Self::List(_) => 0,
        }
    }
}

/// Where a call's callee comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Callee {
    /// The value of a register, whose identity the call binds to or checks.
    Register(Register),
    /// A function of the program the record names: its identity is a static operand.
    Known(FunctionId),
    /// The property `atom` of the receiver, through method site `site`.
    Method { atom: u32, site: u32 },
}

/// Where a call's receiver comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Receiver {
    Register(Register),
    /// The activation's `this`, or the field of it a path reads (an atom and its cache site).
    This { path: Option<(u32, u16)> },
    /// `undefined`: a known function is called with no receiver.
    Undefined,
    /// The object a construction's plan allocates.
    Allocated,
    /// The base a read met, whose accessor the call is (a getter's receiver): a value, not an
    /// operand, which the frame holds (`CallFrame::call_base`).
    Base,
}

/// The static operands of a call instruction: its callee, receiver and arguments, and where its
/// result goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Invocation {
    pub callee: Callee,
    pub receiver: Receiver,
    pub arguments: Arguments,
    pub result: Register,
    /// Whether the call's completion is its activation's: the call returns its callee's value.
    pub returns: bool,
    /// Whether the window's last argument is the caller's `arguments`, which the activation keeps
    /// as values (`CallForwardingArguments`).
    pub forwards: bool,
}

/// What a tier fixes around its calls: the registers its code holds, whether it completes its
/// activations by status (a call may return its callee's completion), whether its call reads the
/// values a forwarding call keeps, and whether its VM call reads the forms beyond a register
/// window (a known function, a method site).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallFacts {
    pub registers: u16,
    pub activates: bool,
    pub forwarding: bool,
    pub forms: bool,
}

impl CallFacts {
    /// Whether the tier holds `register`.
    pub(crate) const fn holds(self, register: Register) -> bool {
        register < self.registers
    }
}

impl Invocation {
    #[cfg(not(stencil_build))]
    /// The operands of `instruction`, read with the tables of `context`; `None` for any form no
    /// row runs: an eval, which the host decides, a super construction, and an array of arguments.
    #[inline(always)]
    pub fn of(instruction: WideInstruction, context: DecodeContext<'_>) -> Option<Self> {
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

    /// Whether the call allocates its callee's receiver (a construction) instead of binding the
    /// `this` of a receiver.
    #[inline(always)]
    pub fn constructs(&self) -> bool {
        self.receiver == Receiver::Allocated
    }

    /// The first register past the call's window (`Arguments::window_end`).
    pub fn window_end(&self) -> u32 {
        self.arguments.window_end()
    }

    /// Whether the call's operands are all registers of the caller's file: a register callee and
    /// receiver and a window, the form the VM's register call reads.
    fn in_registers(&self) -> bool {
        matches!(self.callee, Callee::Register(_)) && matches!(self.receiver, Receiver::Register(_) | Receiver::Allocated)
            && matches!(self.arguments, Arguments::Window { .. })
    }

    /// The registers the call names outside its arguments.
    fn named(&self) -> impl Iterator<Item = Register> {
        let callee = match self.callee { Callee::Register(register) => Some(register), _ => None };
        let receiver = match self.receiver { Receiver::Register(register) => Some(register), _ => None };
        [callee, receiver, Some(self.result)].into_iter().flatten()
    }

    /// Whether a tier of `facts` lowers the call: it completes by status, holds the registers
    /// the call names and its whole window, reads a forwarding call's kept values if the call
    /// forwards, and reads the call's form.
    pub(crate) fn admits(&self, facts: CallFacts) -> bool {
        facts.activates && (facts.forwarding || !self.forwards) && (facts.forms || self.in_registers())
            && self.window_end() <= u32::from(facts.registers) && self.named().all(|register| facts.holds(register))
    }
}

/// The form a call-family record makes, for every record of the family: also the ones with no
/// staged operation (a direct or parameter eval, whose callee the host decides; a construction
/// over an array of arguments). Where a record decodes, its operation has the same form
/// (`Invocation`, `Operation::SpreadCall`, `Operation::SuperCall`); a tier that asks what kind of
/// call a record is asks this, not the opcode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallForm {
    /// A call through a callee register and a receiver register, over a window of arguments.
    Plain,
    /// A call of a function the record names.
    Known,
    /// A method call through a method site.
    Method,
    /// A call that passes the activation's `arguments` as its last argument.
    Forwarding,
    /// A construction, allocating its receiver.
    Construct,
    /// A call whose one argument register holds the array to apply.
    Array,
    /// A call whose one argument register holds the iterable to spread.
    Spread,
    /// A call whose one argument register holds a rest view.
    Rest,
    /// `super(...)`.
    Super,
}

#[cfg(not(stencil_build))]
impl CallForm {
    /// The form of the call-family record `instruction`, whatever its result flags; `None` for a
    /// record of no call family.
    pub fn of(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::Call => Self::Plain,
            Op::CallForwardingArguments => Self::Forwarding,
            Op::CallKnown => Self::Known,
            Op::CallMethod | Op::CallThisMethod => Self::Method,
            Op::Construct if instruction.is_super_construct() => Self::Super,
            Op::Construct => Self::Construct,
            Op::CallArray => Self::Array,
            Op::CallSpread => Self::Spread,
            Op::CallRest => Self::Rest,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{MethodSite, ReceiverPath, SET_THIS_REGISTER};
    use crate::staged::test_words::FACTS;

    fn call_instruction(op: Op, constructs: bool) -> WideInstruction {
        if constructs {
            WideInstruction::new(op, 4, 5, 6, ImmediateLayout::construct_immediate(2, false, false))
        } else {
            WideInstruction::new(op, 4, 5, 6, ImmediateLayout::call_immediate(7, 2, false, false))
        }
    }

    fn instruction(op: Op, result: Register, imm: u32) -> WideInstruction {
        WideInstruction::new(op, result, 5, 6, imm)
    }

    const SITES: [MethodSite; 2] = [
        MethodSite { atom: 8, cache: 3, argument_start: 0, argument_count: 2, receiver_path: ReceiverPath::NONE },
        MethodSite { atom: 9, cache: 4, argument_start: 2, argument_count: 1, receiver_path: ReceiverPath::at(11, 6) },
    ];
    const RECORDS: [Register; 3] = [4, 2, 7];

    fn methods() -> DecodeContext<'static> {
        DecodeContext { method_sites: &SITES, ..DecodeContext::EMPTY }
    }

    #[test]
    fn the_operands_of_a_call_and_a_construction_decode_once() {
        let call = Invocation::of(call_instruction(Op::Call, false), DecodeContext::EMPTY).expect("a call decodes");
        assert_eq!(call, Invocation { callee: Callee::Register(5), receiver: Receiver::Register(6),
            arguments: Arguments::Window { first: 7, count: 2 }, result: 4, returns: false, forwards: false });
        let construct = Invocation::of(call_instruction(Op::Construct, true), DecodeContext::EMPTY).expect("a construction decodes");
        assert_eq!((construct.callee, construct.receiver, construct.arguments.count(), construct.result),
            (Callee::Register(5), Receiver::Allocated, 2, 4));
        assert_eq!(Invocation::of(call_instruction(Op::Move, false), DecodeContext::EMPTY), None);
        let decoded = crate::staged::Operation::<u64, ()>::decode(call_instruction(Op::Call, false), DecodeContext::EMPTY);
        assert!(matches!(decoded, Some(crate::staged::Operation::Call(crate::staged::CallOperation::Site(site))) if site == call));
    }

    #[test]
    fn the_forms_with_no_row_have_no_invocation() {
        let eval = ImmediateLayout::call_immediate(7, 2, true, false);
        let parameter_eval = ImmediateLayout::call_immediate(7, 2, false, true);
        let sup = ImmediateLayout::construct_immediate(2, true, false);
        let array = ImmediateLayout::construct_immediate(2, false, true);
        for (op, imm) in [(Op::Call, eval), (Op::Call, parameter_eval), (Op::Construct, sup), (Op::Construct, array)] {
            assert_eq!(Invocation::of(instruction(op, 4, imm), DecodeContext::EMPTY), None, "{op:?}");
        }
        let flagged = instruction(Op::Call, 4 | SET_THIS_REGISTER, ImmediateLayout::call_immediate(7, 2, false, false));
        assert_eq!(Invocation::of(flagged, DecodeContext::EMPTY), None);
    }

    #[test]
    fn a_forwarding_call_decodes_with_its_flag_and_a_returning_one_with_its_own() {
        let imm = ImmediateLayout::call_immediate(7, 2, false, false);
        let forwarding = Invocation::of(instruction(Op::CallForwardingArguments, 4, imm), DecodeContext::EMPTY).expect("a forwarding call decodes");
        assert!(forwarding.forwards && !forwarding.constructs());
        let returning = Invocation::of(instruction(Op::Call, 4 | RETURN_REGISTER, imm), DecodeContext::EMPTY).expect("a returning call decodes");
        assert!(returning.returns && !returning.forwards && returning.result == 4);
    }

    #[test]
    fn a_known_call_names_its_function_and_has_no_receiver_register() {
        let imm = ImmediateLayout::call_immediate(7, 2, false, false);
        let known = Invocation::of(instruction(Op::CallKnown, 4, imm), DecodeContext::EMPTY).expect("a known call decodes");
        assert_eq!((known.callee, known.receiver, known.arguments), (Callee::Known(5), Receiver::Undefined, Arguments::Window { first: 7, count: 2 }));
        assert!(known.admits(CallFacts { forms: true, ..FACTS }), "a tier that reads the form admits it");
        assert!(!known.admits(FACTS), "the register call of a tier that reads no other form");
    }

    #[test]
    fn a_method_call_takes_its_receiver_atom_and_registers_from_the_method_site() {
        let method = Invocation::of(instruction(Op::CallMethod, 4, 0), methods()).expect("a method call decodes");
        assert_eq!((method.callee, method.receiver), (Callee::Method { atom: 8, site: 0 }, Receiver::Register(5)));
        let Arguments::List(list) = method.arguments else { panic!("a method call passes a list") };
        assert_eq!(list.registers(&RECORDS), [4, 2], "the registers need not be consecutive");
        let this = Invocation::of(instruction(Op::CallThisMethod, 4, 1), methods()).expect("a this-method call decodes");
        assert_eq!((this.callee, this.receiver), (Callee::Method { atom: 9, site: 1 }, Receiver::This { path: Some((11, 6)) }));
        assert_eq!(Invocation::of(instruction(Op::CallMethod, 4, 2), methods()), None, "a site the table lacks");
        assert_eq!(Invocation::of(instruction(Op::CallMethod, 4, 0), DecodeContext::EMPTY), None, "a tier with no table");
        assert_eq!(RegisterList { start: 2, count: 5 }.registers(&RECORDS), [] as [Register; 0], "records past the table name none");
    }

    #[test]
    fn a_tier_admits_the_calls_whose_window_and_registers_it_holds() {
        let call = Invocation { callee: Callee::Register(1), receiver: Receiver::Register(2),
            arguments: Arguments::Window { first: 7, count: 3 }, result: 0, returns: false, forwards: false };
        assert!(call.admits(FACTS), "a window ending at the last register fits");
        assert!(!Invocation { arguments: Arguments::Window { first: 7, count: 4 }, ..call }.admits(FACTS), "a window past the registers");
        assert!(!Invocation { callee: Callee::Register(10), ..call }.admits(FACTS), "a callee the tier does not hold");
        assert!(!call.admits(CallFacts { activates: false, ..FACTS }), "a tier that completes no activation");
        let forwarding = Invocation { forwards: true, ..call };
        assert!(!forwarding.admits(FACTS) && forwarding.admits(CallFacts { forwarding: true, ..FACTS }));
    }
}
