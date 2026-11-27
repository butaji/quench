//! Constructions of a class whose constructors, up its chain of parents, only construct the
//! parent and initialize `this`.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{InitializerConstant, InitializerStore, InitializerValue, SuperForwarded};

/// The classes of a chain whose stores one construction makes; a longer chain constructs
/// through its bodies.
const SUPER_CHAIN_CLASSES: usize = 8;

/// The stores of a chain, the root's first, in the constructed class's parameters, each with
/// the program of its class, whose field caches its site indexes.
type ChainStores = SmallVec<[(ProgramId, InitializerStore); CONSTRUCT_PLAN_FIELDS]>;

/// What a class's parameters take: `None` for the constructed class, whose parameters are
/// the construction's arguments, else the values the class below passes to `super(...)`, in
/// that class's parameters.
type Passed<'a> = Option<&'a SuperArguments<'a>>;

struct SuperArguments<'a> {
    values: &'a [InitializerValue],
    /// The program of the class whose `super(...)` passes them, which their string constants index.
    program: ProgramId,
    below: Passed<'a>,
}

/// What parameter `parameter` of a class passed `passed` takes, in the constructed class's
/// parameters: `undefined` past the values. `None` for a string literal of another program than
/// `home`, the store's, whose constant index would name another string there.
fn passed_value(passed: Passed<'_>, parameter: u16, home: ProgramId) -> Option<InitializerValue> {
    let Some(arguments) = passed else {
        return Some(InitializerValue::Parameter { parameter, fallback: None });
    };
    match arguments.values.get(usize::from(parameter)) {
        None => Some(InitializerValue::Constant(InitializerConstant::Undefined)),
        Some(InitializerValue::Parameter { parameter, .. }) => passed_value(arguments.below, *parameter, home),
        Some(InitializerValue::Constant(InitializerConstant::String(_))) if arguments.program != home => None,
        Some(value) => Some(*value),
    }
}

/// `value` of a class passed `passed`, in the constructed class's parameters; `None` for a
/// default or a computation of anything but a parameter passed on as it is, which the
/// bodies make, and for a string literal of another program than `home`, the store's.
fn rebased(value: InitializerValue, passed: Passed<'_>, home: ProgramId) -> Option<InitializerValue> {
    let source = |parameter| match passed_value(passed, parameter, home)? {
        InitializerValue::Parameter { parameter, fallback: None } => Some(parameter),
        _ => None,
    };
    Some(match value {
        InitializerValue::Constant(_) | InitializerValue::Construct(_) => value,
        InitializerValue::Parameter { parameter, fallback: None } => passed_value(passed, parameter, home)?,
        InitializerValue::Parameter { parameter, fallback } => InitializerValue::Parameter { parameter: source(parameter)?, fallback },
        InitializerValue::Numeric { parameter, operator, constant } => {
            InitializerValue::Numeric { parameter: source(parameter)?, operator, constant }
        }
    })
}

/// Appends `own`, the stores of a class passed `passed`, rebased.
fn append(stores: &mut ChainStores, program: ProgramId, own: &[InitializerStore], passed: Passed<'_>) -> Option<()> {
    for store in own {
        stores.push((program, InitializerStore { value: rebased(store.value, passed, program)?, ..*store }));
    }
    Some(())
}

/// The stores [[Construct]] of `class`, passed `passed`, makes to the receiver, appended to
/// `stores` after its parent's, and the root constructor that allocates that receiver. Every
/// class of the chain is a function of the running realm, of any program: the root an in-place
/// constructor that only initializes `this` or does nothing, each other a derived
/// constructor that only passes its arguments on (`Function::super_forwarding`) or
/// constructs its parent and initializes `this` (`Function::super_initializer`), its parent
/// its own [[Prototype]] as `super(...)` reads it. `None` for any other chain.
fn chain_stores(
    vm: &Vm,
    class: Value,
    passed: Passed<'_>,
    stores: &mut ChainStores,
    classes: usize,
) -> Option<Value> {
    let record = vm.function_record(class)?;
    let FunctionKind::User(program, function) = &record.kind else {
        return None;
    };
    if vm.record_realm(record) != vm.realm.globals || classes == SUPER_CHAIN_CLASSES {
        return None;
    }
    // A class of another program than the running one builds a receiver other than the general
    // path's in WSL (each class file its own program; JetStream's WSL reads an undefined type
    // argument from the objects those constructions made), so its chain is left to the general
    // path until that divergence is found.
    let (program, code) = (*program, vm.programs.residual(*program).filter(|_| *program == vm.active_program)?);
    let body = code.function_get(*function as usize).filter(|_| code.kind != quench_runtime::bytecode::ProgramKind::Wasm)?;
    if !body.derived_constructor {
        let own = body.initializer_stores(&code.constants).or_else(|| body.only_returns_undefined(&code.constants).then_some(&[]));
        return append(stores, program, own.filter(|_| Vm::inline_constructible(body))?, passed).map(|()| class);
    }
    let parent = vm.heap_reads().object(class)?.proto();
    if body.super_forwarding(&code.atoms, &code.constants) == Some(SuperForwarded::Passed) {
        return chain_stores(vm, parent, passed, stores, classes + 1);
    }
    let initializer = body.super_initializer(&code.atoms, &code.constants)?;
    let arguments = SuperArguments { values: &initializer.arguments, program, below: passed };
    let below = if initializer.forwards { passed } else { Some(&arguments) };
    let root = chain_stores(vm, parent, below, stores, classes + 1)?;
    append(stores, program, &initializer.stores, passed).map(|()| root)
}

