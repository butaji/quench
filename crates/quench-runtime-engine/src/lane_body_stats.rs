//! The compile-time census of a lane body's lowering: the instructions a body compile hands back
//! to the lane (`QUENCH_LANE_UNLOWERED`), by the function they belong to.

/// Instructions a body compile left to the lane (`QUENCH_LANE_UNLOWERED`), by opcode.
static UNLOWERED: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

thread_local! { static LOWERING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

/// The function whose body the compile in progress lowers, which `unlowered` names.
pub(crate) fn lowering_function(function: u32) { LOWERING.with(|cell| cell.set(function)); }

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
