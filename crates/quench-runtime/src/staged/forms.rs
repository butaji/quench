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
    pub fn registers(self, records: &[Register]) -> impl Iterator<Item = Register> + '_ {
        let (window, listed) = match self {
            Self::Window { first, count } => (first..first + count, &[][..]),
            Self::List(list) => (0..0, list.registers(records)),
        };
        window.chain(listed.iter().copied())
    }

    /// The first register past a window; a list's registers are the call records' to name.
    pub const fn window_end(self) -> u32 {
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
    pub const fn holds(self, register: Register) -> bool {
        register < self.registers
    }
}

impl Invocation {

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
    pub fn admits(&self, facts: CallFacts) -> bool {
        facts.activates && (facts.forwarding || !self.forwards) && (facts.forms || self.in_registers())
            && self.window_end() <= u32::from(facts.registers) && self.named().all(|register| facts.holds(register))
    }
}

/// The record of a call-family instruction: which call it makes, for every record of the family, also
/// the ones with no staged operation (a direct or parameter eval, whose callee the host decides; a
/// construction over an array of arguments). Where a record decodes, its operation has the same form
/// (`Invocation`, `Operation::SpreadCall`, `Operation::SuperCall`); a tier that asks what kind of
/// call a record is asks this, not the opcode. `CallRecord::of` is the one mapping from a call-family
/// opcode to its record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallRecord {
    /// A call through a callee register and a receiver register, over a window of arguments.
    Plain,
    /// A call of a function the record names (`CallKnown`).
    Known,
    /// A method call through a method site, on the receiver register (`CallMethod`).
    Method,
    /// A method call on the activation's `this` (`CallThisMethod`): its receiver is `this`, which no register holds.
    ThisMethod,
    /// A call that passes the activation's `arguments` as its last argument.
    Forwarding,
    /// A construction, allocating its receiver.
    Construct,
    /// `super(...)`: the construction the super initializer makes.
    Super,
    /// A call whose one argument register holds the array to apply.
    Array,
    /// A call whose one argument register holds the iterable to spread.
    Spread,
    /// A call whose one argument register holds a rest view.
    Rest,
    /// A first-call stub's call of its chunk (`CallFirstChunk`).
    FirstChunk,
}


#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

#[cfg(test)]
mod tests;
