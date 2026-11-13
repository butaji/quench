use crate::Engine;

#[test]
fn a_replay_short_of_reserved_site_room_runs_unrewritten_code_and_later_replays_agree() {
    use crate::bytecode::append_vec::AppendVec;
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use std::{cell::RefCell, rc::Rc};
    let source = "function sum(o) { let t = 0; for (let i = 0; i < 4; i++) { t += o.a + o.b; o.a = o.b; } return t; }\n\
                  print(sum({ a: 1, b: 2 }) + ',' + sum({ a: 3, b: 4 }));";
    let normal = Engine::specialize(source, "short-room.js").unwrap();
    let mut short = Engine::specialize(source, "short-room.js").unwrap();
    // The tables keep no room past their entries, as a program whose load reserved less than a
    // replay may add.
    short.field_sites = AppendVec::with_headroom(short.field_sites.to_vec(), 0);
    short.superinstructions = AppendVec::with_headroom(short.superinstructions.to_vec(), 0);
    let id = short.functions.iter().position(|slot| slot.is_released()).expect("a released function") as u32;
    assert!(normal.functions[id as usize].is_released());
    let (fields, supers) = (short.field_sites.len(), short.superinstructions.len());
    let first = crate::compile::replay::recompile(&short, id).unwrap().function;
    let second = crate::compile::replay::recompile(&short, id).unwrap().function;
    assert!(matches!(short.rewrite_ranges.borrow()[id as usize], Some(None)), "the skip is recorded");
    assert_eq!((short.field_sites.len(), short.superinstructions.len()), (fields, supers), "no site was added");
    assert_eq!(format!("{first:?}"), format!("{second:?}"), "later replays agree");
    let run = |program: &crate::bytecode::ResidualProgram| {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(program).unwrap();
        output.take()
    };
    assert_eq!(run(&short), run(&normal));
}
