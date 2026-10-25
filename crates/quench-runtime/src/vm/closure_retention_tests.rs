//! What a closure keeps alive. A closure must keep every binding its code (or code nested
//! in it) can read, and nothing else: not a binding only a sibling closure reads, not a local
//! no closure reads, not its defining activation's `arguments` or receiver unless it can
//! observe them. Direct `eval` and `with` can name any binding, so a closure that runs them
//! keeps its whole scope chain.
//!
//! Each case runs as a script that stores the closure it keeps in the global `kept`, and a
//! `WeakRef` to each probed object in the global `weak` under the case's label. After the
//! script, a full collection runs with the kept objects of its job released; each probed
//! object must then be alive exactly when the case expects it. `release()` drops the
//! closure; after another collection every probed object must be gone.

use super::Vm;
use crate::test_host::TestHost;
use crate::{Engine, Value};

/// One retention case: the script, and each probed label with whether the kept closure must
/// keep that object alive.
struct Case {
    name: &'static str,
    source: &'static str,
    probes: &'static [(&'static str, bool)],
    /// Whether the case also runs `HOT_RUNS` times; the exact user example allocates a
    /// million elements per run and runs once.
    hot: bool,
}

const HARNESS: &str = "var weak = {}, kept = null;\nfunction release() { kept = null; }\n";

/// Runs `make` past the loop tier-up threshold before the final run, so the closures are made
/// and their activations unwound by tiered code too.
const HOT_RUNS: u32 = 2 * super::dispatch_fast::LOOP_TIER_THRESHOLD;

