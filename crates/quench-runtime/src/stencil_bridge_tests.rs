use crate::ops::{HostCapabilityKind, HostCapabilityRef, Op, RealmId};
use crate::stencil_fact::PatchValues;
use crate::stencil_select::{RegionAbi, RenderedRegionCache};
use crate::value::{FunctionValue, Value};
use crate::vm::{Host, VmContext, VmError};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

const REENTER: HostCapabilityKind = HostCapabilityKind::Custom(0x751);

struct ReentrantHost {
    pool: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    root: Weak<FunctionValue>,
    effects: Cell<u32>,
    fail: Cell<bool>,
}

struct ArgumentRootHost {
    roots: [Weak<FunctionValue>; 2],
    effects: Cell<u32>,
}

impl Host for ArgumentRootHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        assert_eq!(capability.kind, REENTER);
        assert!(matches!(receiver, Some(Value::Function(_))));
        assert!(matches!(arguments, [Value::Function(_)]));
        self.effects.set(self.effects.get() + 1);
        crate::cycle_collector::collect_cycles();
        for root in &self.roots {
            assert_root_survives(root)?;
        }
        Ok(Value::Number(9.0))
    }
}

impl Host for ReentrantHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        _receiver: Option<&Value>,
        _arguments: &[Value],
    ) -> Result<Value, VmError> {
        assert_eq!(capability.kind, REENTER);
        self.effects.set(self.effects.get() + 1);
        assert_eq!(
            self.pool.borrow().active_leases(),
            0,
            "the native lease must end before a reentrant helper"
        );
        let nested = install_nested_add(&self.pool)?;
        assert_eq!(self.pool.borrow().active_leases(), 1);
        assert_eq!(
            self.pool.borrow_mut().evict_idle(0),
            1,
            "the idle outer entry should be reclaimable while the nested lease stays live"
        );
        let sum = nested
            .invoke(|entry| entry(2.0, 3.0))
            .map_err(|_| VmError::EvalError("nested native entry failed".into()))?;
        crate::cycle_collector::collect_cycles();
        assert_root_survives(&self.root)?;
        if self.fail.get() {
            return Err(VmError::EvalError("host failure after effect".into()));
        }
        Ok(Value::Number(sum))
    }
}

fn install_nested_add(
    pool: &Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
) -> Result<crate::stencil_arena::OwnedLease<extern "C" fn(f64, f64) -> f64>, VmError> {
    let key = crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add)
        .ok_or_else(|| VmError::EvalError("missing add region".into()))?;
    let view = crate::stencil_select::select_physical_for_abi(key, RegionAbi::ScalarF64Binary)
        .ok_or_else(|| VmError::EvalError("missing add view".into()))?;
    let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
    let address = {
        let mut slab = pool.borrow_mut();
        let mut cache = RenderedRegionCache::new();
        let address = slab
            .render_physical_view_or_get(&mut cache, view, &PatchValues::from_site(&site))
            .map_err(arena_error)?;
        slab.make_executable(address).map_err(arena_error)?;
        address
    };
    let token = pool
        .borrow()
        .owned_f64_entry(address)
        .map_err(arena_error)?;
    crate::stencil_arena::SharedStencilSlab::acquire_owned(pool, token).map_err(arena_error)
}

fn arena_error(error: crate::stencil_arena::ArenaError) -> VmError {
    VmError::EvalError(format!("stencil arena failure: {error:?}"))
}

fn assert_root_survives(root: &Weak<FunctionValue>) -> Result<(), VmError> {
    let function = root
        .upgrade()
        .ok_or_else(|| VmError::EvalError("caller root was reclaimed".into()))?;
    if matches!(function.captures.get(0), Value::Function(_)) {
        Ok(())
    } else {
        Err(VmError::EvalError("caller capture was cleared".into()))
    }
}

fn bridge_fixture(
    fail: bool,
) -> (
    crate::machine::ExecutableCode,
    crate::machine::BaselinePlan,
    Rc<ReentrantHost>,
    crate::register_file::RegisterFile,
) {
    let executable = crate::machine::ExecutableCode::from_ops(vec![
        Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        },
        Op::Return { src: 0 },
    ]);
    let plan = crate::machine::BaselinePlan::compile_for_test(
        executable.code(),
        crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
    );
    let (root, weak) = crate::stencil_test_support::cyclic_function_root();
    let host = Rc::new(ReentrantHost {
        pool: plan.shared_stencil_pool_for_test(),
        root: weak,
        effects: Cell::new(0),
        fail: Cell::new(fail),
    });
    let callable = crate::host_api::custom_function(RealmId::ROOT, 0x751);
    let registers =
        crate::register_file::RegisterFile::from_values(vec![Value::Undefined, callable, root]);
    (executable, plan, host, registers)
}

