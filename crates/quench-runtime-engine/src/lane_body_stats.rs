//! A lane view's lane-body counters, kept under `QUENCH_TRACE=js-jit`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::tier_events::Event;

pub(crate) use quench_runtime::vm::engine_types::LaneBodyStats;

#[quench_macros::ext(LaneBodyStatsOps1)]
impl LaneBodyStats {
    pub(crate) fn compiled(&self, function: u32, frames: &[crate::compiled::InlinedCall]) {
        self.function.set(function);
        self.compiles.set(self.compiles.get() + 1);
        self.frames.set(frames.len() as u32);
        *self.shape.borrow_mut() = frames.iter().enumerate().map(|(index, frame)| {
            let parent = frame.parent.map_or(String::from("root"), |parent| parent.to_string());
            format!("{index}:f{}<-{parent}@{}{}", frame.target.function, frame.call, if frame.alternative.is_some() { "+" } else { "" })
        }).collect::<Vec<_>>().join(" ");
    }

    /// The runs that ended the activation (a return or an exception) and the runs that left it
    /// to the lane, which a test reads to see that no exception made a frame. A leave into a
    /// warming callee is the callee's warm-up, not the body's leave.
    #[cfg(test)]
    pub(crate) fn ended_runs(&self) -> (u32, u32) {
        let left: u32 = [Event::LeftAt, Event::LeftInlined, Event::LeftInCallee].map(|end| self.ends[end.end_index()].get()).iter().sum();
        (self.ends[Event::Completed.end_index()].get(), left - self.warming_leaves.get())
    }

    /// `frames`, the body's inlined calls, name the function a leave inside one was in.
    pub(crate) fn ended(&self, exit: crate::compiled::LaneExit, exits: &crate::compiled::ExitTable, frames: &[crate::compiled::InlinedCall], line: u64) {
        let index = Event::ended(exit).end_index();
        self.ends[index].set(self.ends[index].get() + 1);
        // Prefixed by the compile that left there, the first being `c1`.
        let compile = self.compiles.get();
        // Each with the function it left in and its PC there, which the report names the
        // instruction of: `#function@pc`.
        // A guard's leave names the lowering line that made it (`JsStepDispatch::exit_line`).
        let compile = if line == 0 { format!("c{compile}") } else { format!("c{compile} L{line}") };
        let at = match exit {
            crate::compiled::LaneExit::At(pc) => format!("{compile} {pc}{}#{}@{pc}", exits.describe(exit), self.function.get()),
            crate::compiled::LaneExit::Entry => format!("{compile} entry#{}", self.function.get()),
            crate::compiled::LaneExit::Inlined { frame, pc } => match frames.get(frame) {
                Some(call) => format!("{compile} {frame}:{pc}{}#{}@{pc}", exits.describe(exit), call.target.function),
                None => format!("{compile} {frame}:{pc}"),
            },
            _ => return,
        };
        let mut leaves = self.leaves.borrow_mut();
        match leaves.iter_mut().find(|(leave, _)| *leave == at) {
            Some((_, count)) => *count += 1,
            None => leaves.push((at, 1)),
        }
    }
}

/// Why a lane body's call or construction helper answered `LANE_CALL_MISSED`, which leaves the
/// body at the call: one variant per refusal site in `body_call_helper`, `body_construct_helper` and
/// `body_native_call_in_lane`, named `site:reason` in the diagnostics report (`miss_report`).
pub(crate) use quench_runtime::vm::engine_types::{Miss, MISS_NAMES};



/// Counts a miss for `reason` in the VM's tier events and answers the helper's miss.
#[cold]
pub(crate) fn miss(vm: &quench_runtime::vm::Vm, reason: Miss) -> u64 {
    vm.programs.tier_events().record_miss(reason);
    decline(MISS_NAMES[reason as usize]);
    crate::compiled::LANE_CALL_MISSED
}

/// The report line of the misses, `QUENCH_LANE_MISSES {...}`, most frequent first.
pub(crate) fn miss_report(events: &quench_runtime::vm::tier_events::TierEvents) -> String {
    let mut counts: Vec<_> = MISS_NAMES.iter().zip(events.miss_counts()).filter(|(_, count)| *count != 0).collect();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let fields = counts.iter().map(|(name, count)| format!("\"{name}\":{count}")).collect::<Vec<_>>().join(",");
    format!("QUENCH_LANE_MISSES {{{fields}}}")
}

/// Instructions a body compile left to the lane (`QUENCH_LANE_UNLOWERED`), by opcode.
static UNLOWERED: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

/// Counts one instruction of `op` that a lane body compile hands back to the lane.
pub(crate) fn unlowered(op: impl std::fmt::Debug) {
    let name = format!("f{} {op:?}", LOWERING.with(std::cell::Cell::get));
    let mut counts = UNLOWERED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match counts.iter_mut().find(|(known, _)| *known == name) {
        Some((_, count)) => *count += 1,
        None => counts.push((name, 1)),
    }
}

pub(crate) fn unlowered_report() -> String {
    let mut counts = UNLOWERED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let fields = counts.iter().map(|(name, count)| format!("\"{name}\":{count}")).collect::<Vec<_>>().join(",");
    format!("QUENCH_LANE_UNLOWERED {{{fields}}}")
}

thread_local! { static DECLINE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") }; }

/// Names why the decision in progress declines a body, which the view's `LaneBody::Declined`
/// keeps for the diagnostics report (`QUENCH_LANE_STATE`).
pub(crate) fn decline(reason: &'static str) { DECLINE.with(|cell| if cell.get().is_empty() { cell.set(reason) }); }

/// The last `decline` reason, cleared.
pub(crate) fn take_decline() -> &'static str { DECLINE.with(|cell| cell.replace("")) }

thread_local! { static LOWERING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

/// The function whose body the compile in progress lowers, which `unlowered` names.
pub(crate) fn lowering_function(function: u32) { LOWERING.with(|cell| cell.set(function)); }
