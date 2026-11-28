//! What a lane body's plan reads of its unit alone: its leaves, direct sites, entry and the
//! facts about its callees.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::tier_policy::PredictedTier;
use std::collections::BTreeMap;

/// The calls a unit still makes (none it inlines) whose callees have bodies. Each candidate
/// owns one contiguous stable slice, which the code names by address. A site with several
/// callees binds every candidate the unit does not inline, in the order the candidates list them
/// (a chain of identity tests takes them, `Lowering::polymorphic_call`); a construction binds only
/// the first, and a site whose candidates are all inlined binds the first of them.
pub(crate) fn unit_direct_calls(unit: &lane_unit::Unit, direct: &[DirectCandidates])
    -> (Box<[DirectCallee]>, BTreeMap<usize, Vec<*const DirectCallee>>) {
    let chosen: Vec<(usize, Vec<DirectCallee>)> = unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Call) || quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Construct))
        .filter_map(|(pc, instruction)| match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => {
                let inlined = |callee: &DirectCallee| unit.frames.iter()
                    .any(|frame| frame.parent.is_none() && frame.call == origin && frame.target.function == callee.target.function);
                let candidates = direct.get(origin as usize)?;
                let mut bound: Vec<DirectCallee> = candidates.iter().filter(|callee| !inlined(callee)).cloned().collect();
                if bound.is_empty() { bound.extend(candidates.first().cloned()); }
                if quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Construct) { bound.truncate(1); }
                (!bound.is_empty()).then_some((pc, bound))
            }
            lane_unit::Origin::Inlined { .. } => None,
        })
        .collect();
    let mut owned: Vec<DirectCallee> = Vec::new();
    let mut spans: Vec<(usize, std::ops::Range<usize>)> = Vec::with_capacity(chosen.len());
    for (pc, callees) in chosen {
        let start = owned.len();
        owned.extend(callees);
        spans.push((pc, start..owned.len()));
    }
    let direct: Box<[DirectCallee]> = owned.into_boxed_slice();
    // The pointers are taken once the slice is boxed, so they name its final elements.
    let sites = spans.into_iter().map(|(pc, span)| (pc, direct[span].iter().map(std::ptr::from_ref).collect())).collect();
    (direct, sites)
}

/// The unit PC a running loop enters at: its `header`'s own instruction; `Some(None)` for a
/// body that starts at its first, `None` where the unit has no such instruction.
pub(crate) fn entry_pc(unit: &lane_unit::Unit, header: Option<u32>) -> Option<Option<usize>> {
    let Some(header) = header else { return Some(None) };
    unit.origins.iter().position(|origin| *origin == lane_unit::Origin::Caller(header)).map(Some)
}

/// What a compile of a plan comes to, as one number: its unit, the leaves and entry it was
/// planned from, its field proofs and its direct callees. Equal keys make equal code.
pub(crate) fn plan_key(unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug,
    direct: usize, entry: Option<u32>) -> u64 {
    use std::fmt::Write;
    // The instructions, most of the key, fold as their words; the rest as its debug text,
    // written into the hash without a string between.
    let mut key = Fnv::default();
    for instruction in &unit.instructions {
        let (word, imm) = instruction.words();
        key.word(word);
        key.word(u64::from(imm));
    }
    let _ = write!(key, "{:?}{fields:?}{entry:?}{direct}", unit.frames);
    key.0
}

/// A 64-bit FNV-1a hash.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self { Self(0xcbf2_9ce4_8422_2325) }
}

impl Fnv {
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn word(&mut self, word: u64) {
        self.0 = (self.0 ^ word).wrapping_mul(Self::PRIME);
    }
}

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 = text.bytes().fold(self.0, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(Self::PRIME));
        Ok(())
    }
}

/// The predicted tier of `function`'s lane body (design decision 4.9): the one input of every lane tier pick, read from
/// the program's table (`ProgramStore::predicted_tiers`). An AOT program's predicted tier is Ultra for every function
/// (`tier_policy::body_tier`). A program not running has no table and no body.
pub(crate) fn body_pick(aot: bool, predicted: &[PredictedTier], function: u32) -> PredictedTier {
    let tier = predicted.get(function as usize).copied().unwrap_or(PredictedTier::Interpreter);
    quench_runtime::vm::tier_policy::body_tier(aot, tier)
}

/// Whether a field site guards its shapes and exits on a miss (speculates), rather than reading in place through the
/// VM's cache (`lane_region::field_probe`): the solver proves its receiver's class set, of at most `POLYMORPHISM`
/// classes (design decision 4.9, a static predicate of the facts; no run count, no posterior).
pub(crate) fn speculates(classes: quench_runtime::typed_ir::Classes) -> bool {
    matches!(classes, quench_runtime::typed_ir::Classes::Set { len, proven: true, .. }
        if usize::from(len) <= quench_runtime::vm::tier_policy::POLYMORPHISM)
}

/// The calls a plan keeps are its caller's facts and the demand for their callees' bodies: a
/// declined short callee warms again while the caller publishes at once, calling it through
/// its entry cell. A callee the plan inlines has no call of it to bind.
pub(crate) fn bind_callees(direct: &[DirectCallee], caller: &crate::dispatch_fast::LaneView) {
    for callee in direct {
        // SAFETY: a resolved target's view outlives its program.
        if let Some(view) = unsafe { callee.target.view.as_ref() } { view.note_caller(caller); view.demand_body(); }
    }
}

/// What a lane compile reports under `QUENCH_TRACE=js-jit` (`QUENCH_LANE_COMPILE`).
pub(crate) struct Census {
    pub(crate) function: u32, pub(crate) osr: bool, pub(crate) unit: usize, pub(crate) ir: usize, pub(crate) key: u64,
    pub(crate) direct: usize, pub(crate) closed: bool,
}

impl Census {
    pub(crate) fn report(&self, bytes: usize) -> String {
        let Self { function, osr, unit, ir, key, direct, closed } = self;
        format!("QUENCH_LANE_COMPILE {{\"function\":{function},\"osr\":{osr},\"unit\":{unit},\"ir\":{ir},\"direct\":{direct},\"key\":\"{key:016x}\",\"closed\":{closed},\"bytes\":{bytes}}}")
    }
}

/// Whether every call of a unit that has run is direct (`LaneMethod::closed`). A call that
/// recorded no callee has not run; where it first does it leaves, as any helper call does
/// under a direct caller's frames.
pub(crate) fn unit_closed(unit: &lane_unit::Unit, function: u32, direct: impl Fn(usize) -> bool,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>) -> bool {
    unit.instructions.iter().enumerate().all(|(pc, instruction)| !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call)
        || direct(pc)
        || match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => targets(function, origin),
            lane_unit::Origin::Inlined { frame, pc } => targets(unit.frames[frame].target.function, pc),
        }.is_empty())
}
