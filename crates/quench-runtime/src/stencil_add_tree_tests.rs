use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::Value;

fn execute_source_add_chain(
    view: CodeView<'_>,
    inputs: [Value; 3],
) -> Option<(
    Completion,
    u64,
    Option<crate::stencil_select::PhysicalStencilView>,
    Option<crate::stencil_region_builder::NativeLinearWitness>,
    crate::stencil_plan::LocalNumericInputs,
)> {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let pc = (0..view.len()).find(|pc| has_add_tree(&plan, *pc))?;
    let native = plan.native_local_binary_at(pc)?;
    let selection = native.borrow().selection();
    let sources = match selection.inputs {
        crate::stencil_plan::LocalNumericInputs::AddChain { sources, .. } => sources,
        crate::stencil_plan::LocalNumericInputs::BinarySeries { sources, .. } => {
            [sources[0], sources[1], sources[1]]
        }
        _ => return None,
    };
    let environment = add_chain_environment(sources, inputs)?;
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    let (completion, _) = crate::vm::execute_baseline_code_from(
        view,
        &plan,
        0,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .ok()?;
    let entries = native.borrow().native_entry_count();
    let physical = native.borrow().last_native_view();
    let linear = native.borrow().last_linear_witness();
    Some((completion, entries, physical, linear, selection.inputs))
}

fn has_add_tree(plan: &BaselinePlan, pc: usize) -> bool {
    plan.native_local_binary_at(pc).is_some_and(|native| {
        matches!(
            native.borrow().selection().inputs,
            crate::stencil_plan::LocalNumericInputs::AddChain { .. }
                | crate::stencil_plan::LocalNumericInputs::BinarySeries { .. }
        )
    })
}

fn add_chain_environment(
    sources: [crate::stencil_plan::NumericSource; 3],
    inputs: [Value; 3],
) -> Option<std::rc::Rc<crate::environment::Environment>> {
    let environment = crate::environment::Environment::new();
    for (source, value) in sources.into_iter().zip(inputs) {
        let crate::stencil_plan::NumericSource::Local(slot) = source else {
            return None;
        };
        environment.set(slot, value);
    }
    Some(environment)
}

#[test]
fn ordinary_source_add_tree_executes_native_and_guarded_fallback() {
    let source = "function f(a,b,c){return (a+b)+c} f(1,2,4)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let Some(numeric) = execute_source_add_chain(view, numeric_inputs()) else {
            return;
        };
        assert_eq!(numeric.0, Completion::Return(Value::Number(7.0)));
        assert_eq!(numeric.1, 1);
        #[cfg(quench_generated_stencil_artifacts)]
        assert_generated_add_chain(numeric.2);
        let ordered = execute_source_add_chain(view, overflow_inputs()).expect("overflow case");
        assert_eq!(ordered.0, Completion::Return(Value::Number(f64::INFINITY)));
        assert_eq!(ordered.1, 1);
        let fallback = execute_source_add_chain(view, string_inputs()).expect("guard fallback");
        assert_eq!(fallback.0, Completion::Return(Value::String("x23".into())));
        assert_eq!(fallback.1, 0);
        assert!(fallback.2.is_none());
        checked = true;
    });
    assert!(
        checked,
        "lowered add tree must reach normal native admission"
    );
}

#[test]
fn ordinary_source_repeated_add_uses_composed_fragment_image() {
    let source = "function f(a,b){return (a+b)+b} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let Some((completion, entries, _, witness, _)) = execute_source_add_chain(
            view,
            [Value::Number(1.0), Value::Number(2.0), Value::Number(2.0)],
        ) else {
            return;
        };
        assert_eq!(completion, Completion::Return(Value::Number(5.0)));
        assert_eq!(entries, 1);
        let witness = witness.expect("composed image witness");
        assert_eq!(witness.fragments, 2);
        assert_eq!(
            witness.identity.abi,
            crate::stencil_select::RegionAbi::ScalarF64Binary
        );
        #[cfg(quench_generated_stencil_artifacts)]
        assert_eq!(witness.generated_fragments, 2);
        let fallback = execute_source_add_chain(
            view,
            [
                Value::String("x".into()),
                Value::Number(2.0),
                Value::Number(2.0),
            ],
        )
        .expect("hostile input takes ordinary path");
        assert_eq!(fallback.0, Completion::Return(Value::String("x22".into())));
        assert_eq!(fallback.1, 0);
        assert!(fallback.3.is_none());
        checked = true;
    });
    assert!(checked, "ordinary source must reach fragment composition");
}

#[test]
fn ordinary_source_mixed_add_sub_uses_one_heterogeneous_chain() {
    let source = "function f(a,b){return ((a+b)-b)+b} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        checked |= check_mixed_chain(view);
    });
    assert!(
        checked,
        "ordinary source must execute the mixed fragment chain"
    );
}

fn check_mixed_chain(view: crate::machine::CodeView<'_>) -> bool {
    let values = [Value::Number(1.0), Value::Number(2.0), Value::Number(2.0)];
    let Some((completion, entries, _, witness, inputs)) = execute_source_add_chain(view, values)
    else {
        return false;
    };
    let crate::stencil_plan::LocalNumericInputs::BinarySeries { series, .. } = inputs else {
        return false;
    };
    assert_eq!(
        series.operations().collect::<Vec<_>>(),
        [
            crate::ops::BinaryOp::Add,
            crate::ops::BinaryOp::Subtract,
            crate::ops::BinaryOp::Add,
        ]
    );
    assert_eq!(completion, Completion::Return(Value::Number(3.0)));
    assert_eq!(entries, 1);
    let witness = witness.expect("heterogeneous composed image witness");
    assert_eq!(witness.fragments, 3);
    #[cfg(quench_generated_stencil_artifacts)]
    assert_eq!(witness.generated_fragments, 3);
    assert_mixed_chain_edges(view);
    true
}

