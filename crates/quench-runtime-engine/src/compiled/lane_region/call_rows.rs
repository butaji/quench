//! The call family lowered by its rows' data: the lane asks the staged operation whether its
//! facts admit it (`Operation::admitted`: the registers it holds, completing by status, no
//! forwarding), and runs it, not the opcode it decoded from. A construction whose window is not
//! the caller's, or that no row admits, takes the construct plan (`Lowering::construct`).
use super::*;
use super::super::lane_unit::Origin;
use quench_runtime::staged::{CallFacts, CallOperation, Invocation, Operation};

impl Lowering<'_, '_> {
    /// What this lowering fixes around its calls (`CallFacts`): the unit's registers, and whether
    /// the run completes activations by status. Its call helper reads no forwarding values; it
    /// reads every other form of a call from the record's decode (`dispatch_fast::BodyCall`): a
    /// known function, a method site, a window or a list of argument registers.
    fn call_facts(&self) -> CallFacts {
        CallFacts { registers: self.unit.registers, activates: self.completion.status_and_pc(), forwarding: false, forms: true }
    }

    /// The call or return row of `instruction` where the facts admit it.
    fn admitted_call(&self, instruction: WideInstruction) -> Option<Operation<ir::Value, EmitProof>> {
        let row = Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program))?;
        row.admitted(self.call_facts())?.then_some(row)
    }

    /// Whether `instruction` is a call or return row the facts admit.
    pub(crate) fn is_call_row(&self, instruction: WideInstruction) -> bool {
        self.admitted_call(instruction).is_some()
    }

    /// Whether `instruction` is a construction with its arguments in registers, which the
    /// construct plan lowers where no call row did.
    pub(crate) fn is_construction(&self, instruction: WideInstruction) -> bool {
        !self.completion.numeric() && Invocation::of(instruction, quench_runtime::staged::DecodeContext::of(self.program)).is_some_and(|site| site.constructs())
    }

    /// Whether the call at `pc` is a record of the caller whose window its registers hold: the
    /// registers the VM's call reads from the lane's file.
    fn in_callers_window(&self, pc: usize, site: &Invocation) -> bool {
        matches!(self.unit.origins.get(pc), Some(Origin::Caller(_))) && site.window_end() <= u32::from(self.unit.caller_registers)
    }

    /// The admitted row at `pc`, lowered by its staged definition: a return completes the
    /// activation, a call or construction in the caller's window is the staged call (a Math
    /// function of Numbers runs inline), and any other construction takes its plan.
    pub(crate) fn call_row_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        match self.admitted_call(instruction).expect("an admitted call row decodes") {
            Operation::Return { source } => Some(self.complete(state, pc, source)),
            Operation::Call(CallOperation::Site(site)) if site.constructs() && !self.in_callers_window(pc, &site) =>
                self.construct(state, pc, instruction),
            _ => Some(self.intrinsic_call(state, pc, instruction) || self.call(state, pc, instruction)),
        }
    }
}
