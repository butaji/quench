//! What a loop region's code calls and inlines under the b-jit tier: the callees its call sites
//! recorded (`RegionCalls`), which its build stitches in or calls by their entries, and what its
//! published code owns of them (`CallCode`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
use super::super::base_region::callees::Callees;
use super::super::base_region::RealmProof;
use super::super::lane_facts::bind_callees;
use quench_runtime::bytecode::{ResidualProgram, WideInstruction};
use quench_runtime::code_dependencies::Invalidation;
use crate::dispatch_fast::LaneRecoverySpace;

/// The callees the call sites of a region's function recorded, and the realm they were found in:
/// what the region's build stitches in. The default has no call site: every call stays a call.
#[derive(Default)]
pub(crate) struct RegionCalls {
    pub(crate) callees: Callees,
    pub(crate) realm: RealmProof,
    /// By PC, the callees each call site reached (`direct_callees_in_lane`): what a Cranelift
    /// region calls by their entries, as a lane body does (`unit_direct_calls`).
    pub(crate) direct: Vec<DirectCandidates>,
}

impl RegionCalls {
    /// The calls of a region Cranelift lowers: the call sites' `direct` candidates only, nothing
    /// stitched.
    pub(crate) fn direct(direct: Vec<DirectCandidates>) -> Self {
        Self { direct, ..Self::default() }
    }
}

#[quench_macros::ext(CompiledOsrCompileCallsVm)]
impl Vm {
    /// The calls of `function` of `program`, whose call sites reached `candidates` (by PC), that a
    /// region of it may inline or call by their entries; the callees' bodies are in demand for the
    /// calls the region keeps, as a body's are (`bind_callees`, by the view `own` of the function).
    pub(crate) fn region_calls(&mut self, program: &ResidualProgram, function: u32, own: &crate::dispatch_fast::LaneView, candidates: &[DirectCandidates]) -> RegionCalls {
        let code: Vec<WideInstruction> = program.function_get(function as usize).map(|body| body.instructions().collect()).unwrap_or_default();
        let callees = self.stitch_callees(program, function, &code, candidates);
        bind_callees(&callees.owned, own);
        RegionCalls { callees, realm: self.realm_proof(), direct: candidates.to_vec() }
    }
}

pub(crate) use quench_runtime::vm::engine_types::CallCode;

#[quench_macros::ext(CallCodeOps1)]
impl CallCode {
    /// The calls of a region that inlines and enters none, whose code reads `invalidation`
    /// (`VmFacts::held_atoms`), which its publication sets when a fact it holds changes.
    pub(crate) fn holding(invalidation: Invalidation) -> Self {
        Self { invalidation, ..Self::default() }
    }

    /// The calls of a region of a function with `caller_registers` registers: `inlined` as their
    /// callees' rows, `direct` by their entries (`records`: those its rows read).
    pub(crate) fn of((inlined, direct, records): (Vec<InlinedCall>, Box<[DirectCallee]>, Vec<crate::compiled::OwnedCallRecord>), caller_registers: u16, invalidation: Invalidation) -> Self {
        // The windows start past the function's environment window (`base_region::function::inline_base`):
        // the room covers that reach as well as the activations a leave inside a callee pushes.
        let recovery = LaneRecoverySpace::inlined(&inlined).reaching(window_width(&inlined, caller_registers));
        Self { inlined, direct, records, rebound: Default::default(), held: Box::default(), caller_registers, recovery, invalidation }
    }

    /// Whether its code calls a callee by its entry (a b-jit direct row, whose record it owns, or a
    /// Cranelift region's direct call): it runs on the lane frame's payload (`region_lend`), whose
    /// lane stack words the callee's window and frame are pushed through.
    pub(crate) fn enters_callees(&self) -> bool {
        !self.records.is_empty() || !self.direct.is_empty()
    }

    /// The room a leave inside an inlined callee needs, which a run is admitted by.
    pub(crate) fn recovery_space(&self) -> LaneRecoverySpace {
        self.recovery
    }

    /// The registers past the function's own that its inlined callees occupy, in the activation's
    /// window: none where it inlines nothing.
    pub(crate) fn window(&self) -> usize {
        window_width(&self.inlined, self.caller_registers)
    }

    pub(crate) fn caller_registers(&self) -> u16 {
        self.caller_registers
    }

    /// The callees its calls name by identity, which a collection that frees one retires it over
    /// while its code compiles.
    pub(crate) fn identities(&self) -> Box<[Value]> {
        self.inlined.iter().filter_map(|call| call.target.identity()).chain(self.direct.iter().map(|callee| callee.closure.callee)).collect()
    }
}
