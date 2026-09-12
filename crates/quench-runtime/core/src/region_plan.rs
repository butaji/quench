use super::dynbytecode::{DynCode, DynOp, Register, UnaryKind, derive_blocks};

const REGISTER_BITS_PER_WORD: usize = u64::BITS as usize;
const MAX_LIVENESS_FIXPOINT_ROUNDS_FACTOR: usize = 2;
const EXIT_BLOCK: usize = usize::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RegisterRewriteStats {
    pub coalesced_moves: usize,
    pub dead_definitions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(u32);

impl BlockId {
    fn new(index: usize) -> Result<Self, &'static str> {
        index
            .try_into()
            .map(Self)
            .map_err(|_| "RegionPlan block count exceeds BlockId")
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

pub(crate) fn simplify_register_code(
    code: &mut DynCode,
) -> Result<RegisterRewriteStats, &'static str> {
    let mut stats = RegisterRewriteStats::default();
    let initial_plan = RegionPlan::quote(code)?;
    let mut keep = vec![true; code.ops.len()];

    // Work backwards so a chain `producer; move; move` collapses into the
    // producer rather than retargeting an instruction that is later removed.
    for block in initial_plan.blocks().iter().copied() {
        for pc in (block.start..block.end).rev() {
            let DynOp::Move { dst, src } = code.ops[pc].op else {
                continue;
            };
            if dst == src {
                keep[pc] = false;
                stats.coalesced_moves += 1;
                continue;
            }
            let destination_is_live = initial_plan.is_live_out(block, dst)
                || code.ops[pc + 1..block.end]
                    .iter()
                    .enumerate()
                    .any(|(relative, instruction)| {
                        keep[pc + 1 + relative] && instruction.op.reads_register(dst)
                    });
            let source_is_live = initial_plan.is_live_out(block, src)
                || code.ops[pc + 1..block.end]
                    .iter()
                    .enumerate()
                    .any(|(relative, instruction)| {
                        keep[pc + 1 + relative] && instruction.op.reads_register(src)
                    });
            if !destination_is_live || source_is_live {
                continue;
            }
            let Some(producer_pc) = (block.start..pc).rev().find(|candidate| keep[*candidate])
            else {
                continue;
            };
            if code.ops[producer_pc].op.written_register() != Some(src)
                || code.ops[producer_pc].op.reads_register(dst)
            {
                continue;
            }
            if code.ops[producer_pc].op.replace_written_register(src, dst) {
                keep[pc] = false;
                stats.coalesced_moves += 1;
            }
        }
    }
    compact_instructions(code, &keep)?;

    let dce_plan = RegionPlan::quote(code)?;
    keep.fill(true);
    keep.resize(code.ops.len(), true);
    let mut live = vec![false; code.registers];
    for block in dce_plan.blocks().iter().copied() {
        live.fill(false);
        for register in 0..code.registers {
            let register = Register::try_from(register)
                .map_err(|_| "register count exceeds register representation")?;
            live[register as usize] = dce_plan.is_live_out(block, register);
        }
        for pc in (block.start..block.end).rev() {
            let operation = &code.ops[pc].op;
            if let Some(destination) = operation.written_register()
                && !live[destination as usize]
                && dead_definition_is_removable(operation)
            {
                keep[pc] = false;
                stats.dead_definitions += 1;
                continue;
            }
            if let Some(destination) = operation.written_register() {
                live[destination as usize] = false;
            }
            operation.for_each_read_register(|register| live[register as usize] = true);
        }
    }
    compact_instructions(code, &keep)?;
    Ok(stats)
}

fn dead_definition_is_removable(operation: &DynOp) -> bool {
    matches!(
        operation,
        DynOp::LoadLiteral { .. }
            | DynOp::LoadLocal { .. }
            | DynOp::LoadThis { .. }
            | DynOp::Move { .. }
            | DynOp::Unary {
                kind: UnaryKind::Not | UnaryKind::Void,
                ..
            }
            | DynOp::Binary {
                kind: super::Op::StrictEq | super::Op::StrictNe,
                ..
            }
    )
}

fn compact_instructions(code: &mut DynCode, keep: &[bool]) -> Result<(), &'static str> {
    if keep.len() != code.ops.len() {
        return Err("instruction keep mask has the wrong length");
    }
    let mut boundary_map = vec![0usize; code.ops.len() + 1];
    let mut compact_pc = 0usize;
    for (pc, retained) in keep.iter().copied().enumerate() {
        boundary_map[pc] = compact_pc;
        compact_pc += usize::from(retained);
    }
    boundary_map[code.ops.len()] = compact_pc;

    let mut compact = Vec::with_capacity(compact_pc);
    for (retained, mut instruction) in keep.iter().copied().zip(std::mem::take(&mut code.ops)) {
        if !retained {
            continue;
        }
        remap_control_target(&mut instruction.op, &boundary_map)?;
        compact.push(instruction);
    }
    if compact.is_empty() {
        return Err("register simplification removed the complete function");
    }
    code.ops = compact;
    code.blocks = derive_blocks(&code.ops);
    Ok(())
}

