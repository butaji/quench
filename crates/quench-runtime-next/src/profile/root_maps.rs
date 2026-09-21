use super::Profile;
use crate::bytecode::ResidualProgram;
use rustc_hash::FxHashSet;

impl Profile {
    pub fn gc_frame(&mut self, function: u32, pc: u32, active: bool) {
        *self.gc_frame_pcs.entry((function, pc, active)).or_default() += 1;
    }
}

pub(super) fn report(profile: &Profile, program: &ResidualProgram) {
    let distinct: FxHashSet<_> = program.register_roots.iter().copied().collect();
    let runs = program
        .functions
        .iter()
        .filter(|function| function.register_root_offset != u32::MAX)
        .map(|function| function_runs(function, program))
        .sum::<usize>();
    let observed_masks: FxHashSet<_> = profile
        .gc_frame_pcs
        .keys()
        .filter_map(|&(function, pc, _)| root_mask(program, function, pc))
        .collect();
    let observations = profile.gc_frame_pcs.values().sum::<u64>();
    let active = profile
        .gc_frame_pcs
        .iter()
        .filter(|((_, _, active), _)| *active)
        .map(|(_, count)| *count)
        .sum::<u64>();
    eprint!(
        ",\"root_maps\":{{\"full_entries\":{},\"full_bytes\":{},\"distinct_masks\":{},\"mask_runs\":{runs},\"observed_sites\":{},\"observed_masks\":{},\"frame_observations\":{observations},\"active_observations\":{active},\"suspended_observations\":{}}}",
        program.register_roots.len(),
        program.register_roots.len() * size_of::<u64>(),
        distinct.len(),
        profile.gc_frame_pcs.len(),
        observed_masks.len(),
        observations - active,
    );
}

fn function_runs(function: &crate::bytecode::Function, program: &ResidualProgram) -> usize {
    let start = function.register_root_offset as usize;
    program.register_roots[start..start + function.code.len() + 1]
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count()
        + 1
}

fn root_mask(program: &ResidualProgram, function: u32, pc: u32) -> Option<u64> {
    let function = program.functions.get(function as usize)?;
    (function.register_root_offset != u32::MAX)
        .then(|| program.register_roots[function.register_root_offset as usize + pc as usize])
}
