use super::Profile;
use crate::bytecode::ResidualProgram;
use rustc_hash::FxHashSet;

impl Profile {
    pub fn gc_frame(&mut self, program: u32, function: u32, pc: u32, active: bool) {
        *self
            .gc_frame_pcs
            .entry((program, function, pc, active))
            .or_default() += 1;
    }
}

pub(super) fn report(profile: &Profile, program_id: u32, program: &ResidualProgram) {
    let maps = (0..program.functions.len() as u32)
        .filter_map(|function| program.register_root_masks(function))
        .collect::<Vec<_>>();
    let entries = maps.iter().map(|masks| masks.len()).sum::<usize>();
    let distinct: FxHashSet<_> = maps.iter().flat_map(|masks| masks.iter()).collect();
    let runs = maps
        .iter()
        .map(|masks| masks.iter().zip(masks.iter().skip(1)).filter(|(before, after)| before != after).count() + 1)
        .sum::<usize>();
    let observed_masks: FxHashSet<_> = profile
        .gc_frame_pcs
        .keys()
        .filter_map(|&(site_program, function, pc, _)| {
            (site_program == program_id)
                .then(|| root_mask(program, function, pc))
                .flatten()
        })
        .collect();
    let observations = profile
        .gc_frame_pcs
        .iter()
        .filter(|((site_program, _, _, _), _)| *site_program == program_id)
        .map(|(_, count)| *count)
        .sum::<u64>();
    let active = profile
        .gc_frame_pcs
        .iter()
        .filter(|((site_program, _, _, active), _)| *site_program == program_id && *active)
        .map(|(_, count)| *count)
        .sum::<u64>();
    eprint!(
        ",\"root_maps\":{{\"full_entries\":{},\"full_bytes\":{},\"distinct_masks\":{},\"mask_runs\":{runs},\"observed_sites\":{},\"observed_masks\":{},\"frame_observations\":{observations},\"active_observations\":{active},\"suspended_observations\":{}}}",
        entries,
        entries * size_of::<crate::compile::liveness::RegisterSet>(),
        distinct.len(),
        profile
            .gc_frame_pcs
            .keys()
            .filter(|(site_program, _, _, _)| *site_program == program_id)
            .count(),
        observed_masks.len(),
        observations - active,
    );
}

fn root_mask(program: &ResidualProgram, function: u32, pc: u32) -> Option<crate::compile::liveness::RegisterSet> {
    program.register_root_mask(function, pc as usize)
}
