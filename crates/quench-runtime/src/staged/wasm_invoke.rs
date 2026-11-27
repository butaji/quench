//! A Wasm call as a site of the one call step (`invoke::invoke`): the step JavaScript calls run, so
//! a Wasm call and a JavaScript call take the same order (callee, receiver, room, activation) and
//! every improvement to it reaches both. A Wasm function's call has no receiver and a callee the
//! record names, so its plan folds both (`Identity::Folded`, `Receiver::Undefined`); what is the
//! Wasm call's own is the frame (`WasmCallFrame`), over the lane's primitives (`WasmActivation`):
//!
//! - `call_entry`: a direct plan of the function the record names, or the VM's call where the lane
//!   makes no calls inline;
//! - `call_room`: the native entry first (`enter_native`, the rule the general loop makes too), whose
//!   code runs on the native stack; else the callee's shape and the lane's room for its activation;
//! - `call_hand_over`: the native run's resumption, or the activation pushed and entered.
//!
//! A call the step leaves (`Step::Unresolved`) goes where the frame recorded: the general frame
//! push where the lane had no room (`push_frame`), else out of the lane.

use super::invoke::{invoke, CallFrame, Room};
use super::plan::{CalleeKind, CallEntry, DirectPlan, Identity};
use super::wasm_activation::{call_import, enter_native, push_frame, CallTarget, NativeEntry, WasmActivation};
use super::region::Region;
use super::stage::Stage;
use super::{word_offset, Arguments, Callee, Concrete, Constant, Frame, FunctionId, Invocation, NoPlans, PropertyFrame, Receiver, Step};
use crate::bytecode::Register;

/// Where the frame admitted a call (`WasmCallFrame::call_room`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Admitted {
    /// Not yet decided.
    Pending,
    /// Native code ran the callee.
    Native,
    /// The lane holds the callee's activation, of this shape.
    Activation,
    /// The lane has no room for the activation: the general loop pushes the call's frame.
    NoRoom,
    /// The call leaves the lane.
    Exit,
}

/// The frame of a Wasm call made in the lane: the call's primitives and where it was admitted.
struct WasmCallFrame<'a, A: WasmActivation> {
    call: &'a mut A,
    function: u32,
    shape: Option<A::Shape>,
    admitted: Admitted,
    /// Where the caller resumes after native code ran the callee.
    ran: Option<A::Resume>,
}

impl<A: WasmActivation> Frame<Concrete> for WasmCallFrame<'_, A> {
    type Fact = ();
    type Control = A::Resume;
    fn fact(&self, _register: Register) {}
    fn read(&mut self, stage: &mut Concrete, register: Register) -> u64 {
        let file = self.call.caller_registers();
        stage.load_u64(Region::Frame.mutable(), file, word_offset(register))
    }
    fn write(&mut self, _stage: &mut Concrete, _register: Register, _value: u64, _fact: ()) {
        unreachable!("a Wasm call writes no register before its callee runs")
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 {
        crate::Value::UNDEFINED.to_bits()
    }
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {
        unreachable!("a Wasm frame has no this")
    }
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) {
        (constant.value.to_bits(), ())
    }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> {
        None
    }
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> A::Resume {
        unreachable!("a call's frame takes no branch")
    }
    fn jump(&mut self, _stage: &mut Concrete) -> A::Resume {
        unreachable!("a call's frame takes no jump")
    }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (crate::Value::boolean(condition).to_bits(), ())
    }
}

impl<A: WasmActivation> PropertyFrame<Concrete> for WasmCallFrame<'_, A> {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans {
        NoPlans
    }
}

