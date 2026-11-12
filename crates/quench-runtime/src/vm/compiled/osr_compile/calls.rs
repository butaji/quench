//! What a loop region's code calls and inlines under `--b-jit`: the callees its call sites
//! recorded (`RegionCalls`), which its build stitches in or calls by their entries, and what its
//! published code owns of them (`CallCode`).

use super::super::*;
use super::super::base_region::callees::Callees;
use super::super::base_region::RealmProof;
use crate::bytecode::{ResidualProgram, WideInstruction};
use crate::code_dependencies::Invalidation;
use crate::vm::dispatch_fast::LaneRecoverySpace;
use quench_bjit::call_record::CallRecord;

/// The callees the call sites of a region's function recorded, and the realm they were found in:
/// what the region's build stitches in. The default has no call site: every call stays a call.
#[derive(Default)]
pub(in crate::vm) struct RegionCalls {
    pub(in crate::vm::compiled) callees: Callees,
    pub(in crate::vm::compiled) realm: RealmProof,
}

impl<H: Host> Vm<H> {
    /// The calls of `function` of `program`, whose call sites reached `candidates` (by PC), that a
    /// region of it may inline.
    pub(in crate::vm) fn region_calls(&mut self, program: &ResidualProgram, function: u32, candidates: &[DirectCandidates]) -> RegionCalls {
        let code: Vec<WideInstruction> = program.function_get(function as usize).map(|body| body.instructions().collect()).unwrap_or_default();
        RegionCalls { callees: self.stitch_callees(program, function, &code, candidates), realm: self.realm_proof() }
    }
}

/// What a region's published code owns of its calls: those it runs as their callees' rows (the
/// frames a leave inside one recovers, over registers past its function's own, and the room that
/// recovery needs), the callees it calls by their entries and the records its direct rows read,
/// which its code must not outlive, and the invalidation word its rows read where a call may have
/// changed a fact they fold. A collection that frees a callee it names retires the code
/// (`ProgramStore::retire_dead_native_calls`).
#[derive(Default)]
pub(in crate::vm) struct CallCode {
    pub(in crate::vm) inlined: Vec<InlinedCall>,
    pub(in crate::vm) direct: Box<[DirectCallee]>,
    _records: Vec<Box<CallRecord>>,
    /// The registers of the function the region belongs to: its inlined callees' follow them.
    caller_registers: u16,
    recovery: LaneRecoverySpace,
    pub(in crate::vm) invalidation: Invalidation,
}

impl CallCode {
    /// The calls of a region of a function with `caller_registers` registers: `inlined` as their
    /// callees' rows, `direct` by their entries (`records`: those its rows read).
    pub(super) fn of((inlined, direct, records): (Vec<InlinedCall>, Box<[DirectCallee]>, Vec<Box<CallRecord>>), caller_registers: u16, invalidation: Invalidation) -> Self {
        let recovery = LaneRecoverySpace::inlined(&inlined);
        Self { inlined, direct, _records: records, caller_registers, recovery, invalidation }
    }

    /// The room a leave inside an inlined callee needs, which a run is admitted by.
    pub(in crate::vm) fn recovery_space(&self) -> LaneRecoverySpace {
        self.recovery
    }

    /// The registers past the function's own that its inlined callees occupy, in the activation's
    /// window: none where it inlines nothing.
    pub(in crate::vm) fn window(&self) -> usize {
        window_width(&self.inlined, self.caller_registers)
    }

    pub(in crate::vm) fn caller_registers(&self) -> u16 {
        self.caller_registers
    }

    /// Whether the code names a callee `live` says died.
    pub(in crate::vm) fn names_dead(&self, live: impl Fn(Value) -> bool) -> bool {
        names_dead_callee(&self.inlined, &self.direct, live)
    }

    /// The callees its calls name by identity, which a collection that frees one retires it over
    /// while its code compiles.
    pub(in crate::vm) fn identities(&self) -> Box<[Value]> {
        self.inlined.iter().filter_map(|call| call.target.identity()).chain(self.direct.iter().map(|callee| callee.callee)).collect()
    }
}
