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
    assert!(matches!(short.rewrite_ranges.settled.borrow()[id as usize], Some(None)), "the skip is recorded");
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

/// Atoms numbered from zero, for programs that run specialized, as a host's do.
struct SpecializedAtoms;

impl crate::compile::AtomPrefix for SpecializedAtoms {
    fn len(&self) -> usize {
        0
    }
    fn name(&self, atom: crate::bytecode::Atom) -> &str {
        unreachable!("atom {atom} lies outside the empty prefix")
    }
    fn find(&self, _: &str) -> Option<crate::bytecode::Atom> {
        None
    }
    fn table(&self) -> crate::bytecode::AtomTable {
        crate::bytecode::AtomTable::default()
    }
    fn specialized(&self) -> bool {
        true
    }
}

fn eval_program(source: &str) -> crate::bytecode::ResidualProgram {
    // A caller's context makes the eval's functions keep their records.
    let context = crate::compile::EvalContext { in_function: true, ..Default::default() };
    Engine::specialize_eval_with_context(source, "eval-rewrites.js", &SpecializedAtoms, false, context, &[], &[]).unwrap()
}

#[test]
fn an_evals_kept_records_take_their_rewrites_on_first_read_as_the_load_would_have() {
    let sources = [
        "function sum(o) { let t = 0; for (let i = 0; i < 4; i++) { t += o.a + o.b; o.a = o.b; } return t; } function mix(p) { return p.x + p.y * 2; } sum({ a: 1, b: 2 }) + mix({ x: 1, y: 2 });",
        "var o = { m() { return this.v + this.w; }, v: 1, w: 2 }; var f = () => o.m() + o.v; f();",
    ];
    for source in sources {
        let deferred = eval_program(source);
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(true));
        let at_load = std::panic::catch_unwind(|| eval_program(source));
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(false));
        let at_load = at_load.unwrap();
        assert!(deferred.rewrite_ranges.kept_pending, "{source}");
        assert!(!at_load.rewrite_ranges.kept_pending, "{source}");
        at_load.prime_loop_decoding();
        for index in 0..deferred.functions.len() {
            assert_eq!(deferred.functions[index].record().is_some(), index == 0, "function {index} of {source}");
        }
        for index in 0..deferred.functions.len() {
            assert_eq!(format!("{:?}", deferred.function(index)), format!("{:?}", at_load.function(index)), "function {index} of {source}");
        }
        assert_eq!(deferred.field_sites.len(), at_load.field_sites.len(), "{source}");
        assert_eq!(deferred.superinstructions.len(), at_load.superinstructions.len(), "{source}");
    }
}

#[test]
fn an_evals_deferred_rewrites_stay_unrun_in_a_table_with_no_room() {
    use crate::bytecode::append_vec::AppendVec;
    let source = "function sum(o) { let t = 0; for (let i = 0; i < 4; i++) { t += o.a + o.b; o.a = o.b; } return t; } sum({ a: 1, b: 2 });";
    let mut short = eval_program(source);
    short.field_sites = AppendVec::with_headroom(short.field_sites.to_vec(), 0);
    short.superinstructions = AppendVec::with_headroom(short.superinstructions.to_vec(), 0);
    let (fields, supers) = (short.field_sites.len(), short.superinstructions.len());
    short.function(1);
    assert_eq!((short.field_sites.len(), short.superinstructions.len()), (fields, supers), "no site was added");
    assert!(matches!(short.rewrite_ranges.settled.borrow()[1], Some(None)), "the skip is recorded");
}

#[test]
fn an_eval_of_hot_code_runs_the_same_whether_its_rewrites_wait_for_first_reads_or_not() {
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use std::{cell::RefCell, rc::Rc};
    let source = "function t(local) { return eval('function P(x) { this.x = x; this.y = x + 1; } P.prototype.l = function () { return this.x * this.x + this.y; }; \
                  var s = 0; for (var i = 0; i < 20; i++) { s += new P(i).l(); } var f = (o) => o.x + o.y + local; s + f(new P(3)) + local;'); }\n\
                  print(t(1) + ',' + t(2));";
    let program = Engine::specialize(source, "eval-hot.js").unwrap();
    let run = |at_load: bool| {
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(at_load));
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let result = vm.execute(&program);
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(false));
        result.unwrap();
        output.take()
    };
    assert_eq!(run(false), run(true));
}
