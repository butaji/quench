//! Quench: dominator-scoped value numbering and dominated-branch folding.
//!
//! The lane specializer emits each guard from the facts of its own site, so a value that is
//! tested for the same tag, shape or bound by several sites in one dominating chain is tested
//! at each. This pass runs before any opt level's elaboration (opt `none` has no value
//! numbering at all) and removes the repeats at their source:
//!
//! * a pure scalar operation, or a `notrap` load with no store or call between, that an
//!   earlier dominating instruction already computed becomes an alias of that result;
//! * a `brif` whose condition (or the same comparison, or its complement) the single edge into
//!   a dominating block already decided becomes a `jump`.
//!
//! The pass trusts only SSA dominance and an explicit memory epoch: it is exact, keyed on
//! nothing but instruction shapes, and cannot reorder anything.

use crate::dominator_tree::DominatorTree;
use crate::flowgraph::ControlFlowGraph;
use crate::ir::condcodes::{CondCode, IntCC};
use crate::ir::{Block, Function, Inst, InstructionData, MemFlags, Opcode, Type, Value, ValueDef};
use alloc::vec::Vec;
use cranelift_entity::SecondaryMap;
use crate::FxHashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    opcode: Opcode,
    ty: Type,
    args: [Value; 2],
    imm: i64,
    cond: u8,
    flags: Option<MemFlags>,
    epoch: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fact {
    Value(Value),
    Compare(IntCC, Value, Value),
}

fn pure_scalar(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::Iconst
            | Opcode::Band
            | Opcode::Bor
            | Opcode::Bxor
            | Opcode::Ushr
            | Opcode::Sshr
            | Opcode::Ishl
            | Opcode::Iadd
            | Opcode::Isub
            | Opcode::Imul
            | Opcode::Icmp
            | Opcode::Uextend
            | Opcode::Sextend
            | Opcode::Ireduce
    )
}

struct Pass<'a> {
    func: &'a mut Function,
    numbered: FxHashMap<Key, Value>,
    numbered_undo: Vec<(Key, Option<Value>)>,
    facts: FxHashMap<Fact, bool>,
    facts_undo: Vec<(Fact, Option<bool>)>,
    epoch: u32,
    next_epoch: u32,
    folded: bool,
}

impl Pass<'_> {
    fn key(&self, inst: Inst) -> Option<Key> {
        let dfg = &self.func.dfg;
        let mut key = Key {
            opcode: dfg.insts[inst].opcode(),
            ty: crate::ir::types::INVALID,
            args: [Value::from_u32(u32::MAX); 2],
            imm: 0,
            cond: 0,
            flags: None,
            epoch: 0,
        };
        match dfg.insts[inst] {
            InstructionData::UnaryImm { opcode, imm } if pure_scalar(opcode) => key.imm = imm.bits(),
            InstructionData::Unary { opcode, arg } if pure_scalar(opcode) => key.args[0] = dfg.resolve_aliases(arg),
            InstructionData::Binary { opcode, args } if pure_scalar(opcode) => {
                key.args = [dfg.resolve_aliases(args[0]), dfg.resolve_aliases(args[1])];
            }
            InstructionData::IntCompare { opcode, args, cond } if pure_scalar(opcode) => {
                key.args = [dfg.resolve_aliases(args[0]), dfg.resolve_aliases(args[1])];
                key.cond = IntCC::all().iter().position(|c| *c == cond)? as u8;
            }
            InstructionData::Load { opcode, arg, flags, offset } if opcode.can_load() && dfg.mem_flags[flags].notrap() => {
                key.args[0] = dfg.resolve_aliases(arg);
                key.imm = i64::from(i32::from(offset));
                key.flags = Some(flags);
                key.epoch = self.epoch;
            }
            _ => return None,
        }
        let results = dfg.inst_results(inst);
        if results.len() != 1 {
            return None;
        }
        key.ty = dfg.value_type(results[0]);
        Some(key)
    }

    fn learn(&mut self, fact: Fact, holds: bool) {
        let old = self.facts.insert(fact, holds);
        self.facts_undo.push((fact, old));
    }

    fn comparison(&self, condition: Value) -> Option<(IntCC, Value, Value)> {
        let ValueDef::Result(inst, _) = self.func.dfg.value_def(condition) else { return None };
        let InstructionData::IntCompare { args, cond, .. } = self.func.dfg.insts[inst] else { return None };
        Some((cond, self.func.dfg.resolve_aliases(args[0]), self.func.dfg.resolve_aliases(args[1])))
    }

    fn learn_condition(&mut self, condition: Value, holds: bool) {
        let condition = self.func.dfg.resolve_aliases(condition);
        self.learn(Fact::Value(condition), holds);
        if let Some((cond, a, b)) = self.comparison(condition) {
            self.learn(Fact::Compare(cond, a, b), holds);
            self.learn(Fact::Compare(cond.complement(), a, b), !holds);
        }
    }

    fn known(&self, condition: Value) -> Option<bool> {
        let condition = self.func.dfg.resolve_aliases(condition);
        if let Some(holds) = self.facts.get(&Fact::Value(condition)) {
            return Some(*holds);
        }
        let (cond, a, b) = self.comparison(condition)?;
        self.facts.get(&Fact::Compare(cond, a, b)).copied()
    }

    fn fresh_epoch(&mut self) {
        self.next_epoch += 1;
        self.epoch = self.next_epoch;
    }

    fn block(&mut self, block: Block) {
        let mut cursor = self.func.layout.first_inst(block);
        while let Some(inst) = cursor {
            cursor = self.func.layout.next_inst(inst);
            let opcode = self.func.dfg.insts[inst].opcode();
            if let Some(key) = self.key(inst) {
                let result = self.func.dfg.first_result(inst);
                if let Some(&leader) = self.numbered.get(&key) {
                    if let ValueDef::Result(leader_inst, _) = self.func.dfg.value_def(leader) {
                        self.func.dfg.replace_with_aliases(inst, leader_inst);
                        self.func.layout.remove_inst(inst);
                    }
                } else {
                    let old = self.numbered.insert(key, result);
                    self.numbered_undo.push((key, old));
                }
                continue;
            }
            if let InstructionData::Brif { arg, blocks, .. } = self.func.dfg.insts[inst] {
                if let Some(holds) = self.known(arg) {
                    let taken = blocks[usize::from(!holds)];
                    self.func.dfg.insts[inst] = InstructionData::Jump { opcode: Opcode::Jump, destination: taken };
                    self.folded = true;
                }
                continue;
            }
            if opcode.can_store() || opcode.is_call() || opcode.other_side_effects() {
                self.fresh_epoch();
            }
        }
    }
}

