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
    let distinct: FxHashSet<_> = program
        .functions
        .iter()
        .filter(|function| function.register_root_offset != u32::MAX)
        .flat_map(|function| function_masks(function, program))
        .collect();
    let runs = program
        .functions
        .iter()
        .filter(|function| function.register_root_offset != u32::MAX)
        .map(|function| function_runs(function, program))
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
        program
            .functions
            .iter()
            .filter(|function| function.register_root_offset != u32::MAX)
            .map(|function| function.code.len() + 1)
            .sum::<usize>(),
        program.register_roots.bytes().len(),
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

fn function_masks<'a>(
    function: &crate::bytecode::Function,
    program: &'a ResidualProgram,
) -> impl Iterator<Item = u64> + Clone + 'a {
    program
        .register_roots
        .masks(function.register_root_offset, function.registers, function.code.len() + 1)
        .into_iter()
        .flatten()
}

fn function_runs(function: &crate::bytecode::Function, program: &ResidualProgram) -> usize {
    let masks = function_masks(function, program);
    masks
        .clone()
        .zip(masks.skip(1))
        .filter(|(first, second)| first != second)
        .count()
        + 1
}

fn root_mask(program: &ResidualProgram, function: u32, pc: u32) -> Option<u64> {
    let function = program.functions.get(function as usize)?;
    (function.register_root_offset != u32::MAX)
        .then(|| {
            program.register_roots.mask(function.register_root_offset, function.registers, pc as usize)
        })
        .flatten()
}