impl<A: WasmActivation> CallFrame<Concrete> for WasmCallFrame<'_, A> {
    /// The function the record names, folded; the VM's call where the lane makes no calls inline.
    #[inline(always)]
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        match site.callee {
            Callee::Known(_) => CallEntry::Direct(DirectPlan {
                kind: CalleeKind::Script,
                identity: Identity::Folded(u64::from(self.function)),
                sloppy: None,
                registers: 0,
                returned: 0,
                bound: None,
            }),
            _ => CallEntry::Helper,
        }
    }

    #[inline(always)]
    fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(self.call.caller_registers())
    }

    /// The native entry first (`enter_native`), whose code runs on the native stack; else the
    /// callee's shape and the lane's room for its activation, in the lane's order: activations, the
    /// frames and activations together, storage, then the value stack. Leaves where neither holds
    /// the call, having recorded where it goes.
    #[inline(always)]
    fn call_room(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<Room<u64>> {
        let room = Room { cell: 0, top: 0, next: 0, shadow: 0 };
        if !self.call.inline_calls() {
            self.admitted = Admitted::Exit;
            return None;
        }
        match enter_native(self.call, self.function) {
            NativeEntry::Ran(resumed) => {
                (self.admitted, self.ran) = (Admitted::Native, Some(resumed));
                Some(room)
            }
            NativeEntry::Leave => {
                self.admitted = Admitted::Exit;
                None
            }
            NativeEntry::Interpret => self.activation_room().then_some(room),
        }
    }

    /// The native run's resumption, or the activation pushed and entered.
    #[inline(always)]
    fn call_hand_over(&mut self, _stage: &mut Concrete, _this: u64, _room: Room<u64>) -> Option<A::Resume> {
        if let Some(resumed) = self.ran.take() {
            return Some(resumed);
        }
        let shape = self.shape.take()?;
        Some(self.call.push_activation(self.function, shape))
    }
}

impl<A: WasmActivation> WasmCallFrame<'_, A> {
    /// Whether the lane holds the callee's activation: its shape, then the room checks in the lane's
    /// order. Records the shape, or that the call takes the general frame push.
    #[inline(always)]
    fn activation_room(&mut self) -> bool {
        let Some(shape) = self.call.shape(self.function) else {
            self.admitted = Admitted::NoRoom;
            return false;
        };
        let registers = A::shape_registers(shape);
        let room = self.call.activation_room()
            && self.call.frame_room()
            && self.call.storage_room(registers)
            && self.call.values_room(registers);
        self.admitted = if room { Admitted::Activation } else { Admitted::NoRoom };
        self.shape = room.then_some(shape);
        room
    }
}

/// The known call of `function` with the window `window` into `result` through the one call step,
/// at any stage: the interpreter's lane (`step_call_known_of`) and the native tier's lowering run
/// this one site, a return call (`returns`) the frame's activation where it replaces the caller.
#[inline(always)]
pub fn step_known_call_of<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss,
    (function, (first, count), result, returns): (FunctionId, (Register, u16), Register, bool)) -> Step<S::Word, F::Control> {
    let site = Invocation {
        callee: Callee::Known(function),
        receiver: Receiver::Undefined,
        arguments: Arguments::Window { first, count },
        result,
        returns,
        forwards: false,
    };
    invoke(stage, frame, miss, site)
}

/// The call of `function` with the window `window` and result `result` through the one call step:
/// where the caller resumes, else where the frame admitted the call.
#[inline(always)]
fn invoke_function<A: WasmActivation>(call: &mut A, function: FunctionId, window: (Register, u16), result: Register)
    -> Result<A::Resume, Admitted> {
    let mut frame = WasmCallFrame { call, function: u32::from(function), shape: None, admitted: Admitted::Pending, ran: None };
    match step_known_call_of(&mut Concrete::pure(), &mut frame, (), (function, window, result, false)) {
        Step::Control(resumed) => Ok(resumed),
        _ => Err(frame.admitted),
    }
}

/// A known call of `function`, the record's callee: an activation of the callee or its native run,
/// else the call's frame where the lane has no room. `None` leaves the lane.
#[inline(always)]
pub fn step_call_known_of<A: WasmActivation>(call: &mut A, function: FunctionId, window: (Register, u16), result: Register)
    -> Option<A::Resume> {
    match invoke_function(call, function, window, result) {
        Ok(resumed) => Some(resumed),
        Err(Admitted::NoRoom) => Some(push_frame(call)),
        Err(_) => None,
    }
}

/// A reference call: its callee checked first (a function of the running program and instance, or
/// an import), then a function's call made as a known call's is. Any other callee, or no room,
/// leaves the lane.
#[inline(always)]
pub fn step_call_reference_of<A: WasmActivation>(call: &mut A, window: (Register, u16), result: Register) -> Option<A::Resume> {
    match call.reference() {
        CallTarget::Function(function) => invoke_function(call, FunctionId::try_from(function).ok()?, window, result).ok(),
        CallTarget::Import => call_import(call),
        CallTarget::Other => None,
    }
}
