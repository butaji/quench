use crate::completion::Completion;
use crate::ops::{BinaryOp, HostCapabilityKind, HostCapabilityRef, Op, RealmId};
use crate::value::Value;
use crate::vm::{Host, VmError};
use std::cell::Cell;

const ROOT_CHECK: HostCapabilityKind = HostCapabilityKind::Custom(0x752);

struct RootCheckingHost {
    root: std::rc::Weak<crate::value::FunctionValue>,
    effects: Cell<u32>,
}

impl Host for RootCheckingHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        _receiver: Option<&Value>,
        _arguments: &[Value],
    ) -> Result<Value, VmError> {
        assert_eq!(capability.kind, ROOT_CHECK);
        self.effects.set(self.effects.get() + 1);
        crate::cycle_collector::collect_cycles();
        let root = self.root.upgrade().expect("pending throw root survives");
        assert!(matches!(root.captures.get(0), Value::Function(_)));
        Ok(Value::Undefined)
    }
}

struct BoundaryFixture {
    body: crate::machine::FunctionCode,
    try_op: Op,
    registers: crate::register_file::RegisterFile,
}

fn add(dst: u16, lhs: u16, rhs: u16) -> Op {
    Op::Binary {
        dst,
        operator: BinaryOp::Add,
        lhs,
        rhs,
    }
}

fn fixture(lhs: Value, rhs: Value) -> BoundaryFixture {
    let body = crate::machine::FunctionCode::from_ops(vec![add(0, 1, 2), Op::Throw { src: 3 }]);
    let try_op = Op::Try {
        body: body.clone(),
        handler: None,
        finalizer: Some(crate::machine::FunctionCode::from_ops(vec![add(4, 4, 5)])),
        catch_slot: None,
        dst: 6,
        finally_dst: None,
    };
    let registers = crate::register_file::RegisterFile::from_values(vec![
        Value::Undefined,
        lhs,
        rhs,
        Value::String("boom".into()),
        Value::Number(0.0),
        Value::Number(1.0),
        Value::Undefined,
    ]);
    BoundaryFixture {
        body,
        try_op,
        registers,
    }
}

fn execute_body(fixture: &mut BoundaryFixture) -> (Completion, crate::machine::BaselinePlan) {
    let code = fixture.body.code().expect("try body code");
    let plan = crate::machine::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let context = crate::vm::VmContext::default();
    let environment = crate::environment::Environment::new();
    let _guard = crate::locals::EnvironmentGuard::install(environment.clone());
    let (completion, next) = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        0,
        &mut fixture.registers,
        &context,
        environment,
    )
    .expect("body reaches semantic throw");
    assert_eq!(next, 2);
    (completion, plan)
}

fn finish(fixture: &mut BoundaryFixture, completion: Completion) -> Completion {
    crate::exceptions::finish_try_completion(&mut fixture.registers, &fixture.try_op, completion)
        .expect("finally completes")
}

fn native_entries(plan: &crate::machine::BaselinePlan) -> u64 {
    plan.native_binary_at(0)
        .expect("native add admission")
        .borrow()
        .native_entry_count()
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn native_liveout_precedes_throw_and_finally_runs_once() {
    let mut fixture = fixture(Value::Number(2.0), Value::Number(3.0));
    let (completion, plan) = execute_body(&mut fixture);
    assert_eq!(completion, Completion::Throw(Value::String("boom".into())));
    assert_eq!(fixture.registers.read(0), Some(Value::Number(5.0)));
    assert_eq!(native_entries(&plan), 1, "native execution witness");
    let completion = finish(&mut fixture, completion);
    assert_eq!(completion, Completion::Throw(Value::String("boom".into())));
    assert_eq!(fixture.registers.read(0), Some(Value::Number(5.0)));
    assert_eq!(fixture.registers.read(4), Some(Value::Number(1.0)));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn failed_numeric_guard_falls_back_before_throw_and_finally() {
    let mut fixture = fixture(Value::String("2".into()), Value::Number(3.0));
    let (completion, plan) = execute_body(&mut fixture);
    assert_eq!(fixture.registers.read(0), Some(Value::String("23".into())));
    assert_eq!(native_entries(&plan), 0, "guard miss must precede entry");
    let completion = finish(&mut fixture, completion);
    assert_eq!(completion, Completion::Throw(Value::String("boom".into())));
    assert_eq!(fixture.registers.read(4), Some(Value::Number(1.0)));
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn pending_throw_roots_value_across_allocating_finalizer_boundary() {
    let (root, weak) = crate::stencil_test_support::cyclic_function_root();
    let mut fixture = fixture(Value::Number(2.0), Value::Number(3.0));
    fixture.registers.write(3, root);
    fixture
        .registers
        .write(7, crate::host_api::custom_function(RealmId::ROOT, 0x752));
    let Op::Try { finalizer, .. } = &mut fixture.try_op else {
        unreachable!()
    };
    *finalizer = Some(crate::machine::FunctionCode::from_ops(vec![Op::Call {
        dst: 8,
        callee: 7,
        receiver: None,
        args: Vec::new(),
        spreads: Vec::new(),
    }]));
    let (completion, plan) = execute_body(&mut fixture);
    fixture.registers.write(3, Value::Undefined);
    let host = std::rc::Rc::new(RootCheckingHost {
        root: weak,
        effects: Cell::new(0),
    });
    let context = crate::vm::VmContext::default().with_host(host.clone());
    let completion = crate::vm::with_current_context(&context, || finish(&mut fixture, completion));
    assert!(matches!(completion, Completion::Throw(Value::Function(_))));
    assert_eq!(native_entries(&plan), 1);
    assert_eq!(host.effects.get(), 1);
}
