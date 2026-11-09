//! Windowed liveness: a function wider than one register set is solved one window at a time.
//!
//! Liveness of each register is independent of the others, so the same transfer table shifted
//! down by a window's width yields that window's live sets exactly. The windows are stored
//! side by side per pc (`RegisterMasks::Windowed`).

use super::{add_suspended_exception_roots, analyze_graph, register_bit, MethodArguments, RegisterMasks, RegisterSet};
use crate::bytecode::{FieldSite, Function, Successors, Superinstruction};

/// The bytes one body's windowed masks may take. A body whose windows would exceed it keeps
/// one conservative all-live map instead: a documented memory bound with safe exhaustion,
/// rather than an arbitrary register count.
pub(crate) const MAX_MASK_BYTES: usize = 4 << 20;

/// Whether `windows` sets for `length` instructions exceed [`MAX_MASK_BYTES`].
fn over_budget(windows: usize, length: usize) -> bool {
    windows.checked_mul(length + 1)
        .and_then(|sets| sets.checked_mul(std::mem::size_of::<RegisterSet>()))
        .is_none_or(|bytes| bytes > MAX_MASK_BYTES)
}

/// Whether `function`'s root map is the conservative all-live one: its windows would exceed
/// the byte budget.
#[cfg_attr(not(feature = "aot"), allow(dead_code))]
pub(crate) fn is_conservative(function: &Function, length: usize) -> bool {
    over_budget(usize::from(function.registers).div_ceil(RegisterSet::BITS as usize), length)
}

/// The precise masks of a body past one set's width, or `None` past [`MAX_WINDOWS`].
pub(super) fn masked(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<RegisterMasks> {
    let width = RegisterSet::BITS as usize;
    let windows = usize::from(function.registers).div_ceil(width);
    let transfers = super::all_transfers(function, methods, fields, superinstructions);
    if over_budget(windows, transfers.len()) { return None; }
    let successors = Successors::of_code(function);
    let parameters = if function.has_restricted_legacy_caller_access() { RegisterSet::default() } else {
        function.local_registers.iter().filter(|entry| entry.local < function.params)
            .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register))
    };
    let mut masks = vec![RegisterSet::default(); (transfers.len() + 1) * windows];
    for window in 0..windows {
        let shift = (window * width) as u32;
        let shifted: Vec<_> = transfers.iter().map(|&(read, written)| (read >> shift, written >> shift)).collect();
        let observable = parameters >> shift;
        let mut live = analyze_graph(&shifted, successors.starts(), successors.targets(), |_| observable);
        add_suspended_exception_roots(function, &mut live);
        for (pc, set) in live.into_iter().enumerate() {
            masks[pc * windows + window] = set;
        }
    }
    Some(RegisterMasks::Windowed { windows: windows as u16, masks: masks.into_boxed_slice() })
}

#[cfg(test)]
mod tests {
    use super::{over_budget, RegisterSet, MAX_MASK_BYTES};

    #[test]
    fn the_window_budget_admits_past_4096_registers_and_exhausts_safely() {
        // 4097 registers span 9 windows; a small body is far inside the budget.
        assert!(!over_budget(9, 10));
        // 9 windows of a 60K-instruction body exceed 4 MiB of sets.
        assert!(over_budget(9, 60_000));
        assert!(!over_budget(9, 7_000));
        assert_eq!(MAX_MASK_BYTES, 4 << 20);
        assert_eq!(std::mem::size_of::<RegisterSet>(), 64);
    }
}