const CASES: &[Case] = &[
    Case {
        name: "user_example",
        source: r#"
            function heavyTask() {
              const massiveData = new Array(1_000_000).fill("data");
              weak.massiveData = new WeakRef(massiveData);
              return function () { return massiveData.length; };
            }
            function make() { return heavyTask(); }
        "#,
        probes: &[("massiveData", true)],
        hot: false,
    },
    Case {
        name: "user_example_tiered",
        source: r#"
            function heavyTask() {
              const massiveData = new Array(1_000).fill("data");
              weak.massiveData = new WeakRef(massiveData);
              return function () { return massiveData.length; };
            }
            function make() { return heavyTask(); }
        "#,
        probes: &[("massiveData", true)],
        hot: true,
    },
    Case {
        name: "sibling_capture",
        source: r#"
            function make() {
              const big = {tag: 'sibling'}; weak.big = new WeakRef(big);
              const sibling = () => big.tag; sibling();
              return () => 1;
            }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "sibling_capture_kept_binding",
        source: r#"
            function make() {
              const big = {tag: 'other'}; weak.big = new WeakRef(big);
              const used = {tag: 'used'}; weak.used = new WeakRef(used);
              const sibling = () => big.tag; sibling();
              return () => used.tag;
            }
        "#,
        probes: &[("big", false), ("used", true)],
        hot: true,
    },
    Case {
        name: "uncaptured_local",
        source: r#"
            function make() {
              const big = {tag: 'local'}; weak.big = new WeakRef(big);
              return () => 1;
            }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "uncaptured_parameter",
        source: r#"
            function inner(value) { return () => 1; }
            function make() { const big = {tag: 'parameter'}; weak.big = new WeakRef(big); return inner(big); }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "arguments_captured_by_sibling",
        source: r#"
            function inner(value) { const sibling = () => arguments[0]; sibling(); return () => 1; }
            function make() { const big = {tag: 'arguments'}; weak.big = new WeakRef(big); return inner(big); }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "arguments_read_by_closure",
        source: r#"
            function inner(value) { return () => arguments.length; }
            function make() { const big = {tag: 'read'}; weak.big = new WeakRef(big); return inner(big); }
        "#,
        probes: &[("big", true)],
        hot: true,
    },
    Case {
        name: "receiver_of_non_arrow_closure",
        source: r#"
            function inner() { const sibling = () => this; sibling(); return function () { return 1; }; }
            function make() { const big = {tag: 'receiver'}; weak.big = new WeakRef(big); return inner.call(big); }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "receiver_read_by_arrow",
        source: r#"
            function inner() { return () => this.tag; }
            function make() { const big = {tag: 'arrow'}; weak.big = new WeakRef(big); return inner.call(big); }
        "#,
        probes: &[("big", true)],
        hot: true,
    },
    Case {
        name: "block_sibling",
        source: r#"
            function make() {
              const keep = 1;
              { const big = {tag: 'block'}; weak.big = new WeakRef(big); const sibling = () => big; sibling(); }
              return () => keep;
            }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "loop_body_siblings",
        source: r#"
            function make() {
              let count = 0;
              for (let i = 0; i < 3; i++) {
                const big = {tag: 'loop' + i}; weak['big' + i] = new WeakRef(big);
                const sibling = () => big; sibling(); count++;
              }
              return () => count;
            }
        "#,
        probes: &[("big0", false), ("big1", false), ("big2", false)],
        hot: true,
    },
    Case {
        name: "loop_body_keeps_its_iteration",
        source: r#"
            function make() {
              let chosen;
              for (let i = 0; i < 3; i++) {
                const big = {tag: 'iteration' + i}; weak['big' + i] = new WeakRef(big);
                const closure = () => big; if (i === 1) chosen = closure;
              }
              return chosen;
            }
        "#,
        probes: &[("big0", false), ("big1", true), ("big2", false)],
        hot: true,
    },
    Case {
        name: "loop_iteration_shares_outer_binding",
        source: r#"
            function make() {
              const outer = {tag: 'outer'}; weak.outer = new WeakRef(outer);
              const other = {tag: 'other'}; weak.other = new WeakRef(other);
              let chosen;
              for (let i = 0; i < 3; i++) {
                const big = {tag: 'iteration' + i}; weak['big' + i] = new WeakRef(big);
                const sibling = () => other; sibling();
                const closure = () => big.tag + outer.tag; if (i === 2) chosen = closure;
              }
              if (chosen() !== 'iteration2outer') throw new Error('loop closure');
              return chosen;
            }
        "#,
        probes: &[
            ("outer", true),
            ("other", false),
            ("big0", false),
            ("big1", false),
            ("big2", true),
        ],
        hot: true,
    },
    Case {
        name: "catch_clause_sibling",
        source: r#"
            function make() {
              const big = {tag: 'catch'}; weak.big = new WeakRef(big);
              try { throw big; } catch (error) { const sibling = () => error; sibling(); }
              return () => 1;
            }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "nested_closure_keeps_outer_binding",
        source: r#"
            function make() {
              const outer = {tag: 'outer'}; weak.outer = new WeakRef(outer);
              const other = {tag: 'other'}; weak.other = new WeakRef(other);
              const sibling = () => other; sibling();
              return function middle() { return () => outer.tag; };
            }
        "#,
        probes: &[("outer", true), ("other", false)],
        hot: true,
    },
    Case {
        name: "generator_yielded_closure",
        source: r#"
            function* generator() { const big = {tag: 'generator'}; weak.big = new WeakRef(big); yield () => 1; }
            function make() { const iterator = generator(); const closure = iterator.next().value; iterator.next(); return closure; }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "async_function_closure",
        source: r#"
            async function body(out) { const big = {tag: 'async'}; weak.big = new WeakRef(big); out.closure = () => 1; }
            function make() { const out = {}; body(out); return out.closure; }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "class_constructor_closure",
        source: r#"
            class Holder { constructor() { const big = {tag: 'constructor'}; weak.big = new WeakRef(big); this.closure = () => 1; } }
            function make() { return new Holder().closure; }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "class_method_closure",
        source: r#"
            class Holder { method() { const big = {tag: 'method'}; weak.big = new WeakRef(big); return () => 1; } }
            function make() { return new Holder().method(); }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "class_method_super_home",
        source: r#"
            class Base { tag() { return 'base'; } }
            class Derived extends Base {
              method() {
                const big = {tag: 'home'}; weak.big = new WeakRef(big);
                const sibling = () => big; sibling();
                return () => super.tag();
              }
            }
            function make() { const closure = new Derived().method(); if (closure() !== 'base') throw new Error('super'); return closure; }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "class_private_field_closure",
        source: r#"
            class Holder {
              #secret = {tag: 'secret'};
              method() {
                weak.secret = new WeakRef(this.#secret);
                const big = {tag: 'private'}; weak.big = new WeakRef(big);
                const sibling = () => big; sibling();
                const self = this;
                return () => self.#secret.tag;
              }
            }
            function make() { const closure = new Holder().method(); if (closure() !== 'secret') throw new Error('private'); return closure; }
        "#,
        probes: &[("secret", true), ("big", false)],
        hot: true,
    },
    Case {
        name: "class_field_initializer_reads_enclosing",
        source: r#"
            function make() {
              const seed = {tag: 'seed'}; weak.seed = new WeakRef(seed);
              const big = {tag: 'field'}; weak.big = new WeakRef(big);
              const sibling = () => big; sibling();
              // The constructor runs its field initializer on its own environment.
              class Holder { field = seed.tag; }
              return () => new Holder().field;
            }
        "#,
        probes: &[("seed", true), ("big", false)],
        hot: true,
    },
    Case {
        name: "new_target_read_by_arrow",
        source: r#"
            function Maker() {
              const big = {tag: 'new.target'}; weak.big = new WeakRef(big);
              const sibling = () => big; sibling();
              this.closure = () => new.target;
            }
            function make() { const closure = new Maker().closure; if (closure() !== Maker) throw new Error('new.target'); return closure; }
        "#,
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "global_var_reassigned_by_function",
        source: r#"
            // The script's own assignment leaves a copy in its root environment, which the
            // kept closure's scope chain reaches; reads of a global var go to the global object.
            var holder = {tag: 'global'}; weak.holder = new WeakRef(holder);
            function make() { holder = null; return () => holder; }
        "#,
        probes: &[("holder", false)],
        hot: true,
    },
    Case {
        name: "direct_eval_keeps_scope",
        source: r#"
            function make() { const big = {tag: 'eval'}; weak.big = new WeakRef(big); return () => eval('big'); }
        "#,
        probes: &[("big", true)],
        hot: true,
    },
    Case {
        name: "sibling_direct_eval_keeps_scope",
        source: r#"
            function make() {
              const big = {tag: 'eval sibling'}; weak.big = new WeakRef(big);
              const sibling = () => eval('1'); sibling();
              return () => 1;
            }
        "#,
        // The sibling's direct eval may name `big`, but only while the sibling lives; the
        // kept closure runs no eval and names nothing.
        probes: &[("big", false)],
        hot: true,
    },
    Case {
        name: "with_keeps_scope",
        source: r#"
            function make() {
              const big = {tag: 'with'}; weak.big = new WeakRef(big); const object = {};
              return function () { with (object) { return big; } };
            }
        "#,
        probes: &[("big", true)],
        hot: true,
    },
    Case {
        name: "many_closures_over_one_scope",
        source: r#"
            function make() {
              const a = {tag: 'a'}; weak.a = new WeakRef(a);
              const b = {tag: 'b'}; weak.b = new WeakRef(b);
              const c = {tag: 'c'}; weak.c = new WeakRef(c);
              const big = {tag: 'unread'}; weak.big = new WeakRef(big);
              const sibling = () => big; sibling();
              const readers = [() => a.tag, () => b.tag, () => c.tag, () => a.tag + b.tag, () => b.tag + c.tag, () => c.tag + a.tag];
              return () => readers.map(read => read()).join();
            }
        "#,
        // Seven kept closures of distinct functions reach `make`'s environment: their demands
        // cost more than tracing its slots whole (`PartialEnvironment::whole_cost`), so the
        // trace reads it whole and keeps `big` while they live, a bounded loss of precision.
        probes: &[("a", true), ("b", true), ("c", true), ("big", true)],
        hot: true,
    },
];

/// The compilations the interpreter, specialized and native tiers share.
fn compilations(source: &str, name: &str) -> [(&'static str, crate::bytecode::ResidualProgram); 2] {
    [
        ("specialized", Engine::specialize(source, name).unwrap()),
        (
            "unspecialized",
            Engine::specialize_unspecialized(source, name).unwrap(),
        ),
    ]
}

/// Whether the object `weak[label]` refers to survived the last collection.
fn probe_alive(
    vm: &mut Vm<TestHost>,
    program: &crate::bytecode::ResidualProgram,
    label: &str,
) -> bool {
    let weak = vm.intern_atom("weak");
    let weak = vm.own_property(vm.realm.globals, weak).unwrap();
    let label_atom = vm.intern_atom(label);
    let reference = vm.get_property(program, weak, label_atom).unwrap();
    match vm.heap.get(reference) {
        Some(super::Cell::WeakRef { target, .. }) => {
            target.is_some_and(|target| vm.heap.weak_value(target).is_some())
        }
        _ => panic!("weak.{label} is no WeakRef"),
    }
}

/// A full collection after the script's job: its WeakRef targets are released first.
fn collect(vm: &mut Vm<TestHost>, program: &crate::bytecode::ResidualProgram) {
    vm.clear_kept_objects();
    vm.collect_now(program);
}

fn check_case(case: &Case, runs: u32) {
    let source = format!(
        "{HARNESS}{}\nfor (let run = 0; run < {runs}; run++) kept = make();\nif (typeof kept !== 'function') throw new Error('no closure');\n",
        case.source
    );
    for (compilation, program) in compilations(&source, case.name) {
        let mut vm = Vm::new(TestHost::default().with_test262());
        vm.execute(&program).unwrap();
        collect(&mut vm, &program);
        for &(label, retained) in case.probes {
            assert_eq!(
                probe_alive(&mut vm, &program, label),
                retained,
                "{} ({compilation}, {runs} runs): while the closure lives, {label} {}",
                case.name,
                if retained {
                    "must stay alive"
                } else {
                    "must be collected"
                },
            );
        }
        // The kept closure still runs after the collection cleared what it cannot read.
        let kept = vm.intern_atom("kept");
        let kept = vm.own_property(vm.realm.globals, kept).unwrap();
        vm.call_value(&program, kept, Value::UNDEFINED, &[])
            .unwrap();
        let release = vm.intern_atom("release");
        let release = vm.own_property(vm.realm.globals, release).unwrap();
        vm.call_value(&program, release, Value::UNDEFINED, &[])
            .unwrap();
        collect(&mut vm, &program);
        for &(label, _) in case.probes {
            assert!(
                !probe_alive(&mut vm, &program, label),
                "{} ({compilation}, {runs} runs): {label} must be collected once the closure is dropped",
                case.name,
            );
        }
    }
}

macro_rules! retention_tests {
    ($($test:ident => $name:literal),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                let case = CASES.iter().find(|case| case.name == $name).expect("a listed case");
                check_case(case, 1);
                if case.hot {
                    check_case(case, HOT_RUNS);
                }
            }
        )*

        #[test]
        fn closure_retention_cases_are_all_tested() {
            let tested = [$($name),*];
            for case in CASES {
                assert!(tested.contains(&case.name), "{} has no test", case.name);
            }
        }
    };
}

