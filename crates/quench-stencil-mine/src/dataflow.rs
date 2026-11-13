//! Block-local dataflow: which earlier instruction produced each register an instruction
//! reads, and which results are single-use temporaries (read once, and by no other block).
use crate::bits::Bits;
use quench_runtime::FlowInstruction;

pub struct Flow {
    /// For each instruction, the in-block producers of its operands, ascending, once each.
    pub links_in: Vec<Vec<usize>>,
    /// The instruction writes one register, one in-block instruction reads it and no other
    /// block reads it before writing it.
    pub single_use: Bits,
}

/// One past the highest register the instructions read or write.
fn register_width(instructions: &[FlowInstruction]) -> usize {
    let registers = instructions.iter().flat_map(|instruction| instruction.uses.iter().chain(&instruction.defs));
    registers.map(|&register| usize::from(register) + 1).max().unwrap_or(0)
}

/// Registers read before any write in each block, with how many blocks read each.
pub struct Exposure {
    per_block: Vec<Bits>,
    /// Blocks reading each register before writing it, by register.
    blocks_reading: Vec<u32>,
}

impl Exposure {
    pub fn of(instructions: &[FlowInstruction], blocks: &[std::ops::Range<usize>]) -> Self {
        let width = register_width(instructions);
        let per_block: Vec<Bits> = blocks
            .iter()
            .map(|block| exposed_reads(&instructions[block.clone()], width))
            .collect();
        let mut blocks_reading = vec![0; width];
        for register in per_block.iter().flat_map(Bits::iter) {
            blocks_reading[register] += 1;
        }
        Self { per_block, blocks_reading }
    }

    /// Whether a block other than `block` reads `register` before writing it.
    pub fn escapes(&self, block: usize, register: u16) -> bool {
        let own = u32::from(self.per_block[block].contains(usize::from(register)));
        self.blocks_reading.get(usize::from(register)).copied().unwrap_or(0) > own
    }
}

fn exposed_reads(block: &[FlowInstruction], width: usize) -> Bits {
    let (mut written, mut exposed) = (Bits::new(width), Bits::new(width));
    for instruction in block {
        for &register in instruction.uses.iter().filter(|&&register| !written.contains(usize::from(register))) {
            exposed.insert(usize::from(register));
        }
        for &register in &instruction.defs {
            written.insert(usize::from(register));
        }
    }
    exposed
}

pub fn analyze(block: &[FlowInstruction], escapes: impl Fn(u16) -> bool) -> Flow {
    let mut last_def: Vec<Option<usize>> = vec![None; register_width(block)];
    let mut links_in = vec![Vec::new(); block.len()];
    let mut readers = vec![0usize; block.len()];
    for (index, instruction) in block.iter().enumerate() {
        for register in &instruction.uses {
            if let Some(producer) = last_def[usize::from(*register)] {
                readers[producer] += 1;
                if !links_in[index].contains(&producer) {
                    links_in[index].push(producer);
                }
            }
        }
        links_in[index].sort_unstable();
        for &register in &instruction.defs {
            last_def[usize::from(register)] = Some(index);
        }
    }
    let mut single_use = Bits::new(block.len());
    for (index, instruction) in block.iter().enumerate() {
        if matches!(instruction.defs.as_slice(), [register] if readers[index] == 1 && !escapes(*register)) {
            single_use.insert(index);
        }
    }
    Flow { links_in, single_use }
}