fn remap_control_target(operation: &mut DynOp, boundary_map: &[usize]) -> Result<(), &'static str> {
    let target = match operation {
        DynOp::Jump { target }
        | DynOp::JumpIfFalse { target, .. }
        | DynOp::PushHandler { target } => Some(target),
        DynOp::ForInNext { done, .. } => Some(done),
        _ => None,
    };
    if let Some(target) = target {
        *target = *boundary_map
            .get(*target)
            .ok_or("control target is outside bytecode during compaction")?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionBlock {
    pub id: BlockId,
    pub start: usize,
    pub end: usize,
    pub loop_region: bool,
    successor_start: usize,
    successor_count: usize,
}

impl RegionBlock {
    pub fn legacy_tuple(self) -> (usize, usize, bool) {
        (self.start, self.end, self.loop_region)
    }
}

#[derive(Debug)]
pub struct RegionPlan {
    blocks: Box<[RegionBlock]>,
    successors: Box<[BlockId]>,
    register_word_count: usize,
    live_in: Box<[u64]>,
    live_out: Box<[u64]>,
}

impl RegionPlan {
    pub fn quote(code: &DynCode) -> Result<Self, &'static str> {
        if code.ops.is_empty() || code.blocks.is_empty() {
            return Err("RegionPlan requires non-empty bytecode and block data");
        }
        let register_word_count = code.registers.div_ceil(REGISTER_BITS_PER_WORD);
        let mut block_at_pc = vec![EXIT_BLOCK; code.ops.len() + 1];
        let mut previous_end = 0usize;
        for (index, (start, end, _)) in code.blocks.iter().copied().enumerate() {
            if start != previous_end || end <= start || end > code.ops.len() {
                return Err("RegionPlan blocks must be ordered, contiguous, and non-empty");
            }
            block_at_pc[start] = index;
            previous_end = end;
        }
        if previous_end != code.ops.len() {
            return Err("RegionPlan blocks must cover all bytecode");
        }

        let mut successors = Vec::new();
        let mut blocks = Vec::with_capacity(code.blocks.len());
        for (index, (start, end, loop_region)) in code.blocks.iter().copied().enumerate() {
            let successor_start = successors.len();
            let mut target_pcs = [EXIT_BLOCK; 2];
            let successor_count =
                successor_pcs(&code.ops[end - 1].op, end, code.ops.len(), &mut target_pcs);
            for target_pc in target_pcs[..successor_count].iter().copied() {
                if target_pc == code.ops.len() {
                    continue;
                }
                let target_index = *block_at_pc
                    .get(target_pc)
                    .ok_or("RegionPlan successor is outside bytecode")?;
                if target_index == EXIT_BLOCK {
                    return Err("RegionPlan successor is not a block entry");
                }
                let target = BlockId::new(target_index)?;
                if !successors[successor_start..].contains(&target) {
                    successors.push(target);
                }
            }
            blocks.push(RegionBlock {
                id: BlockId::new(index)?,
                start,
                end,
                loop_region,
                successor_start,
                successor_count: successors.len() - successor_start,
            });
        }

        let liveness_words = blocks
            .len()
            .checked_mul(register_word_count)
            .ok_or("RegionPlan liveness size overflow")?;
        let mut uses = vec![0u64; liveness_words];
        let mut definitions = vec![0u64; liveness_words];
        for block in &blocks {
            let word_start = block.id.index() * register_word_count;
            for instruction in &code.ops[block.start..block.end] {
                let mut invalid_register = false;
                instruction.op.for_each_read_register(|register| {
                    if register as usize >= code.registers {
                        invalid_register = true;
                    } else if !bit_is_set(&definitions[word_start..], register) {
                        set_bit(&mut uses[word_start..], register);
                    }
                });
                if let Some(register) = instruction.op.written_register() {
                    if register as usize >= code.registers {
                        invalid_register = true;
                    } else {
                        set_bit(&mut definitions[word_start..], register);
                    }
                }
                if invalid_register {
                    return Err("RegionPlan instruction register is outside the register file");
                }
            }
        }

        let mut live_in = vec![0u64; liveness_words];
        let mut live_out = vec![0u64; liveness_words];
        let mut next_out = vec![0u64; register_word_count];
        let mut next_in = vec![0u64; register_word_count];
        let maximum_rounds = blocks
            .len()
            .saturating_mul(MAX_LIVENESS_FIXPOINT_ROUNDS_FACTOR)
            .saturating_add(1);
        let mut converged = false;
        for _ in 0..maximum_rounds {
            let mut changed = false;
            for block in blocks.iter().rev() {
                let word_start = block.id.index() * register_word_count;
                let word_end = word_start + register_word_count;
                next_out.fill(0);
                for successor in &successors
                    [block.successor_start..block.successor_start + block.successor_count]
                {
                    let successor_start = successor.index() * register_word_count;
                    for (word, successor_word) in next_out
                        .iter_mut()
                        .zip(&live_in[successor_start..successor_start + register_word_count])
                    {
                        *word |= *successor_word;
                    }
                }
                for word in 0..register_word_count {
                    next_in[word] = uses[word_start + word]
                        | (next_out[word] & !definitions[word_start + word]);
                }
                if live_out[word_start..word_end] != next_out
                    || live_in[word_start..word_end] != next_in
                {
                    live_out[word_start..word_end].copy_from_slice(&next_out);
                    live_in[word_start..word_end].copy_from_slice(&next_in);
                    changed = true;
                }
            }
            if !changed {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err("RegionPlan liveness did not converge within the named budget");
        }

        Ok(Self {
            blocks: blocks.into_boxed_slice(),
            successors: successors.into_boxed_slice(),
            register_word_count,
            live_in: live_in.into_boxed_slice(),
            live_out: live_out.into_boxed_slice(),
        })
    }

    pub fn blocks(&self) -> &[RegionBlock] {
        &self.blocks
    }

    pub fn successors(&self, block: RegionBlock) -> &[BlockId] {
        &self.successors[block.successor_start..block.successor_start + block.successor_count]
    }

    pub fn is_live_in(&self, block: RegionBlock, register: Register) -> bool {
        self.is_live(&self.live_in, block, register)
    }

    pub fn is_live_out(&self, block: RegionBlock, register: Register) -> bool {
        self.is_live(&self.live_out, block, register)
    }

    fn is_live(&self, words: &[u64], block: RegionBlock, register: Register) -> bool {
        let start = block.id.index() * self.register_word_count;
        bit_is_set(&words[start..start + self.register_word_count], register)
    }
}

fn successor_pcs(
    op: &DynOp,
    fallthrough: usize,
    code_len: usize,
    output: &mut [usize; 2],
) -> usize {
    match op {
        DynOp::Jump { target } => {
            output[0] = *target;
            1
        }
        DynOp::JumpIfFalse { target, .. } => {
            output[0] = *target;
            if fallthrough < code_len && fallthrough != *target {
                output[1] = fallthrough;
                2
            } else {
                1
            }
        }
        DynOp::ForInNext { done, .. } => {
            output[0] = *done;
            if fallthrough < code_len && fallthrough != *done {
                output[1] = fallthrough;
                2
            } else {
                1
            }
        }
        DynOp::Return { .. } | DynOp::Throw { .. } | DynOp::Rethrow => 0,
        _ if fallthrough < code_len => {
            output[0] = fallthrough;
            1
        }
        _ => 0,
    }
}

fn set_bit(words: &mut [u64], register: Register) {
    let register = register as usize;
    let word = register / REGISTER_BITS_PER_WORD;
    let bit = register % REGISTER_BITS_PER_WORD;
    if let Some(word) = words.get_mut(word) {
        *word |= 1u64 << bit;
    }
}

fn bit_is_set(words: &[u64], register: Register) -> bool {
    let register = register as usize;
    let word = register / REGISTER_BITS_PER_WORD;
    let bit = register % REGISTER_BITS_PER_WORD;
    words
        .get(word)
        .is_some_and(|word| word & (1u64 << bit) != 0)
}

#[cfg(test)]
mod tests {
    use super::super::dynbytecode::Literal;
    use super::*;
    use oxc_span::Span;

    const FIRST_REGISTER: Register = 0;
    const SECOND_REGISTER: Register = 1;
    const RESULT_REGISTER: Register = 2;
    const FIRST_LOCAL_SLOT: usize = 0;
    const SECOND_LOCAL_SLOT: usize = 1;
    const FIRST_NUMERIC_LITERAL: f64 = 1.0;
    const SECOND_NUMERIC_LITERAL: f64 = 2.0;

    fn instruction(op: DynOp) -> super::super::dynbytecode::DynInstr {
        super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        }
    }

    fn code(ops: Vec<DynOp>, registers: usize) -> DynCode {
        let ops = ops.into_iter().map(instruction).collect::<Vec<_>>();
        let blocks = derive_blocks(&ops);
        DynCode {
            ops,
            registers,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks,
            bindings: Vec::new(),
            is_script: false,
            strict: false,
        }
    }

    #[test]
    fn quote_builds_control_edges_and_loop_liveness() {
        let code = DynCode {
            ops: vec![
                instruction(DynOp::LoadLiteral {
                    dst: FIRST_REGISTER,
                    value: super::super::dynbytecode::Literal::Number(0.0),
                }),
                instruction(DynOp::Jump { target: 2 }),
                instruction(DynOp::JumpIfFalse {
                    test: FIRST_REGISTER,
                    target: 4,
                }),
                instruction(DynOp::Jump { target: 2 }),
                instruction(DynOp::Return {
                    src: Some(FIRST_REGISTER),
                }),
            ],
            registers: 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(0, 2, false), (2, 3, true), (3, 4, true), (4, 5, false)],
            bindings: Vec::new(),
            is_script: false,
            strict: false,
        };
        let plan = RegionPlan::quote(&code).expect("quote valid CFG");
        assert_eq!(plan.blocks().len(), 4);
        assert_eq!(plan.successors(plan.blocks()[0]), &[BlockId(1)]);
        assert_eq!(plan.successors(plan.blocks()[1]), &[BlockId(3), BlockId(2)]);
        assert!(plan.is_live_in(plan.blocks()[1], FIRST_REGISTER));
        assert!(plan.is_live_out(plan.blocks()[2], FIRST_REGISTER));
        assert!(!plan.is_live_in(plan.blocks()[0], FIRST_REGISTER));
    }

    #[test]
    fn register_simplification_coalesces_copy_and_remaps_jump() {
        let mut code = code(
            vec![
                DynOp::LoadLocal {
                    dst: FIRST_REGISTER,
                    slot: FIRST_LOCAL_SLOT,
                },
                DynOp::Move {
                    dst: SECOND_REGISTER,
                    src: FIRST_REGISTER,
                },
                DynOp::Jump { target: 3 },
                DynOp::Return {
                    src: Some(SECOND_REGISTER),
                },
            ],
            2,
        );

        let stats = simplify_register_code(&mut code).expect("simplify valid register code");

        assert_eq!(stats.coalesced_moves, 1);
        assert_eq!(stats.dead_definitions, 0);
        assert_eq!(code.ops.len(), 3);
        assert!(matches!(
            code.ops[0].op,
            DynOp::LoadLocal {
                dst: SECOND_REGISTER,
                slot: FIRST_LOCAL_SLOT
            }
        ));
        assert!(matches!(code.ops[1].op, DynOp::Jump { target: 2 }));
        assert!(matches!(
            code.ops[2].op,
            DynOp::Return {
                src: Some(SECOND_REGISTER)
            }
        ));
    }

    #[test]
    fn register_simplification_removes_dead_load_and_remaps_both_edges() {
        let mut code = code(
            vec![
                DynOp::LoadLocal {
                    dst: FIRST_REGISTER,
                    slot: FIRST_LOCAL_SLOT,
                },
                DynOp::LoadLocal {
                    dst: SECOND_REGISTER,
                    slot: SECOND_LOCAL_SLOT,
                },
                DynOp::JumpIfFalse {
                    test: SECOND_REGISTER,
                    target: 4,
                },
                DynOp::Jump { target: 5 },
                DynOp::Return { src: None },
                DynOp::Return { src: None },
            ],
            2,
        );

        let stats = simplify_register_code(&mut code).expect("simplify valid branch code");

        assert_eq!(stats.coalesced_moves, 0);
        assert_eq!(stats.dead_definitions, 1);
        assert_eq!(code.ops.len(), 5);
        assert!(matches!(
            code.ops[1].op,
            DynOp::JumpIfFalse {
                test: SECOND_REGISTER,
                target: 3
            }
        ));
        assert!(matches!(code.ops[2].op, DynOp::Jump { target: 4 }));
    }

    #[test]
    fn register_simplification_keeps_observable_coercive_binary() {
        let mut code = code(
            vec![
                DynOp::LoadLiteral {
                    dst: FIRST_REGISTER,
                    value: Literal::Number(FIRST_NUMERIC_LITERAL),
                },
                DynOp::LoadLiteral {
                    dst: SECOND_REGISTER,
                    value: Literal::Number(SECOND_NUMERIC_LITERAL),
                },
                DynOp::Binary {
                    kind: super::super::Op::Add,
                    dst: RESULT_REGISTER,
                    left: FIRST_REGISTER,
                    right: SECOND_REGISTER,
                },
                DynOp::Return { src: None },
            ],
            3,
        );

        let stats = simplify_register_code(&mut code).expect("simplify coercive binary code");

        assert_eq!(stats, RegisterRewriteStats::default());
        assert_eq!(code.ops.len(), 4);
    }
}
