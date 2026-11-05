use crate::dominator_tree::DominatorTree;
use crate::ir::{Block, Function, Value, ValueLabelAssignments, types};
use crate::loop_analysis::{Loop, LoopAnalysis};
use alloc::vec::Vec;
use cranelift_entity::{EntitySet, SecondaryMap, packed_option::PackedOption};

/// Keep loop-confined scalar literals at the loop entry after elaboration's
/// local rematerialization. No effects or trapping computations move across
/// the entry branch; other rematerialization decisions remain unchanged.
pub(crate) fn run(func: &mut Function, domtree: &DominatorTree, loop_analysis: &LoopAnalysis) {
    if loop_analysis.loops().next().is_none() { return; }
    use crate::ir::{InstructionData, Opcode};
    let mut use_loop = SecondaryMap::<Value, PackedOption<Loop>>::new();
    let mut mixed_uses = EntitySet::<Value>::new();
    let mut entry = SecondaryMap::<Loop, PackedOption<Block>>::new();
    let mut multiple_entries = EntitySet::<Loop>::new();
    let mut calling_loops = EntitySet::<Loop>::new();
    for block in func.layout.blocks() {
        let lp = loop_analysis.innermost_loop(block);
        for inst in func.layout.block_insts(block) {
            // Call clobbers make long-lived literal registers expensive.
            // Mark ancestors too: each loop is visited only on its first mark.
            if func.dfg.insts[inst].opcode().is_call() {
                let mut containing = lp;
                while let Some(lp) = containing {
                    if !calling_loops.insert(lp) {
                        break;
                    }
                    containing = loop_analysis.loop_parent(lp);
                }
            }
            for value in func.dfg.inst_values(inst) {
                let value = func.dfg.resolve_aliases(value);
                match (use_loop[value].expand(), lp) {
                    (_, None) => {
                        mixed_uses.insert(value);
                    }
                    (None, Some(lp)) => use_loop[value] = lp.into(),
                    (Some(old), Some(lp)) if old != lp => {
                        mixed_uses.insert(value);
                    }
                    _ => {}
                }
            }
        }
        let Some(last) = func.layout.last_inst(block) else {
            continue;
        };
        for dest in func.dfg.insts[last]
            .branch_destination(&func.dfg.jump_tables, &func.dfg.exception_tables)
        {
            let header = dest.block(&func.dfg.value_lists);
            let Some(lp) = loop_analysis.innermost_loop(header) else {
                continue;
            };
            if loop_analysis.loop_header(lp) != header || loop_analysis.is_in_loop(block, lp) {
                continue;
            }
            match entry[lp].expand() {
                None => entry[lp] = block.into(),
                Some(old) if old != block => {
                    multiple_entries.insert(lp);
                }
                _ => {}
            }
        }
    }
    let mut placements = Vec::new();
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            let InstructionData::UnaryImm {
                opcode: Opcode::Iconst,
                imm,
            } = func.dfg.insts[inst]
            else {
                continue;
            };
            let value = func.dfg.first_result(inst);
            let ty = func.dfg.value_type(value);
            if !ty.is_int() || ty.bits() > types::I64.bits() || mixed_uses.contains(value) {
                continue;
            }
            let Some(lp) = use_loop[value].expand() else {
                continue;
            };
            if loop_analysis.innermost_loop(block) != Some(lp)
                || multiple_entries.contains(lp)
                || calling_loops.contains(lp)
            {
                continue;
            }
            let Some(parent) = entry[lp].expand() else {
                continue;
            };
            if domtree.idom(loop_analysis.loop_header(lp)) != Some(parent) {
                continue;
            }
            let last = func.layout.last_inst(parent).unwrap();
            if !matches!(func.dfg.insts[last].opcode(), Opcode::Jump | Opcode::Brif) {
                continue;
            }
            placements.push((parent, ty.bits(), i64::from(imm), inst));
        }
    }
    if placements.is_empty() {
        return;
    }
    // Versioned loops can share an entry. One literal there is sufficient
    // for all versions, without a hash table or duplicate live registers.
    placements.sort_unstable();
    // Keep distinct debug identities, including label-alias targets. Moving
    // their definitions is safe; combining their assignment histories is not.
    let mut labelled = EntitySet::<Value>::new();
    if let Some(labels) = &func.dfg.values_labels {
        for (&value, assignments) in labels {
            labelled.insert(func.dfg.resolve_aliases(value));
            if let ValueLabelAssignments::Alias { value, .. } = assignments {
                labelled.insert(func.dfg.resolve_aliases(*value));
            }
        }
    }
    let mut previous: Option<((Block, u32, i64), Value)> = None;
    let mut replacements = SecondaryMap::<Value, PackedOption<Value>>::new();
    for (parent, bits, imm, inst) in placements {
        let value = func.dfg.first_result(inst);
        func.layout.remove_inst(inst);
        let key = (parent, bits, imm);
        if let Some((old_key, old_value)) = previous {
            if key == old_key && !labelled.contains(value) {
                replacements[value] = old_value.into();
                continue;
            }
        }
        let last = func.layout.last_inst(parent).unwrap();
        func.layout.insert_inst(inst, last);
        // The materialization can now execute without entering its original
        // source block. Do not report that source statement as executed.
        func.srclocs[inst] = Default::default();
        if !labelled.contains(value) {
            previous = Some((key, value));
        }
    }
    // Project through pre-existing aliases without cleaning up unrelated
    // aliases or their debug assignment histories.
    for value in func.dfg.values() {
        let original = func.dfg.resolve_aliases(value);
        if let Some(replacement) = replacements[original].expand() {
            replacements[value] = replacement.into();
        }
    }
    let stencil = &mut func.stencil;
    for block in stencil.layout.blocks() {
        for inst in stencil.layout.block_insts(block) {
            stencil.dfg.map_inst_values(inst, |value| replacements[value].expand().unwrap_or(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::{Cursor, FuncCursor};
    use crate::flowgraph::ControlFlowGraph;
    use crate::ir::{self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName, types};
    use alloc::vec;

    const LITERAL: i64 = 0xD008;
    const TEST_SOURCE_OFFSET: u32 = 42;

    fn function() -> Function {
        let mut signature = Signature::new(crate::isa::CallConv::SystemV);
        signature.params.extend([AbiParam::new(types::I8), AbiParam::new(types::I64)]);
        signature.returns.push(AbiParam::new(types::I32));
        Function::with_name_signature(UserFuncName::testcase("loop_constants"), signature)
    }

    fn apply(f: &mut Function) {
        let flags = crate::settings::Flags::new(crate::settings::builder());
        crate::verify_function(&*f, &flags).unwrap();
        let cfg = ControlFlowGraph::with_function(f);
        let dom = DominatorTree::with_function(f, &cfg);
        let mut loops = LoopAnalysis::new();
        loops.compute(f, &cfg, &dom);
        run(f, &dom, &loops);
        crate::verify_function(&*f, &flags).unwrap();
    }

    #[test]
    fn mixed_outside_sibling_and_nested_uses_do_not_move() {
        #[derive(Clone, Copy, Debug)]
        enum Shape { Confined, Outside, Sibling, Nested }
        for shape in [Shape::Confined, Shape::Outside, Shape::Sibling, Shape::Nested] {
            let mut f = function();
            let entry = f.dfg.make_block();
            let header = f.dfg.make_block();
            let second = f.dfg.make_block();
            let latch = f.dfg.make_block();
            let exit = f.dfg.make_block();
            let again = f.dfg.append_block_param(entry, types::I8);
            let pointer = f.dfg.append_block_param(entry, types::I64);
            let mut cursor = FuncCursor::new(&mut f);
            cursor.insert_block(entry);
            cursor.ins().jump(header, &[]);
            cursor.insert_block(header);
            let literal = cursor.ins().iconst(types::I32, LITERAL);
            let ir::ValueDef::Result(definition, _) = cursor.func.dfg.value_def(literal) else { unreachable!() };
            cursor.func.srclocs[definition] = ir::RelSourceLoc::new(TEST_SOURCE_OFFSET);
            cursor.ins().store(MemFlagsData::trusted(), literal, pointer, 0);
            match shape {
                Shape::Confined | Shape::Outside => { cursor.ins().brif(again, header, &[], exit, &[]); }
                Shape::Sibling => {
                    cursor.ins().brif(again, header, &[], second, &[]);
                    cursor.insert_block(second);
                    cursor.ins().store(MemFlagsData::trusted(), literal, pointer, 0);
                    cursor.ins().brif(again, second, &[], exit, &[]);
                }
                Shape::Nested => {
                    cursor.ins().jump(second, &[]);
                    cursor.insert_block(second);
                    cursor.ins().store(MemFlagsData::trusted(), literal, pointer, 0);
                    cursor.ins().brif(again, second, &[], latch, &[]);
                    cursor.insert_block(latch);
                    cursor.ins().store(MemFlagsData::trusted(), literal, pointer, 0);
                    cursor.ins().brif(again, header, &[], exit, &[]);
                }
            }
            cursor.insert_block(exit);
            let result = if matches!(shape, Shape::Outside) { literal } else { cursor.ins().iconst(types::I32, 0) };
            cursor.ins().return_(&[result]);
            apply(&mut f);
            assert_eq!(f.layout.inst_block(definition), Some(if matches!(shape, Shape::Confined) { entry } else { header }), "{shape:?}: {f}");
            assert_eq!(f.srclocs[definition], if matches!(shape, Shape::Confined) { ir::RelSourceLoc::default() } else { ir::RelSourceLoc::new(TEST_SOURCE_OFFSET) });
        }
    }

    #[test]
    fn coalescing_updates_alias_branch_arguments_and_preserves_debug_roots() {
        for debug in [false, true] {
            let mut f = function();
            let entry = f.dfg.make_block();
            let headers = [f.dfg.make_block(), f.dfg.make_block()];
            let exit = f.dfg.make_block();
            let choose = f.dfg.append_block_param(entry, types::I8);
            let pointer = f.dfg.append_block_param(entry, types::I64);
            for header in headers { f.dfg.append_block_param(header, types::I32); }
            let result = f.dfg.append_block_param(exit, types::I32);
            let mut cursor = FuncCursor::new(&mut f);
            cursor.insert_block(entry);
            let initial = cursor.ins().iconst(types::I32, 0);
            cursor.ins().brif(choose, headers[0], &[initial.into()], headers[1], &[initial.into()]);
            let mut literals = Vec::new();
            for header in headers {
                cursor.insert_block(header);
                let literal = cursor.ins().iconst(types::I32, LITERAL);
                let alias = cursor.ins().iconst(types::I32, 0);
                let ir::ValueDef::Result(definition, _) = cursor.func.dfg.value_def(literal) else { unreachable!() };
                let ir::ValueDef::Result(alias_inst, _) = cursor.func.dfg.value_def(alias) else { unreachable!() };
                cursor.func.srclocs[definition] = ir::RelSourceLoc::new(TEST_SOURCE_OFFSET);
                cursor.func.dfg.replace_with_aliases(alias_inst, definition);
                cursor.func.layout.remove_inst(alias_inst);
                cursor.ins().store(MemFlagsData::trusted(), alias, pointer, 0);
                cursor.ins().brif(choose, header, &[alias.into()], exit, &[alias.into()]);
                literals.push(literal);
            }
            cursor.insert_block(exit);
            cursor.ins().return_(&[result]);
            if debug {
                cursor.func.collect_debug_info();
                cursor.func.dfg.values_labels.as_mut().unwrap().insert(literals[0], ir::ValueLabelAssignments::Starts(vec![ir::ValueLabelStart {
                    from: ir::RelSourceLoc::new(0), label: ir::ValueLabel::from_u32(0),
                }]));
                cursor.func.dfg.add_value_label_alias(literals[1], ir::RelSourceLoc::new(0), literals[0]);
            }
            let labels = cursor.func.dfg.values_labels.clone();
            apply(&mut f);
            assert_eq!(f.dfg.values_labels, labels, "unrelated label roots and aliases survive relocation");
            let constants = f.layout.blocks().flat_map(|block| f.layout.block_insts(block)).filter(|inst|
                matches!(f.dfg.insts[*inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } if imm.bits() == LITERAL)).count();
            assert_eq!(constants, if debug { 2 } else { 1 }, "protected debug definitions are not coalesced: {f}");
            for inst in f.layout.block_insts(entry).filter(|inst|
                matches!(f.dfg.insts[*inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } if imm.bits() == LITERAL)) {
                assert_eq!(f.srclocs[inst], ir::RelSourceLoc::default(), "speculative materialization is not a source execution probe");
            }
            for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
                for value in f.dfg.inst_values(inst) {
                    if let ir::ValueDef::Result(definition, _) = f.dfg.value_def(f.dfg.resolve_aliases(value)) {
                        assert!(f.layout.inst_block(definition).is_some(), "live alias operand refers to a removed definition: {f}");
                    }
                }
            } }
            if debug { for value in literals {
                let ir::ValueDef::Result(definition, _) = f.dfg.value_def(value) else { panic!("debug literal changed identity") };
                assert!(f.layout.inst_block(definition).is_some());
            } }
        }
    }

    #[test]
    fn calls_block_selected_and_ancestor_loops_but_not_siblings() {
        #[derive(Clone, Copy, Debug)]
        enum CallSite { Selected, Descendant, Sibling }
        const TEST_CALL_NAMESPACE: u32 = u32::from_be_bytes(*b"QCLC");
        for site in [CallSite::Selected, CallSite::Descendant, CallSite::Sibling] {
            let mut f = function();
            let signature = f.import_signature(Signature::new(crate::isa::CallConv::SystemV));
            let name = f.declare_imported_user_function(ir::UserExternalName::new(TEST_CALL_NAMESPACE, 0));
            let target = f.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name), signature, colocated: false, patchable: false,
            });
            let entry = f.dfg.make_block();
            let header = f.dfg.make_block();
            let other = f.dfg.make_block();
            let latch = f.dfg.make_block();
            let exit = f.dfg.make_block();
            let again = f.dfg.append_block_param(entry, types::I8);
            let pointer = f.dfg.append_block_param(entry, types::I64);
            let mut cursor = FuncCursor::new(&mut f);
            cursor.insert_block(entry);
            if matches!(site, CallSite::Sibling) {
                cursor.ins().brif(again, header, &[], other, &[]);
            } else { cursor.ins().jump(header, &[]); }
            cursor.insert_block(header);
            let literal = cursor.ins().iconst(types::I32, LITERAL);
            let ir::ValueDef::Result(definition, _) = cursor.func.dfg.value_def(literal) else { unreachable!() };
            cursor.func.srclocs[definition] = ir::RelSourceLoc::new(TEST_SOURCE_OFFSET);
            cursor.ins().store(MemFlagsData::trusted(), literal, pointer, 0);
            match site {
                CallSite::Selected => {
                    cursor.ins().call(target, &[]);
                    cursor.ins().brif(again, header, &[], exit, &[]);
                }
                CallSite::Descendant => {
                    cursor.ins().jump(other, &[]);
                    cursor.insert_block(other);
                    cursor.ins().call(target, &[]);
                    cursor.ins().brif(again, other, &[], latch, &[]);
                    cursor.insert_block(latch);
                    cursor.ins().brif(again, header, &[], exit, &[]);
                }
                CallSite::Sibling => {
                    cursor.ins().brif(again, header, &[], exit, &[]);
                    cursor.insert_block(other);
                    cursor.ins().call(target, &[]);
                    cursor.ins().brif(again, other, &[], exit, &[]);
                }
            }
            cursor.insert_block(exit);
            let zero = cursor.ins().iconst(types::I32, 0);
            cursor.ins().return_(&[zero]);
            apply(&mut f);
            let moved = matches!(site, CallSite::Sibling);
            assert_eq!(f.layout.inst_block(definition), Some(if moved { entry } else { header }), "{site:?}: {f}");
            assert_eq!(f.srclocs[definition], if moved { ir::RelSourceLoc::default() }
                else { ir::RelSourceLoc::new(TEST_SOURCE_OFFSET) });
        }
    }

}