fn assert_mixed_chain_edges(view: crate::machine::CodeView<'_>) {
    let fallback = execute_source_add_chain(
        view,
        [
            Value::String("x".into()),
            Value::Number(2.0),
            Value::Number(2.0),
        ],
    )
    .expect("observable coercion uses ordinary execution");
    let Completion::Return(Value::Number(value)) = fallback.0 else {
        panic!("ordinary numeric completion");
    };
    assert!(value.is_nan());
    assert_eq!(fallback.1, 0);
    assert!(fallback.3.is_none());
    let ordered = execute_source_add_chain(
        view,
        [
            Value::Number(1.0e308),
            Value::Number(-1.0e308),
            Value::Number(-1.0e308),
        ],
    )
    .expect("mixed series preserves operation order");
    assert_eq!(ordered.0, Completion::Return(Value::Number(0.0)));
    assert_eq!(ordered.1, 1);
    assert!(ordered.3.is_some());
}

#[test]
fn ordinary_source_add_mul_div_uses_one_heterogeneous_chain() {
    let source = "function f(a,b){return ((a+b)*b)/b} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        checked |= check_mul_div_chain(view);
    });
    assert!(checked, "ordinary source must execute the arithmetic chain");
}

fn check_mul_div_chain(view: crate::machine::CodeView<'_>) -> bool {
    let values = [Value::Number(1.0), Value::Number(2.0), Value::Number(2.0)];
    let Some((completion, entries, _, witness, inputs)) = execute_source_add_chain(view, values)
    else {
        return false;
    };
    let crate::stencil_plan::LocalNumericInputs::BinarySeries { series, .. } = inputs else {
        return false;
    };
    assert_eq!(
        series.operations().collect::<Vec<_>>(),
        [
            crate::ops::BinaryOp::Add,
            crate::ops::BinaryOp::Multiply,
            crate::ops::BinaryOp::Divide,
        ]
    );
    assert_eq!(completion, Completion::Return(Value::Number(3.0)));
    assert_eq!(entries, 1);
    let witness = witness.expect("three-fragment arithmetic image");
    assert_eq!(witness.fragments, 3);
    #[cfg(quench_generated_stencil_artifacts)]
    assert_eq!(witness.generated_fragments, 3);
    assert_mul_div_edges(view);
    true
}

fn assert_mul_div_edges(view: crate::machine::CodeView<'_>) {
    let zero = [Value::Number(1.0), Value::Number(0.0), Value::Number(0.0)];
    let result = execute_source_add_chain(view, zero).expect("zero divisor remains native");
    let Completion::Return(Value::Number(value)) = result.0 else {
        panic!("numeric completion");
    };
    assert!(value.is_nan());
    assert_eq!(result.1, 1);
    let coercive = [
        Value::String("x".into()),
        Value::Number(2.0),
        Value::Number(2.0),
    ];
    let fallback = execute_source_add_chain(view, coercive).expect("coercive fallback");
    let Completion::Return(Value::Number(value)) = fallback.0 else {
        panic!("coercive completion");
    };
    assert!(value.is_nan());
    assert_eq!(fallback.1, 0);
    assert!(fallback.3.is_none());
}

#[test]
fn ordinary_source_repeated_add_depth_is_data_not_a_new_plan() {
    let source = "function f(a,b){return (((a+b)+b)+b)+b} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let Some((completion, entries, _, witness, _)) = execute_source_add_chain(
            view,
            [Value::Number(1.0), Value::Number(2.0), Value::Number(2.0)],
        ) else {
            return;
        };
        assert_eq!(completion, Completion::Return(Value::Number(9.0)));
        assert_eq!(entries, 1);
        assert_eq!(witness.expect("composed image").fragments, 4);
        let negative_zero = execute_source_add_chain(
            view,
            [
                Value::Number(-0.0),
                Value::Number(-0.0),
                Value::Number(-0.0),
            ],
        )
        .expect("ordered signed-zero execution");
        let Completion::Return(Value::Number(value)) = negative_zero.0 else {
            panic!("numeric completion");
        };
        assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
        checked = true;
    });
    assert!(
        checked,
        "ordinary source must admit variable-depth composition"
    );
}

#[cfg(quench_generated_stencil_artifacts)]
fn assert_generated_add_chain(view: Option<crate::stencil_select::PhysicalStencilView>) {
    let view = view.expect("normal driver must retain the invoked physical view");
    assert!(view.generated);
    assert_eq!(view.key, crate::stencil_select::add_chain_region_key());
    assert_eq!(view.abi, crate::stencil_select::RegionAbi::ScalarF64x3);
    assert!(view.fallthrough.is_some());
}

fn numeric_inputs() -> [Value; 3] {
    [Value::Number(1.0), Value::Number(2.0), Value::Number(4.0)]
}

fn overflow_inputs() -> [Value; 3] {
    [
        Value::Number(f64::MAX),
        Value::Number(f64::MAX),
        Value::Number(-f64::MAX),
    ]
}

fn string_inputs() -> [Value; 3] {
    [
        Value::String("x".into()),
        Value::Number(2.0),
        Value::Number(3.0),
    ]
}
