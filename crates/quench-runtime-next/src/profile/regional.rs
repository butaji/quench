use super::Profile;
use crate::bytecode::{DispatchClass, Op, ResidualProgram};

impl Profile {
    #[inline(always)]
    pub(super) fn site(&mut self, function: u32, pc: usize) {
        let function = function as usize;
        if self.site_counts.len() <= function {
            self.site_counts.resize_with(function + 1, Vec::new);
        }
        let sites = &mut self.site_counts[function];
        if sites.len() <= pc {
            sites.resize(pc + 1, 0);
        }
        sites[pc] += 1;
    }

    #[inline(always)]
    pub(crate) fn regional_binary(&mut self, function: u32, pc: u32, fast: bool) {
        self.regional_binary_inputs
            .entry((function, pc))
            .or_default()[usize::from(fast)] += 1;
    }
}

#[derive(Clone, Copy)]
struct Region {
    function: usize,
    start: usize,
    end: usize,
    executions: u64,
    binaries: [u64; 2],
}

pub(super) fn report(profile: &Profile, program: &ResidualProgram) {
    let mut regions = Vec::new();
    let mut general = [0u64; 2];
    let mut numeric_dispatch_binaries = 0u64;
    for (id, function) in program.functions.iter().enumerate() {
        let counts = profile
            .site_counts
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if function.dispatch == DispatchClass::Numeric {
            numeric_dispatch_binaries += binary_count(function, counts);
            continue;
        }
        for (&(function_id, _), counts) in &profile.regional_binary_inputs {
            if function_id as usize == id {
                general[0] += counts[0];
                general[1] += counts[1];
            }
        }
        for (pc, instruction) in function.code.iter().enumerate() {
            if is_backward_edge(instruction.op(), instruction.imm() as usize, pc) {
                regions.push(region(profile, id, instruction.imm() as usize, pc, counts));
            }
        }
    }
    regions.sort_unstable_by_key(|region| std::cmp::Reverse(region.binaries.iter().sum::<u64>()));
    eprint!(
        "{{\"kind\":\"rqj-regional-numeric\",\"general_binary\":{{\"fast_integer\":{},\"fallback\":{},\"total\":{}}},\"numeric_dispatch_binary\":{numeric_dispatch_binaries},\"backward_regions\":[",
        general[1],
        general[0],
        general.iter().sum::<u64>(),
    );
    for (position, region) in regions.iter().take(32).enumerate() {
        if position != 0 {
            eprint!(",");
        }
        let binaries = region.binaries.iter().sum::<u64>();
        eprint!(
            "{{\"function\":{},\"start\":{},\"end\":{},\"instructions\":{},\"executions\":{},\"binary_executions\":{binaries},\"fast_integer\":{},\"fallback\":{}}}",
            region.function,
            region.start,
            region.end,
            region.end - region.start + 1,
            region.executions,
            region.binaries[1],
            region.binaries[0],
        );
    }
    eprintln!("]}}");
}

pub(super) fn report_functions(profile: &Profile, program: &ResidualProgram) {
    eprint!(",\"functions\":[");
    for (id, count) in profile.functions.iter().enumerate() {
        if id != 0 {
            eprint!(",");
        }
        let function = &program.functions[id];
        let name = function
            .name
            .map(|atom| program.atoms[atom as usize].as_ref())
            .unwrap_or("<script>");
        let max_live = (function.register_root_offset != u32::MAX).then(|| {
            let start = function.register_root_offset as usize;
            program.register_roots[start..start + function.code.len() + 1]
                .iter()
                .map(|mask| mask.count_ones())
                .max()
                .unwrap_or(0)
        });
        eprint!(
            "{{\"id\":{id},\"name\":\"{name}\",\"count\":{count},\"instructions\":{},\"registers\":{},\"max_live\":{},\"dispatch\":\"{:?}\"}}",
            function.code.len(),
            function.registers,
            max_live
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".into()),
            function.dispatch,
        );
    }
    eprintln!("]}}");
}

fn region(profile: &Profile, function: usize, start: usize, end: usize, counts: &[u64]) -> Region {
    let mut binaries = [0; 2];
    for (&(id, pc), values) in &profile.regional_binary_inputs {
        if id as usize == function && (start..=end).contains(&(pc as usize)) {
            binaries[0] += values[0];
            binaries[1] += values[1];
        }
    }
    Region {
        function,
        start,
        end,
        executions: counts.get(start..=end).unwrap_or(&[]).iter().sum(),
        binaries,
    }
}

fn binary_count(function: &crate::bytecode::Function, counts: &[u64]) -> u64 {
    function
        .code
        .iter()
        .enumerate()
        .filter(|(_, instruction)| {
            matches!(
                instruction.op(),
                Op::Binary | Op::NumericAdd | Op::NumericMultiply
            )
        })
        .map(|(pc, _)| counts.get(pc).copied().unwrap_or(0))
        .sum()
}

fn is_backward_edge(op: Op, target: usize, pc: usize) -> bool {
    matches!(op, Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse) && target <= pc
}

impl Profile {
    #[inline(always)]
    pub fn function(&mut self, id: usize) {
        if self.functions.len() <= id {
            self.functions.resize(id + 1, 0);
        }
        self.functions[id] += 1;
    }
}
