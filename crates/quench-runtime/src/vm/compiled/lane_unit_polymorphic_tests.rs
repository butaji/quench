//! A caller's call that several alternatives inline keeps its own `Call` after the last of them.

use super::lane_unit::{unit, CalleeBinding, CallTarget, Origin, MAX_UNIT_INLINED_INSTRUCTIONS};
use crate::bytecode::Class;

#[test]
fn a_polymorphic_site_with_an_unbodied_target_keeps_its_call_after_the_last_alternative() {
    let big = "this.n=this.n+1;".repeat(MAX_UNIT_INLINED_INSTRUCTIONS);
    let source = format!("class A{{probe(x){{return x+1;}}}} class B{{probe(x){{return x+2;}}}} class C{{probe(x){{return x+3;}}}} \
        class D{{probe(x){{{big}return x;}}}} function caller(o,x){{let r=o.probe(x);return r+1;}}");
    let program = crate::Engine::specialize(&source, "polymorphic-miss.js").unwrap();
    let named = |name: &str| program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let caller = named("caller");
    let probes: Vec<u32> = program.functions.iter().enumerate().filter(|(_, body)|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "probe")).map(|(id, _)| id as u32).collect();
    assert_eq!(probes.len(), 4);
    let call_pc = program.function(caller).code.iter().position(|packed|
        super::super::dispatch_fast::decoded_instruction(program.function(caller), *packed).op().class().eq(Class::Call)).unwrap() as u32;
    let targets = |function: u32, pc: u32| if function == caller as u32 && pc == call_pc {
        probes.iter().map(|&function| CallTarget { function, callee: CalleeBinding::Function }).collect()
    } else { Vec::new() };
    let planned = unit(&program, caller as u32, program.function(caller), &targets, &|id| program.function_get(id as usize)).unwrap();
    assert_eq!(planned.frames.len(), 3, "the three small targets inline, the fourth has no body in the unit");
    let call = planned.instructions.iter().zip(&planned.origins).position(|(instruction, origin)|
        instruction.op().class().eq(Class::Call) && *origin == Origin::Caller(call_pc)).expect("the original call follows the alternatives");
    assert_eq!(planned.frames.last().unwrap().alternative, Some(call), "the last alternative's miss is the call");
    assert!(planned.frames[..2].iter().all(|frame| frame.alternative.is_some()));
    assert!(planned.register_masks(&program, caller as u32).is_some());
    // Several linked targets keep the call even when every one of them inlines.
    let few = |function: u32, pc: u32| targets(function, pc).into_iter().take(3).collect::<Vec<_>>();
    let planned = unit(&program, caller as u32, program.function(caller), &few, &|id| program.function_get(id as usize)).unwrap();
    assert!(planned.instructions.iter().any(|instruction| instruction.op().class().eq(Class::Call)), "several linked targets keep the call");
}

#[test]
fn a_budget_for_one_callee_inlines_the_heavier_call_site() {
    use super::lane_unit::{with_budget, with_trips};
    // The cold site comes first in program order; the hot one is in a loop.
    let source = "class A{p(x){return x+1;}} class B{q(x){return x+2;}} \
        function caller(a,b){let r=a.p(1);let s=0;for(let i=0;i<100;i++){s=s+b.q(i);}return r+s;}";
    let program = crate::Engine::specialize(source, "site-weights.js").unwrap();
    let id_of = |name: &str| program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap() as u32;
    let (caller, cold, hot) = (id_of("caller"), id_of("p"), id_of("q"));
    let calls: Vec<u32> = program.function(caller as usize).code.iter().enumerate().filter(|(_, packed)|
        super::super::dispatch_fast::decoded_instruction(program.function(caller as usize), **packed).op().class().eq(Class::Call))
        .map(|(pc, _)| pc as u32).collect();
    assert_eq!(calls.len(), 2);
    let targets = |function: u32, pc: u32| match (function == caller, pc) {
        (true, pc) if pc == calls[0] => vec![CallTarget { function: cold, callee: CalleeBinding::Function }],
        (true, pc) if pc == calls[1] => vec![CallTarget { function: hot, callee: CalleeBinding::Function }],
        _ => Vec::new(),
    };
    let body = |id: u32| program.function_get(id as usize);
    let budget = program.function(cold as usize).code.len();
    assert_eq!(program.function(hot as usize).code.len(), budget, "the callees cost alike");
    let planned = with_budget(&program, caller, program.function(caller as usize), budget, &targets, &body).unwrap();
    assert_eq!(planned.frames.iter().map(|frame| frame.target.function).collect::<Vec<_>>(), [hot], "the loop's site takes the budget");
    // With room for both, program order and weight agree: both inline.
    let planned = with_budget(&program, caller, program.function(caller as usize), 2 * budget, &targets, &body).unwrap();
    assert_eq!(planned.frames.len(), 2);
    // A measured loop that took one trip weighs no more than the site before it: the earlier site wins the tie.
    let planned = with_trips(&program, caller, program.function(caller as usize), budget, &targets, &|_, _| Some(1), &body).unwrap();
    assert_eq!(planned.frames.iter().map(|frame| frame.target.function).collect::<Vec<_>>(), [cold]);
}
