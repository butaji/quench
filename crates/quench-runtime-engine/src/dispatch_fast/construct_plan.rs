//! Construct plans: a constructor's receiver allocated finished, its initializer stores made at once.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::plan_fields::{field_value, FieldClass};
pub(crate) mod nested;
pub(crate) mod adds;
pub(crate) use adds::{add_initializer_fields_in_lane, add_stores_in_lane};
pub(crate) use nested::{finish_forwarded_in_lane, nested_construction, nested_root_holds};

pub(crate) use quench_runtime::vm::engine_types::ConstructPlan;

pub(crate) use quench_runtime::vm::engine_types::ConstructPlanKind;

#[quench_macros::ext(ConstructPlanOps1)]
impl ConstructPlan {
    /// This plan, with its receiver header derived from its prototype and shape; it holds
    /// under the dependency `watch_construct_plan` then gives it.
    pub(crate) fn finish(self) -> Self {
        self.derive_header()
    }

    /// This plan with the header its receiver starts with, of its prototype and shape: inline
    /// storage when its fields fit the inline slots, else an arena run.
    pub(crate) fn derive_header(self) -> Self {
        let header = InlineObjectHeader::for_fields(self.prototype, self.shape, usize::from(self.fields));
        let mut classes = [FieldClass::Passed; CONSTRUCT_PLAN_FIELDS];
        for (field, class) in classes.iter_mut().enumerate() {
            *class = if self.constant & (1 << field) != 0 { FieldClass::Constant }
                else if self.computed & (1 << field) != 0 { FieldClass::Computed }
                else if self.defaulted & (1 << field) != 0 { FieldClass::Defaulted } else { FieldClass::Passed };
        }
        Self { header, classes, ..self }
    }

    /// The value of a field of a plan whose stores default nothing (`ConstructPlanKind::Passed`):
    /// a constant field's constant, else the argument its parameter names.
    #[inline(always)]
    pub(crate) fn passed_value(&self, field: usize, argument: impl Fn(u16) -> Value) -> Value {
        if self.constant & (1 << field) != 0 { self.fallbacks[field] } else { argument(self.parameters[field]) }
    }

    /// `values` of a plan with no constant or computed field: each field the argument its
    /// parameter names, or its fallback where the store defaults a falsy one.
    #[inline(always)]
    fn passed_values(&self, vm: &Vm, values: &mut [Value; CONSTRUCT_PLAN_FIELDS], argument: impl Fn(u16) -> Value) {
        let fields = usize::from(self.fields).min(CONSTRUCT_PLAN_FIELDS);
        for field in 0..fields {
            let passed = argument(self.parameters[field]);
            values[field] = if self.defaulted >> field & 1 != 0 && !vm.truthy(passed) { self.fallbacks[field] } else { passed };
        }
    }

    /// This plan's field values in order, `undefined` past them: the argument each field's
    /// parameter names (`argument`), or its fallback for a defaulted field passed a falsy
    /// value, or a constant field's constant, or a computed field's Number, as its store
    /// computes it. `None` when a computed field's argument is no Number.
    pub(crate) fn values(&self, vm: &Vm, argument: impl Fn(u16) -> Value) -> Option<[Value; CONSTRUCT_PLAN_FIELDS]> {
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        self.fill_values(vm, &mut values, argument)?;
        Some(values)
    }

    /// `values` written into `values`, which holds `undefined` throughout; `None` where a
    /// computed field's argument is no Number.
    #[inline(always)]
    pub(crate) fn fill_values(&self, vm: &Vm, values: &mut [Value; CONSTRUCT_PLAN_FIELDS], argument: impl Fn(u16) -> Value) -> Option<()> {
        if self.constant | self.computed == 0 {
            self.passed_values(vm, values, argument);
            return Some(());
        }
        let mut stage = quench_runtime::staged::Concrete::pure();
        for (field, value) in values.iter_mut().enumerate().take(usize::from(self.fields)) {
            let class = self.classes[field];
            let staged = field_value(&mut stage, class, self.fallbacks[field].to_bits(), u64::from(self.operators[field]),
                |_| argument(self.parameters[field]).to_bits(), ());
            *value = match (staged, class) {
                (Some(bits), _) => Value::from_bits(bits),
                // A falsy test the word cannot decide: the heap value's cell does.
                (None, FieldClass::Defaulted) => initializer_value(vm, argument(self.parameters[field]), Some(self.fallbacks[field])),
                // A computed field whose argument is no Number leaves the plan.
                (None, _) => return None,
            };
        }
        Some(())
    }