/// Run the pass; whether it folded a branch. Only then does the CFG change: the caller
/// recomputes it and removes the blocks the fold left unreachable. Value numbering alone
/// leaves it and the dominator tree valid.
pub(crate) fn run(func: &mut Function, cfg: &ControlFlowGraph, domtree: &DominatorTree) -> bool {
    let Some(entry) = func.layout.entry_block() else { return false };
    let mut predecessors = SecondaryMap::<Block, u32>::with_default(0);
    for block in func.layout.blocks() {
        predecessors[block] = cfg.pred_iter(block).count() as u32;
    }
    let mut pass = Pass {
        func,
        numbered: FxHashMap::default(),
        numbered_undo: Vec::new(),
        facts: FxHashMap::default(),
        facts_undo: Vec::new(),
        epoch: 0,
        next_epoch: 0,
        folded: false,
    };
    enum Step {
        Enter(Block),
        Exit(usize, usize),
    }
    let mut end_epoch = SecondaryMap::<Block, u32>::with_default(0);
    let mut stack = alloc::vec![Step::Enter(entry)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(block) => {
                let marks = (pass.numbered_undo.len(), pass.facts_undo.len());
                match domtree.idom(block).filter(|_| predecessors[block] == 1) {
                    Some(parent) => {
                        pass.epoch = end_epoch[parent];
                        // The single incoming edge decides the parent's branch for this block.
                        if let Some(last) = pass.func.layout.last_inst(parent) {
                            if let InstructionData::Brif { arg, blocks, .. } = pass.func.dfg.insts[last] {
                                let pool = &pass.func.dfg.value_lists;
                                let (then_block, else_block) = (blocks[0].block(pool), blocks[1].block(pool));
                                if then_block != else_block {
                                    pass.learn_condition(arg, then_block == block);
                                }
                            }
                        }
                    }
                    None => pass.fresh_epoch(),
                }
                pass.block(block);
                end_epoch[block] = pass.epoch;
                stack.push(Step::Exit(marks.0, marks.1));
                let children: Vec<Block> = domtree.children(block).collect();
                for child in children.into_iter().rev() {
                    stack.push(Step::Enter(child));
                }
            }
            Step::Exit(numbered, facts) => {
                while pass.numbered_undo.len() > numbered {
                    let (key, old) = pass.numbered_undo.pop().expect("mark within log");
                    match old {
                        Some(value) => {
                            pass.numbered.insert(key, value);
                        }
                        None => {
                            pass.numbered.remove(&key);
                        }
                    }
                }
                while pass.facts_undo.len() > facts {
                    let (fact, old) = pass.facts_undo.pop().expect("mark within log");
                    match old {
                        Some(holds) => {
                            pass.facts.insert(fact, holds);
                        }
                        None => {
                            pass.facts.remove(&fact);
                        }
                    }
                }
            }
        }
    }
    pass.folded
}