retention_tests! {
    closure_retention_user_example => "user_example",
    closure_retention_user_example_tiered => "user_example_tiered",
    closure_retention_sibling_capture => "sibling_capture",
    closure_retention_sibling_capture_kept_binding => "sibling_capture_kept_binding",
    closure_retention_uncaptured_local => "uncaptured_local",
    closure_retention_uncaptured_parameter => "uncaptured_parameter",
    closure_retention_arguments_captured_by_sibling => "arguments_captured_by_sibling",
    closure_retention_arguments_read_by_closure => "arguments_read_by_closure",
    closure_retention_receiver_of_non_arrow_closure => "receiver_of_non_arrow_closure",
    closure_retention_receiver_read_by_arrow => "receiver_read_by_arrow",
    closure_retention_block_sibling => "block_sibling",
    closure_retention_loop_body_siblings => "loop_body_siblings",
    closure_retention_loop_body_keeps_its_iteration => "loop_body_keeps_its_iteration",
    closure_retention_loop_iteration_shares_outer_binding => "loop_iteration_shares_outer_binding",
    closure_retention_catch_clause_sibling => "catch_clause_sibling",
    closure_retention_nested_closure_keeps_outer_binding => "nested_closure_keeps_outer_binding",
    closure_retention_generator_yielded_closure => "generator_yielded_closure",
    closure_retention_async_function_closure => "async_function_closure",
    closure_retention_class_constructor_closure => "class_constructor_closure",
    closure_retention_class_method_closure => "class_method_closure",
    closure_retention_class_method_super_home => "class_method_super_home",
    closure_retention_class_private_field_closure => "class_private_field_closure",
    closure_retention_class_field_initializer_reads_enclosing => "class_field_initializer_reads_enclosing",
    closure_retention_new_target_read_by_arrow => "new_target_read_by_arrow",
    closure_retention_global_var_reassigned_by_function => "global_var_reassigned_by_function",
    closure_retention_direct_eval_keeps_scope => "direct_eval_keeps_scope",
    closure_retention_sibling_direct_eval_keeps_scope => "sibling_direct_eval_keeps_scope",
    closure_retention_with_keeps_scope => "with_keeps_scope",
    closure_retention_many_closures_over_one_scope => "many_closures_over_one_scope",
}