    /// The receiver this plan allocates with its fields `field(k)` in order and `undefined`
    /// past them, which is how unused inline slots start: gathered into the receiver's inline
    /// slots where they fit, else into the arena run's values.
    #[inline(always)]
    pub(crate) fn allocate(&self, vm: &mut Vm, field: impl Fn(usize) -> Value) -> Value {
        let fields = usize::from(self.fields);
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        for (index, value) in values.iter_mut().enumerate().take(fields) {
            *value = field(index);
        }
        vm.allocate_planned_object(self.header, self.prototype, &values, fields)
    }

    /// The receiver this plan allocates with `values`, as `values` computed them: no second
    /// array gathered from them.
    #[inline(always)]
    pub(crate) fn allocate_values(&self, vm: &mut Vm, values: &[Value; CONSTRUCT_PLAN_FIELDS]) -> Value {
        vm.allocate_planned_object(self.header, self.prototype, values, usize::from(self.fields))
    }

    /// This plan holding the fields `fields` that a forwarded initializer's adds gave a receiver
    /// of `shape` (`record_forwarded_fields_in_lane`), forwarding as `forwarded`.
    pub(crate) fn with_recorded(&self, fields: &Self, shape: u32, forwarded: ForwardedResolution) -> Self {
        Self {
            shape,
            fields: fields.fields,
            parameters: fields.parameters,
            defaulted: fields.defaulted,
            constant: fields.constant,
            computed: fields.computed,
            operators: fields.operators,
            fallbacks: fields.fallbacks,
            nested: fields.nested,
            forwarded: Some(forwarded),
            ..*self
        }
        .derive_header()
    }

    /// This plan extended by the field `store` adds, `None` once it names all it can.
    pub(crate) fn with_field(mut self, store: &quench_runtime::bytecode::InitializerStore, value: Value) -> Option<Self> {
        let field = usize::from(self.fields);
        let slot = self.parameters.get_mut(field)?;
        match store.value {
            quench_runtime::bytecode::InitializerValue::Parameter { parameter, fallback } => {
                *slot = parameter;
                if let Some(fallback) = fallback {
                    self.kind = ConstructPlanKind::Defaulted;
                    self.defaulted |= 1 << field;
                    self.fallbacks[field] = Value::number(fallback);
                }
            }
            quench_runtime::bytecode::InitializerValue::Constant(_) => {
                self.constant |= 1 << field;
                self.fallbacks[field] = value;
            }
            quench_runtime::bytecode::InitializerValue::Numeric { parameter, operator, constant } => {
                *slot = parameter;
                self.kind = ConstructPlanKind::Defaulted;
                self.computed |= 1 << field;
                self.operators[field] = operator;
                self.fallbacks[field] = Value::number(constant);
            }
            quench_runtime::bytecode::InitializerValue::Construct(_) => return None,
        }
        self.fields += 1;
        Some(self)
    }
}

pub(crate) use quench_runtime::vm::engine_types::EMPTY_CONSTRUCT_PLAN;

/// The Number a computed initializer store (`InitializerValue::Numeric`) gives its field:
/// `operator` of the parameter's `passed` value and `constant`, as the body's `Binary`
/// computes it on two Numbers; `None` for any other argument, whose operator may run guest code.
#[inline(always)]
fn numeric_initializer(passed: Value, operator: u8, constant: f64) -> Option<Value> {
    match quench_runtime::vm::operations::static_number_binary(u32::from(operator), passed.as_number()?, constant)? {
        quench_runtime::bytecode::Constant::Number(number) => Some(Value::number(number)),
        _ => None,
    }
}

/// The value an initializer store (`InitializerStore`) gives its field from the parameter's
/// `passed` value: `fallback` in place of a falsy one where the store has it, else as passed.
#[inline(always)]
pub(crate) fn initializer_value(vm: &Vm, passed: Value, fallback: Option<Value>) -> Value {
    match fallback {
        Some(fallback) if !vm.truthy(passed) => fallback,
        _ => passed,
    }
}

