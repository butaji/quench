//! Windowed liveness: a function wider than one register set is solved one window at a time.
//!
//! Liveness of each register is independent of the others, so the same transfer table shifted
//! down by a window's width yields that window's live sets exactly. The windows are stored
//! side by side per pc (`RegisterMasks::Windowed`).

use super::{add_suspended_exception_roots, analyze_graph, register_bit, MethodArguments, RegisterMasks, RegisterSet};
use crate::bytecode::{FieldSite, Function, Successors, Superinstruction};

/// The windows one body may carry: 8 * 512 = 4096 registers. Wider bodies keep one
/// conservative all-live map instead, bounding the tables a huge register file would take.
pub(super) const MAX_WINDOWS: usize = 8;

/// The precise masks of a body past one set's width, or `None` past [`MAX_WINDOWS`].
pub(super) fn masked(
    function: &Function,
    methods: &(impl MethodArguments + ?Sized),
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<RegisterMasks> {
    let width = RegisterSet::BITS as usize;
    let windows = usize::from(function.registers).div_ceil(width);
    if windows > MAX_WINDOWS { return None; }
    let transfers = super::all_transfers(function, methods, fields, superinstructions);
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
