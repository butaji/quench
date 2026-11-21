//! What a loop region's code calls and inlines under `--b-jit`: the callees its call sites
//! recorded (`RegionCalls`), which its build stitches in or calls by their entries, and what its
//! published code owns of them (`CallCode`).

use super::super::*;
use super::super::base_region::callees::Callees;
use super::super::base_region::RealmProof;
use super::super::lane_facts::bind_callees;
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
    /// By PC, the callees each call site reached (`direct_callees_in_lane`): what a Cranelift
    /// region calls by their entries, as a lane body does (`unit_direct_calls`).
    pub(in crate::vm::compiled) direct: Vec<DirectCandidates>,
}

impl RegionCalls {
    /// The calls of a region Cranelift lowers: the call sites' `direct` candidates only, nothing
    /// stitched.
    pub(in crate::vm) fn direct(direct: Vec<DirectCandidates>) -> Self {
        Self { direct, ..Self::default() }
    }
}

impl Vm {
    /// The calls of `function` of `program`, whose call sites reached `candidates` (by PC), that a
    /// region of it may inline or call by their entries; the callees' bodies are in demand for the
    /// calls the region keeps, as a body's are (`bind_callees`, by the view `own` of the function).
    pub(in crate::vm) fn region_calls(&mut self, program: &ResidualProgram, function: u32, own: &crate::vm::dispatch_fast::LaneView, candidates: &[DirectCandidates]) -> RegionCalls {
        let code: Vec<WideInstruction> = program.function_get(function as usize).map(|body| body.instructions().collect()).unwrap_or_default();
        let callees = self.stitch_callees(program, function, &code, candidates);
        bind_callees(&callees.owned, own);
        RegionCalls { callees, realm: self.realm_proof(), direct: candidates.to_vec() }
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
    records: Vec<Box<CallRecord>>,
    /// The registers of the function the region belongs to: its inlined callees' follow them.
    caller_registers: u16,
    recovery: LaneRecoverySpace,
    pub(in crate::vm) invalidation: Invalidation,
}

impl CallCode {
    /// The calls of a region that inlines and enters none, whose code reads `invalidation`
    /// (`VmFacts::held_atoms`), which its publication sets when a fact it holds changes.
    pub(super) fn holding(invalidation: Invalidation) -> Self {
        Self { invalidation, ..Self::default() }
    }

    /// The calls of a region of a function with `caller_registers` registers: `inlined` as their
    /// callees' rows, `direct` by their entries (`records`: those its rows read).
    pub(super) fn of((inlined, direct, records): (Vec<InlinedCall>, Box<[DirectCallee]>, Vec<Box<CallRecord>>), caller_registers: u16, invalidation: Invalidation) -> Self {
        // The windows start past the function's environment window (`base_region::function::inline_base`):
        // the room covers that reach as well as the activations a leave inside a callee pushes.
        let recovery = LaneRecoverySpace::inlined(&inlined).reaching(window_width(&inlined, caller_registers));
        Self { inlined, direct, records, caller_registers, recovery, invalidation }
    }

    /// Releases the capacity its vectors hold past their length (`LaneDependencies::trim`).
    pub(in crate::vm) fn trim(&mut self) {
        self.inlined.shrink_to_fit();
        self.records.shrink_to_fit();
    }

    /// Whether its code calls a callee by its entry (a b-jit direct row, whose record it owns, or a
    /// Cranelift region's direct call): it runs on the lane frame's payload (`region_lend`), whose
    /// lane stack words the callee's window and frame are pushed through.
    pub(in crate::vm) fn enters_callees(&self) -> bool {
        !self.records.is_empty() || !self.direct.is_empty()
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