/// The value initializer `store` gives its field from the parameters' values `argument`:
/// `None` where the store leaves it to the body (`add_initializer_fields_in_lane`). The running
/// program is the store's (`Vm::active_constant`).
#[inline(always)]
pub(crate) fn initializer_store_value(vm: &Vm, store: &quench_runtime::bytecode::InitializerStore, argument: impl Fn(u16) -> Value) -> Option<Value> {
    match store.value {
        quench_runtime::bytecode::InitializerValue::Parameter { parameter, fallback } => {
            Some(initializer_value(vm, argument(parameter), fallback.map(Value::number)))
        }
        quench_runtime::bytecode::InitializerValue::Constant(constant) => constant.value(|index| vm.active_constant(index)),
        quench_runtime::bytecode::InitializerValue::Numeric { parameter, operator, constant } => numeric_initializer(argument(parameter), operator, constant),
        quench_runtime::bytecode::InitializerValue::Construct(_) => None,
    }
}

/// `initialize_fields_in_lane` while `callee`'s plan holds: the finished receiver, its fields
/// the arguments the plan's parameters name. A plan holds while its callee's layout and
/// `prototype` do: the receiver's template is derived from those alone, and the epoch keeps
/// the adds it planned free of setters. Apart from the planning, whose frame is larger.
#[inline(always)]
pub(crate) fn planned_construction_in_lane(
    vm: &mut Vm,
    callee: Value,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    planned_construction(vm, callee, construct_argument_in_lane(r, i))
}

/// `planned_construction_in_lane` of a plan of passed fields that fit the inline slots, where it
/// calls nothing out of line (`Heap::alloc_planned_inline_quick`); `None`, before any effect,
/// for anything else, which `planned_construction_in_lane` makes.
#[inline(always)]
pub(crate) fn planned_inline_quick_in_lane(vm: &mut Vm, callee: Value, r: Registers, i: &LaneInstruction) -> Option<Value> {
    planned_inline_quick(vm, callee, construct_argument_in_lane(r, i))
}

/// `planned_inline_quick_in_lane` from the parameters' values `argument` gives: any construction
/// site's quick case, a lane record's or a lane body's (`body_calls::constructed_in_place`).
#[inline(always)]
pub(crate) fn planned_inline_quick(vm: &mut Vm, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    let plan = &vm.lane_stack.construct_plans[index];
    let len = usize::from(plan.fields);
    if plan.kind != ConstructPlanKind::Passed || len > INLINE_PROPERTY_COUNT {
        return None;
    }
    const _: () = assert!(INLINE_PROPERTY_COUNT == 2, "the inline slots are filled one by one");
    let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
    if len > 0 { inline[0] = plan.passed_value(0, &argument); }
    if len > 1 { inline[1] = plan.passed_value(1, &argument); }
    let header = plan.header;
    vm.allocate_planned_inline_quick(header, inline, len)
}

/// `planned_construction_in_lane` from the parameters' values `argument` gives.
#[inline(always)]
pub(crate) fn planned_construction(vm: &mut Vm, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Passed {
        return defaulted_construction_in_lane(vm, index, argument);
    }
    let len = usize::from(plan.fields);
    if len <= INLINE_PROPERTY_COUNT {
        // Fields that fit the inline slots are those slots: no values array gathered first.
        const _: () = assert!(INLINE_PROPERTY_COUNT == 2, "the inline slots are filled one by one");
        let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
        if len > 0 { inline[0] = plan.passed_value(0, &argument); }
        if len > 1 { inline[1] = plan.passed_value(1, &argument); }
        return Some(vm.allocate_planned_inline(plan.header, plan.prototype, inline, len));
    }
    Some(vm.allocate_planned_object(plan.header, plan.prototype, &quench_runtime::vm::tier_queries::planned_fields::<CONSTRUCT_PLAN_FIELDS>(len, |index| plan.passed_value(index, &argument)), len))
}

/// Whether the running lane's frame constructs in place (`LaneContext::script_constructs`),
/// which a body's constructions inherit (`body_construction_in_lane`).
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_constructs_in_lane(context: *mut ()) -> bool {
    let cx = context.cast::<LaneContext>();
    quench_runtime::staged::call_construct::concrete_admits_construct_run(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs })
}

