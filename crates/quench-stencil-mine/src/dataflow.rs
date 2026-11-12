//! Block-local dataflow: which earlier instruction produced each register an instruction
//! reads, and which results are single-use temporaries (read once, and by no other block).
use quench_runtime::FlowInstruction;
use std::collections::{HashMap, HashSet};

pub struct Flow {
    /// For each instruction, the in-block producers of its operands, ascending, once each.
    pub links_in: Vec<Vec<usize>>,
    /// The instruction writes one register, one in-block instruction reads it and no other
    /// block reads it before writing it.
    pub single_use: Vec<bool>,
}

/// Registers read before any write in each block, with how many blocks read each.
pub struct Exposure {
    per_block: Vec<HashSet<u16>>,
    blocks_reading: HashMap<u16, u32>,
}

impl Exposure {
    pub fn of(instructions: &[FlowInstruction], blocks: &[std::ops::Range<usize>]) -> Self {
        let per_block: Vec<HashSet<u16>> = blocks
            .iter()
            .map(|block| exposed_reads(&instructions[block.clone()]))
            .collect();
        let mut blocks_reading = HashMap::new();
        for register in per_block.iter().flatten() {
            *blocks_reading.entry(*register).or_insert(0) += 1;
        }
        Self { per_block, blocks_reading }
    }

    /// Whether a block other than `block` reads `register` before writing it.
    pub fn escapes(&self, block: usize, register: u16) -> bool {
        let own = u32::from(self.per_block[block].contains(&register));
        self.blocks_reading.get(&register).copied().unwrap_or(0) > own
    }
}

fn exposed_reads(block: &[FlowInstruction]) -> HashSet<u16> {
    let (mut written, mut exposed) = (HashSet::new(), HashSet::new());
    for instruction in block {
        exposed.extend(instruction.uses.iter().filter(|register| !written.contains(*register)));
        written.extend(instruction.defs.iter().copied());
    }
    exposed
}

pub fn analyze(block: &[FlowInstruction], escapes: impl Fn(u16) -> bool) -> Flow {
    let mut last_def: HashMap<u16, usize> = HashMap::new();
    let mut links_in = vec![Vec::new(); block.len()];
    let mut readers = vec![0usize; block.len()];
    for (index, instruction) in block.iter().enumerate() {
        for register in &instruction.uses {
            if let Some(&producer) = last_def.get(register) {
                readers[producer] += 1;
                if !links_in[index].contains(&producer) {
                    links_in[index].push(producer);
                }
            }
        }
        links_in[index].sort_unstable();
        for &register in &instruction.defs {
            last_def.insert(register, index);
        }
    }
    let single_use = block
        .iter()
        .enumerate()
        .map(|(index, instruction)| {
            matches!(instruction.defs.as_slice(), [register] if readers[index] == 1 && !escapes(*register))
        })
        .collect();
    Flow { links_in, single_use }
}
