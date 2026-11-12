//! Lane-frame write-backs at calls: a register is stored once per definition, not once per call.

use super::*;

/// The `(offset, address, value)` of each store on the hot path, which no cold block holds.
fn hot_stores(function: &ir::Function) -> Vec<(i32, ir::Value, ir::Value)> {
    function.layout.blocks().filter(|block| !function.layout.is_cold(*block))
        .flat_map(|block| function.layout.block_insts(block))
        .filter_map(|inst| match function.dfg.insts[inst] {
            ir::InstructionData::Store { args, offset, .. } => Some((i32::from(offset),
                function.dfg.resolve_aliases(args[1]), function.dfg.resolve_aliases(args[0]))),
            _ => None,
        }).collect()
}

#[test]
fn calls_write_back_a_register_once_per_definition() {
    let source = "function run(g,n){let t=0,i=0;while(i<n){g(i);g(i);g(i);t=(t+i)|0;i=(i+1)|0;}return t;}";
    let program = crate::Engine::specialize(source, "home-calls.js").unwrap();
    let caller = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "run")).unwrap();
    let unit = super::super::lane_unit::unit(&program, caller as u32, program.function(caller),
        &|_, _| Vec::new(), &|id| program.function_get(id as usize)).unwrap();
    let header = unit.instructions.iter().enumerate().filter_map(|(pc, instruction)|
        instruction.op().control_flow_layout().jumps()
            .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc)).min().unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses::<crate::test_host::TestHost>();
    let region = lower(&program, caller as u32, &unit, LaneHelpers::Process(addresses),
        &BTreeMap::new(), &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[],
        Coverage::PrefixFrom(header), Completion::StatusAndPc, ElementViews::Dense).expect("call loop region");
    cranelift_codegen::verify_function(&region.function, &*isa).unwrap();
    let stores = hot_stores(&region.function);
    let mut distinct = stores.clone();
    distinct.sort_by_key(|(offset, address, value)| (*offset, address.index(), value.index()));
    distinct.dedup();
    assert_eq!(stores.len(), distinct.len(), "a call stored a register its frame slot already held: {}", region.function);
}