/// A lane body's `new callee(...)` of the `argc` arguments at `arguments`, in `code`, the
/// running residual: the callee's construct plan (`planned_construction`), else the receiver
/// its initializing body's cached adds finish (`initializer_construction`), which leaves the
/// plan; `None` (a construction the lane makes otherwise, or a collection due, which the lane
/// runs) to leave the body at the construction, before any effect.
///
/// # Safety
/// `arguments` are the body's `argc` registers.
pub(crate) unsafe fn body_construction_in_lane(vm: &mut Vm, code: &ResidualProgram, callee: Value, arguments: *const Value, argc: u16) -> Option<Value> {
    if !quench_runtime::staged::call_construct::concrete_unpending(vm.construct_target.is_some()) || vm.collection_due() {
        return None;
    }
    // SAFETY: the caller passes the body's argument registers.
    let argument = |parameter: u16| if parameter < argc { unsafe { *arguments.add(usize::from(parameter)) } } else { Value::UNDEFINED };
    // A forwarding constructor's plan (`this.initialize(...)`) holds with its forwarding.
    let this = planned_construction(vm, callee, argument)
        .or_else(|| forwarded_fields_in_lane(vm, callee, argument))
        .or_else(|| initializer_construction(vm, code, callee, argument))?;
    note_allocation(vm);
    Some(this)
}

/// The receiver of a construction through forwarding constructor `callee` whose forwarded
/// initializer the body calls by its entry (`DirectCallee::forwarder`): allocated finished
/// from the plan while it holds, whose forwarding has no fields of its own. `None` leaves
/// the body to its helper path before any effect.
pub(crate) fn body_receiver_in_lane(vm: &mut Vm, callee: Value) -> Option<Value> {
    if !quench_runtime::staged::call_construct::concrete_unpending(vm.construct_target.is_some()) || vm.collection_due() || vm.with_scope_entered {
        return None;
    }
    let index = holding_construct_plan_index(vm, callee)?;
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Forwarding || plan.fields != 0 || plan.forwarded.is_none() {
        return None;
    }
    let this = vm.allocate_planned_object(plan.header, plan.prototype, &[Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS], 0);
    note_allocation(vm);
    Some(this)
}

/// The receiver `callee`'s cached adds finish where its body only stores its parameters and
/// primitive constants into `this` (`Function::initializer_stores`) and an in-place
/// construction admits it (`Vm::inline_constructible`): the static facts of
/// `ScriptCallTarget::initializes`, so a region with no lane run constructs as the lane does,
/// and the plan it leaves serves later constructions. Such a body runs no guest code, binds no
/// environment and reads no `arguments`. A miss leaves the unreachable receiver to the
/// collector. Out of line, so planned constructions keep their code.
#[inline(never)]
fn initializer_construction(vm: &mut Vm, code: &ResidualProgram, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let (program, function, _) = vm.same_realm_user_target(callee)?;
    if program != vm.active_program || function == super::ROOT_FUNCTION_ID
        || !Vm::inline_constructible(code.function_get(function as usize)?) {
        return None;
    }
    let (this, added) = initialize_fields_in_lane(vm, code, callee, callee, function, argument)?;
    added.then_some(this)
}

/// `planned_construction_in_lane` under the plan at `index` with defaulted fields, whose values
/// it computes first; `None` for a forwarding constructor's plan, which holds only with its
/// forwarding (`forwarded_fields_in_lane`). Out of line, so plans of plain fields keep their
/// code.
#[inline(never)]
fn defaulted_construction_in_lane(vm: &mut Vm, index: usize, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind == ConstructPlanKind::Forwarding {
        return None;
    }
    let mut values = plan.values(vm, argument)?;
    // A nested field holds while its root does; a fresh object's allocation never collects.
    if let Some(nested) = plan.nested {
        if !nested_root_holds(vm, &nested) { return None; }
        if nested.constructs {
            values[usize::from(nested.field)] = nested_construction(vm, nested.constructor, &nested.arguments)?;
        }
    }
    { let plan = vm.lane_stack.construct_plans[index]; Some(plan.allocate_values(vm, &values)) }
}

/// A construction's parameter `k` as a plain activation sees it: argument `k`, or undefined
/// when not passed.
#[inline(always)]
pub(crate) fn construct_argument_in_lane(r: Registers, i: &LaneInstruction) -> impl Fn(u16) -> Value + '_ {
    let argc = ImmediateLayout::argument_count(i.imm());
    move |parameter| if parameter < argc { r.get(i.c + parameter) } else { Value::UNDEFINED }
}

/// The construct plan cache entry `callee` owns, when it names a heap cell.
#[inline] pub(crate) fn construct_plan_index(callee: Value) -> Option<usize> {
    callee.heap_index().map(|index| index as usize & (CONSTRUCT_PLANS - 1))
}