/// `stores` made to `this` as cached adds (`add_stores_in_lane`), each under its class's program,
/// whose field caches its site indexes; the program running before is active again after.
fn add_chain_stores(
    vm: &mut Vm,
    this: Value,
    stores: &[(ProgramId, InitializerStore)],
    argument: &impl Fn(u16) -> Value,
    each: &mut dyn FnMut(&InitializerStore, Value),
) -> bool {
    let running = vm.active_program;
    let mut added = true;
    for (program, store) in stores {
        if *program != vm.active_program {
            enter_program(vm, *program);
        }
        let Some(code) = vm.programs.residual(*program).map(std::ptr::from_ref) else {
            added = false;
            break;
        };
        // SAFETY: the program store keeps a residual for as long as the VM runs it.
        let code = unsafe { &*code };
        added = add_stores_in_lane(vm, code, this, std::slice::from_ref(store), argument, |_, _| None, &mut *each);
        if !added {
            break;
        }
    }
    if vm.active_program != running {
        enter_program(vm, running);
    }
    added
}

/// Makes `program` the VM's active one, with its constants.
fn enter_program(vm: &mut Vm, program: ProgramId) {
    vm.set_active_program(program);
}

/// Notes on `callee`'s plan, when the construction left one, the parent it holds with.
fn plan_super_parent(vm: &mut Vm, callee: Value) {
    if let Some(index) = construct_plan_index(callee)
        && vm.lane_stack.construct_plans[index].callee == callee
        && let Some(class) = vm.heap_reads().object(callee)
    {
        vm.lane_stack.construct_plans[index].super_parent = class.proto();
    }
}

/// `new callee(...)` of a derived class whose chain only initializes `this` (`chain_stores`):
/// the root's [[Construct]] with `callee` as `new.target`, whose receiver comes from `callee`'s
/// `prototype`, named by the root as the general path names it
/// (`Vm::inline_construct_template_by`), and takes the chain's stores as cached adds that run
/// no guest code (`initialize_stores_in_lane`), and which every body returns as it is. A plan
/// recorded under `callee` (`ConstructPlan::super_parent`) makes later constructions at once:
/// it holds while `callee`'s chain does, every parent class on it included, each the
/// [[Prototype]] of the class below it (`watch_construct_plan`). TypeScript's node
/// classes (`class extends Node { constructor(k, p, e) { super(k, p, e) } }`) and WSL's
/// (`constructor(origin, base) { super(origin); this.base = base; }`) otherwise left the lane
/// at every node. `None` leaves the construction to the general path: another program's or
/// realm's class, or an add the caches do not answer, whose partial receiver no code has seen.
#[inline(never)]
pub(crate) fn super_construction(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
    callee: Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some()) {
        return None;
    }
    if let Some(this) = holding_construct_plan(vm, callee)
        .filter(|plan| !plan.super_parent.is_undefined())
        .and_then(|_| planned_construction_in_lane(vm, callee, r, i))
    {
        return Some(this);
    }
    let mut stores = ChainStores::new();
    let root = chain_stores(vm, callee, None, &mut stores, 0).filter(|root| *root != callee)?;
    let argument = construct_argument_in_lane(r, i);
    let (this, added) = initialize_stores_in_lane(vm, callee, root, Value::UNDEFINED, |vm, this, _, each| add_chain_stores(vm, this, &stores, &argument, each))?;
    added.then(|| {
        plan_super_parent(vm, callee);
        this
    })
}

#[cfg(test)]
#[path = "../super_chain_tests.rs"]
pub(crate) mod tests;
