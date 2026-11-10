//! A Wasm function's selective tier and its module's lifecycle, as `Tier`s. A function's artifact
//! assumes nothing, so it is the generic body; the module's artifact, the whole program's, is
//! the program's `compiled`, and its tier records only how its compile went.

use crate::vm::compiled::CompiledProgram;
use crate::vm::compiled::tier_policy::{TIERS, Tiers};
use crate::vm::tier_state::{Build, Controller, Event, Past, Refusal, Tier, Verdict};
use std::rc::Rc;

/// The controller of a function's selective artifact. Its warm-up is `TIERS.wasm.promotion_entries`
/// entries, doubled for each compile the code cache refused.
pub(in crate::vm) struct WasmController;

impl Controller for WasmController {
    type Pending = ();
    type Artifact = Rc<CompiledProgram>;
    type Feedback = ();
    type Stats = ();
    type Leave = ();

    fn warmup(policy: &Tiers, past: &Past<()>) -> u32 {
        u32::from(policy.wasm.promotion_entries).checked_shl(u32::from(past.refused)).unwrap_or(u32::MAX)
    }

    fn judge(_: &Tiers, _: &mut (), _: &Past<()>, _: ()) -> Verdict<()> {
        Verdict::Keep
    }
}

pub(in crate::vm) type WasmTier = Tier<WasmController>;

impl WasmTier {
    /// Counts an interpreter entry, and whether it claims the function's one compile. A lane
    /// handoff (`lane_handoff`) that reaches the threshold hands the entry to the general
    /// dispatcher, whose own entry claims it, and counts nothing.
    pub(in crate::vm) fn note_entry(&mut self, lane_handoff: bool) -> bool {
        if self.count(false, &TIERS) {
            return false;
        }
        if self.is_claimed() {
            return false;
        }
        lane_handoff || {
            self.apply(Event::Entry { favourable: false }, &TIERS);
            true
        }
    }

    /// Credits an entry's worth of loop work, short of the threshold: the entry that reaches it
    /// claims the compile.
    pub(in crate::vm) fn note_backedges(&mut self) {
        self.count(false, &TIERS);
    }

    /// Keeps `compiled` as the function's body: an artifact assumes nothing, so it is generic.
    pub(in crate::vm) fn install(&mut self, compiled: Rc<CompiledProgram>) {
        self.apply(Event::GENERALIZE, &TIERS);
        self.apply(Event::Compiled(compiled), &TIERS);
    }

    /// Records that the claimed compile produced no body: the code generator failed it for good,
    /// or the code cache refused it, and the function warms again, longer.
    pub(in crate::vm) fn not_installed(&mut self, refused: bool) {
        self.apply(if refused { Event::REFUSED } else { Event::Failed(Refusal::Codegen) }, &TIERS);
    }

    pub(in crate::vm) fn compiled(&self) -> Option<&Rc<CompiledProgram>> {
        self.artifact()
    }

    /// Whether the function's one compile was claimed: it is under way, done or failed.
    pub(in crate::vm) fn is_claimed(&self) -> bool {
        !matches!(self, Self::Cold | Self::Warming { .. })
    }

    /// Takes the body out of the tier, which a whole-module artifact supersedes.
    pub(in crate::vm) fn take_compiled(&mut self) -> Option<Rc<CompiledProgram>> {
        let compiled = self.compiled().cloned()?;
        self.apply(Event::INVALIDATED, &TIERS);
        Some(compiled)
    }
}

/// The controller of a module's whole-program compile. Its owner prices the warm-up
/// (`WasmModuleCompile::due`): counting never ends it.
pub(in crate::vm) struct ModuleController;

impl Controller for ModuleController {
    type Pending = ();
    type Artifact = ();
    type Feedback = ();
    type Stats = ();
    type Leave = ();

    fn warmup(_: &Tiers, _: &Past<()>) -> u32 {
        u32::MAX
    }

    fn judge(_: &Tiers, _: &mut (), _: &Past<()>, _: ()) -> Verdict<()> {
        Verdict::Keep
    }
}

/// A Wasm module's lifecycle under `--jit`. It starts with no eager artifact (`Cold`, `Warming`
/// its work, `Queued` once that is due), so a short use pays for the few functions it runs hot;
/// once the policy says it runs long, the whole module compiles at once (`Compiling`), as eager
/// tiering would have from the start, and the compile ends `Generic` or `GenericInterpreted`.
#[derive(Default)]
pub(in crate::vm) struct WasmModule {
    tier: Tier<ModuleController>,
    /// When its first work came, for `WasmModuleCompile::after`.
    first: Option<std::time::Instant>,
}

impl WasmModule {
    /// Whether the module still runs its hot functions one by one: its whole compile is not
    /// claimed.
    pub(in crate::vm) fn is_selective(&self) -> bool {
        matches!(self.tier, Tier::Cold | Tier::Warming { .. } | Tier::Queued { .. })
    }

    fn work(&self) -> u32 {
        match self.tier {
            Tier::Warming { entries, .. } => entries,
            _ => 0,
        }
    }

    /// Counts one entry of a selective module; whether its whole compile is due. A passed time
    /// bound counts as the work bound from then on, so every later entry, whichever path
    /// observes it, finds the compile due.
    pub(in crate::vm) fn note_work(&mut self, policy: super::WasmModuleCompile) -> bool {
        match self.tier {
            Tier::Queued { .. } => return true,
            Tier::Cold | Tier::Warming { .. } => {}
            _ => return false,
        }
        let first = *self.first.get_or_insert_with(std::time::Instant::now);
        let before = self.work();
        self.tier.apply(Event::Entry { favourable: false }, &TIERS);
        let work = self.work();
        let clocked = work / TIERS.wasm.module_clock_interval != before / TIERS.wasm.module_clock_interval
            && policy.due(0, first.elapsed());
        let due = clocked || policy.due(work, std::time::Duration::ZERO);
        if due {
            self.tier.apply(Event::DUE, &TIERS);
        }
        due
    }

    /// Claims the module's one whole compile; whether it was still selective to claim it.
    pub(in crate::vm) fn claim(&mut self) -> bool {
        let selective = self.is_selective();
        if selective {
            self.first = None;
            self.tier.apply(Event::Started(Build::new(Box::new(()), 0)), &TIERS);
        }
        selective
    }

    /// Records how the whole compile came out (`compiled`), claiming it first where an eager
    /// compile did not.
    pub(in crate::vm) fn finish(&mut self, compiled: bool) {
        self.claim();
        if compiled {
            self.tier.apply(Event::GENERALIZE, &TIERS);
            self.tier.apply(Event::Compiled(()), &TIERS);
        } else {
            self.tier.apply(Event::Failed(Refusal::Codegen), &TIERS);
        }
    }
}