/// `callee`'s construct plan while it holds: the entry names `callee`, the prototype
/// layout keeps the plan's epoch and `callee`'s own layout still yields the planned
/// `prototype`, from which the receiver's template is derived.
#[inline(always)]
pub(crate) fn holding_construct_plan(vm: &Vm, callee: Value) -> Option<ConstructPlan> {
    holding_construct_plan_index(vm, callee).map(|index| vm.lane_stack.construct_plans[index])
}

/// `callee`'s plan while it holds, as a body keeps a receiver it builds as its field values
/// (`compiled::ScalarConstructPlan`): only a plan of fields passed or constant, with no nested,
/// forwarded or parent construction, whose shape names each field by a string.
pub(crate) fn scalar_construct_plan(vm: &Vm, callee: Value) -> Option<crate::compiled::ScalarConstructPlan> {
    use crate::compiled::{ScalarConstructPlan, ScalarField};
    let plan = &vm.lane_stack.construct_plans[holding_construct_plan_index(vm, callee)?];
    let len = usize::from(plan.fields);
    if plan.kind != ConstructPlanKind::Passed || plan.forwarded.is_some() || plan.nested.is_some()
        || plan.super_parent != Value::UNDEFINED || (plan.defaulted | plan.computed) != 0 || len > CONSTRUCT_PLAN_FIELDS {
        return None;
    }
    let keys = vm.shape_keys(plan.shape);
    if keys.len() != len { return None; }
    let fields = keys.iter().enumerate().map(|(field, key)| match key {
        quench_runtime::vm::PropertyKey::String(atom) => Some((*atom, if plan.constant & (1 << field) != 0 {
            ScalarField::Constant(plan.fallbacks[field])
        } else {
            ScalarField::Parameter(plan.parameters[field])
        })),
        _ => None,
    }).collect::<Option<_>>()?;
    Some(ScalarConstructPlan { callee: crate::compiled::PlanCallee::Live(callee), shape: plan.shape, fields })
}

/// Whether `callee`'s plan holds now (`holding_construct_plan_index`) and an allocation by it
/// would not refuse (`body_construction_in_lane`): no construction under way, no collection due.
/// A body that keeps a receiver as its field values (`compiled::ScalarConstructPlan`) tests it
/// where the construction would run, so the receiver a leave allocates is the plan's.
pub(crate) fn construct_plan_holds(vm: &Vm, callee: Value) -> bool {
    vm.construct_target.is_none() && !vm.collection_due() && holding_construct_plan_index(vm, callee).is_some()
}

/// The construct plan cache index of `callee`'s plan while it holds (`holding_construct_plan`).
#[inline(always)]
pub(crate) fn holding_construct_plan_index(vm: &Vm, callee: Value) -> Option<usize> {
    let index = construct_plan_index(callee)?;
    (vm.lane_stack.construct_plans[index].callee == callee
        && vm.lane_stack.construct_watches[index].as_ref().is_some_and(|watch| watch.is_published()))
    .then_some(index)
}

/// Records `resolution`, where `callee`'s forwarding read `function`, in plan `index`, which
/// then also holds only while `function` and the holder of its `apply` keep their layouts.
pub(crate) fn record_forwarded_resolution(vm: &mut Vm, index: usize, function: Value, resolution: ForwardedResolution) {
    vm.lane_stack.construct_plans[index].forwarded = Some(resolution);
    watch_construct_plan(vm, index, false, &[function, resolution.apply_holder]);
}

/// Makes plan `index` hold only while `objects`, which its proof read besides the callee
/// (the callee's own layout, so its `prototype` and [[Prototype]] among them), keep their
/// values and layouts, and so do the chains above them, which its adds' chain proofs and a
/// forwarding's `apply` read: `renew` starts the plan's dependency, else it adds to it. A
/// write to a slot of any of them (`hold_prototype_values`), or a layout change of any of them
/// or of a prototype above one (`Vm::hold_chain_layouts`), retires it, so a use compares no
/// epoch, layout or shape; a change to a prototype on none of those chains leaves it. The
/// plan never holds where a reader table is full.
pub(crate) fn watch_construct_plan(vm: &mut Vm, index: usize, renew: bool, objects: &[Value]) {
    use std::rc::Rc;
    use quench_runtime::code_dependencies::DependentCode;
    if renew {
        vm.lane_stack.construct_watches[index] = Some(quench_runtime::code_dependencies::new_dependency());
    }
    let Some(watch) = vm.lane_stack.construct_watches[index].clone() else { return };
    let code: Rc<dyn DependentCode> = watch.clone();
    for &object in objects {
        // Marks every link, the object first, so the shape below is the one marked.
        if !vm.hold_chain_layouts(object, &code) {
            watch.retire();
            return;
        }
        let shape = vm.heap_reads().object(object).map(|data| data.exact_shape());
        if !shape.is_some_and(|shape| quench_runtime::tier_surface::hold_prototype_values(shape, &code)) {
            watch.retire();
            return;
        }
    }
}