fn argument_boundary_fixture() -> (
    crate::machine::ExecutableCode,
    crate::machine::BaselinePlan,
    Rc<ArgumentRootHost>,
    crate::register_file::RegisterFile,
) {
    let executable = crate::machine::ExecutableCode::from_ops(vec![
        Op::Call {
            dst: 0,
            callee: 1,
            receiver: Some(2),
            args: vec![3],
            spreads: vec![false],
        },
        Op::Return { src: 0 },
    ]);
    let plan = crate::machine::BaselinePlan::compile_for_test(
        executable.code(),
        crate::stencil_policy::ExecutionPolicy::bridge_opt_in_for_test(),
    );
    let (receiver, receiver_root) = crate::stencil_test_support::cyclic_function_root();
    let (argument, argument_root) = crate::stencil_test_support::cyclic_function_root();
    let host = Rc::new(ArgumentRootHost {
        roots: [receiver_root, argument_root],
        effects: Cell::new(0),
    });
    let callable = crate::host_api::custom_function(RealmId::ROOT, 0x751);
    let registers = crate::register_file::RegisterFile::from_values(vec![
        Value::Undefined,
        callable,
        receiver,
        argument,
    ]);
    (executable, plan, host, registers)
}

fn execute_bridge_call(
    executable: &crate::machine::ExecutableCode,
    plan: &crate::machine::BaselinePlan,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    crate::vm::with_current_context(context, || {
        execute_bridge_call_in_context(executable, plan, registers, context)
    })
}

fn execute_bridge_call_in_context(
    executable: &crate::machine::ExecutableCode,
    plan: &crate::machine::BaselinePlan,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let environment = crate::environment::Environment::new();
    let _guard = crate::locals::EnvironmentGuard::install(Rc::clone(&environment));
    let (completion, next) = crate::vm::execute_baseline_code_from(
        executable.code(),
        plan,
        0,
        registers,
        context,
        Rc::clone(&environment),
    )?;
    let crate::completion::Completion::Call(call) = completion else {
        return Ok((completion, next));
    };
    crate::vm::vm_ops::execute_call_continuation(registers, call)?;
    crate::vm::execute_baseline_code_from(
        executable.code(),
        plan,
        next,
        registers,
        context,
        environment,
    )
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn baseline_bridge_exits_before_reentrant_helper_and_preserves_roots() {
    let (executable, plan, host, mut registers) = bridge_fixture(false);
    let context = VmContext::default().with_host(host.clone());
    let result = execute_bridge_call(&executable, &plan, &mut registers, &context)
        .expect("bridge call completes");
    assert_eq!(
        result.0,
        crate::completion::Completion::Return(Value::Number(5.0))
    );
    assert_eq!(host.effects.get(), 1);
    assert_eq!(host.pool.borrow().active_leases(), 0);
    let region = plan.native_region_at(0).expect("bridge admission").borrow();
    assert_eq!(region.physical_entry_count_for_test(), 1);
    assert_eq!(
        region
            .last_native_view_for_test()
            .expect("physical witness")
            .abi,
        RegionAbi::Bridge
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn baseline_bridge_semantic_throw_is_not_replayed_or_retired() {
    let (executable, plan, host, mut registers) = bridge_fixture(true);
    let initial_registers = registers.clone();
    let context = VmContext::default().with_host(host.clone());
    let result = execute_bridge_call(&executable, &plan, &mut registers, &context);
    assert!(
        matches!(result, Err(VmError::EvalError(message)) if message == "host failure after effect")
    );
    assert_eq!(host.effects.get(), 1, "semantic effects must not replay");
    host.fail.set(false);
    registers = initial_registers;
    let retry = execute_bridge_call(&executable, &plan, &mut registers, &context)
        .expect("the valid physical entry remains installed after a semantic throw");
    assert_eq!(
        retry.0,
        crate::completion::Completion::Return(Value::Number(5.0))
    );
    assert_eq!(host.effects.get(), 2);
    let region = plan.native_region_at(0).expect("bridge admission").borrow();
    assert_eq!(region.physical_entry_count_for_test(), 2);
    assert_eq!(
        region
            .last_native_view_for_test()
            .expect("physical witness")
            .abi,
        RegionAbi::Bridge
    );
}

#[test]
fn unsupported_native_call_shape_roots_receiver_and_argument_at_boundary() {
    let (executable, plan, host, mut registers) = argument_boundary_fixture();
    assert!(plan.native_region_at(0).is_none());
    let context = VmContext::default().with_host(host.clone());
    let result = execute_bridge_call(&executable, &plan, &mut registers, &context)
        .expect("ordinary call boundary completes");
    assert_eq!(
        result.0,
        crate::completion::Completion::Return(Value::Number(9.0))
    );
    assert_eq!(host.effects.get(), 1);
}