/// `initialize_fields_in_lane` of the stores `adds` makes to the receiver it is given, each passed
/// to its `each` as a cached add runs, a nested construction's value (`InitializerValue::Construct`)
/// made by its `nested` from the callee's closure environment `env`: the receiver, and whether
/// every store was a cached add.
pub(crate) fn initialize_stores_in_lane(
    vm: &mut Vm,
    callee: Value,
    constructor: Value,
    env: Value,
    adds: impl FnOnce(&mut Vm, Value, &mut dyn FnMut(&mut Vm, &quench_runtime::bytecode::NestedConstruct) -> Option<Value>, &mut dyn FnMut(&quench_runtime::bytecode::InitializerStore, Value)) -> bool,
) -> Option<(Value, bool)> {
    let plan_index = construct_plan_index(callee);
    let template = vm.inline_construct_template_by(callee, constructor)?;
    let (prototype, source_shape) = (template.proto(), template.shape());
    let this = vm.allocate_object(template);
    let mut planned = Some(ConstructPlan {
        callee,
            callee_layout: (0, 0),
        prototype,
        shape: source_shape,
        ..EMPTY_CONSTRUCT_PLAN
    });
    let nested = std::cell::Cell::new(None);
    let added = adds(vm, this,
        &mut |vm, construct| {
            let (value, field) = nested::nested_construct_in_lane(vm, env, construct)?;
            nested.set(Some(field));
            Some(value)
        },
        &mut |store, value| {
            planned = planned.and_then(|plan| match store.value {
                quench_runtime::bytecode::InitializerValue::Construct(_) => plan.with_nested(nested.take()?),
                _ => plan.with_field(store, value),
            });
        });
    if !added {
        return Some((this, false));
    }
    if let Some(mut plan) = planned
        && let Some(index) = plan_index
        && let Some(layout) = vm.function_prototype_layout(callee)
    {
        plan.callee_layout = layout;
        plan.shape = vm.object_data(this).map_or(source_shape, |object| object.shape());
        // A nested field's root is read at each construction (`defaulted_construction_in_lane`).
        if plan.nested.is_some() {
            plan.kind = ConstructPlanKind::Defaulted;
        }
        vm.lane_stack.construct_plans[index] = plan.finish();
        watch_construct_plan(vm, index, true, &[callee, plan.prototype]);
        if let Some(nested) = plan.nested {
            watch_construct_plan(vm, index, false, nested.held());
        }
    }
    Some((this, true))
}

/// Where the lane continues after a returned construction (`return new C(...)`) made `this`
/// in place: the innermost activation completed with it (`complete_activation_in_lane`), else
/// the frame's own return of it (`frame_return`).
#[inline(always)]
pub(crate) fn return_constructed_in_lane(cx: *mut LaneContext, ip: Ip, this: Value) -> Resumed {
    // A running leaf's construction is its call's completion, as its return is
    // (`complete_leaf_in_lane`).
    if !unsafe { (*cx).leaf.call }.is_null() {
        let form = unsafe { (*cx).leaf.form };
        if let Some((registers, next)) = complete_leaf_in_lane(cx, this, form) {
            let view = Carried {
                memory: MemoryView::script(cx),
                integers: quench_runtime::value::IntegerEncoding::TAG,
                acc: Slot::of(Value::UNDEFINED),
            };
            return Resumed::At(registers, next, view);
        }
    }
    // The common return first, as a `Return` record takes it (`complete_plain_activation_in_lane`),
    // where no leaf call runs.
    if unsafe { (*cx).leaf.call }.is_null()
        && let Some(resumed) = complete_plain_activation_in_lane::<NOT_LEAF>(cx, this)
    {
        return resumed;
    }
    complete_activation_in_lane(cx, ip, this).unwrap_or_else(|| Resumed::Exit(frame_return(unsafe { vm(cx) }, ip, this)))
}
