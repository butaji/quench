//! Construct plans: a constructor's receiver allocated finished, its initializer stores made at once.
use super::*;

/// What a constructor's initializer body (`ScriptCallTarget::initializes`) made of a receiver
/// whose adds all hit: the callee's receiver template under `prototype`, which the callee and
/// that prototype alone determine, ends at `shape`, its fields the arguments for
/// `parameters[..fields]` in order, a defaulted one's fallback in place of a falsy argument.
/// While the prototype layout keeps `epoch` (`prototype_layout_epoch`), which every
/// collection advances, those adds would hit again with the same transitions, so the object
/// can be allocated finished.
#[derive(Clone, Copy)]
pub(super) struct ConstructPlan {
    pub(super) callee: Value,
    pub(super) epoch: u64,
    /// The callee's own layout that fixes where its `prototype` lives
    /// (`Vm::function_prototype_layout`), which a construct reads it through.
    pub(super) callee_layout: (u32, usize),
    pub(super) prototype: Value,
    pub(super) shape: u32,
    /// The receiver's header (`ConstructPlan::derive_header`): of inline storage when its
    /// fields fit the inline slots, else of an arena run.
    pub(super) header: InlineObjectHeader,
    pub(super) kind: ConstructPlanKind,
    pub(super) fields: u8,
    pub(super) parameters: [u16; CONSTRUCT_PLAN_FIELDS],
    /// Fields whose store defaults a falsy argument (`InitializerStore::fallback`), one bit
    /// per field index, each taking `fallbacks` at its index then.
    pub(super) defaulted: u8,
    /// Fields whose store writes a constant (`InitializerValue::Constant`), one bit per field
    /// index, each taking `fallbacks` at its index whatever is passed.
    pub(super) constant: u8,
    /// Fields whose store computes a Number from its parameter (`InitializerValue::Numeric`),
    /// one bit per field index, each with `operators` and the constant in `fallbacks` at it.
    pub(super) computed: u8,
    pub(super) operators: [u8; CONSTRUCT_PLAN_FIELDS],
    pub(super) fallbacks: [Value; CONSTRUCT_PLAN_FIELDS],
    /// For a forwarding constructor's plan, where its body's reads resolved
    /// (`lane_forwarded_construction`), which the plan's own proof keeps. Its fields, if any, are
    /// those its forwarded initializer stores (`lane_record_forwarded_fields`).
    pub(super) forwarded: Option<ForwardedResolution>,
    /// For a class whose constructor only constructs its parent (`lane_super_construction`),
    /// the parent whose stores the plan holds; `undefined` for any other plan. The plan holds
    /// while the class's [[Prototype]] is still that parent, which `super(...)` constructs.
    pub(super) super_parent: Value,
}

/// Which constructions allocate from a `ConstructPlan`, and how they compute its fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConstructPlanKind {
    /// An initializer constructor's plan (`lane_initialize_fields`) whose fields are the
    /// arguments as passed.
    Passed,
    /// An initializer constructor's plan with defaulted or computed fields
    /// (`ConstructPlan::defaulted`, `ConstructPlan::computed`).
    Defaulted,
    /// A forwarding constructor's plan (`lane_planned_receiver`), which holds only with its
    /// forwarding (`lane_forwarded_fields`).
    Forwarding,
}

impl ConstructPlan {
    /// This plan, holding from the current prototype layout epoch, with its receiver
    /// header derived from its prototype and shape.
    pub(super) fn finish(self) -> Self {
        Self {
            epoch: crate::heap::prototype_layout_epoch(),
            ..self.derive_header()
        }
    }

    /// This plan with the header its receiver starts with, of its prototype and shape: inline
    /// storage when its fields fit the inline slots, else an arena run.
    pub(super) fn derive_header(self) -> Self {
        let header = if usize::from(self.fields) <= INLINE_PROPERTY_COUNT {
            InlineObjectHeader::of(self.prototype, self.shape)
        } else {
            InlineObjectHeader::of_run(self.prototype, self.shape)
        };
        Self { header, ..self }
    }

    /// The value of a field of a plan whose stores default nothing (`ConstructPlanKind::Passed`):
    /// a constant field's constant, else the argument its parameter names.
    #[inline(always)]
    pub(super) fn passed_value(&self, field: usize, argument: impl Fn(u16) -> Value) -> Value {
        if self.constant & (1 << field) != 0 { self.fallbacks[field] } else { argument(self.parameters[field]) }
    }

    /// This plan's field values in order, `undefined` past them: the argument each field's
    /// parameter names (`argument`), or its fallback for a defaulted field passed a falsy
    /// value, or a constant field's constant, or a computed field's Number, as its store
    /// computes it. `None` when a computed field's argument is no Number.
    pub(super) fn values<H: Host>(&self, vm: &Vm<H>, argument: impl Fn(u16) -> Value) -> Option<[Value; CONSTRUCT_PLAN_FIELDS]> {
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        for (field, value) in values.iter_mut().enumerate().take(usize::from(self.fields)) {
            if self.constant & (1 << field) != 0 {
                *value = self.fallbacks[field];
                continue;
            }
            if self.computed & (1 << field) != 0 {
                *value = numeric_initializer(argument(self.parameters[field]), self.operators[field], self.fallbacks[field].as_number()?)?;
                continue;
            }
            let fallback = (self.defaulted & (1 << field) != 0).then_some(self.fallbacks[field]);
            *value = initializer_value(vm, argument(self.parameters[field]), fallback);
        }
        Some(values)
    }

    /// The receiver this plan allocates with its fields `field(k)` in order and `undefined`
    /// past them, which is how unused inline slots start: gathered into the receiver's inline
    /// slots where they fit, else into the arena run's values.
    #[inline(always)]
    pub(super) fn allocate(&self, heap: &mut crate::heap::Heap, field: impl Fn(usize) -> Value) -> Value {
        let fields = usize::from(self.fields);
        if fields <= INLINE_PROPERTY_COUNT {
            let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
            for (index, slot) in inline.iter_mut().enumerate().take(fields) {
                *slot = field(index);
            }
            return heap.alloc_object_from_header(self.header, self.prototype, inline);
        }
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        for (index, value) in values.iter_mut().enumerate().take(fields) {
            *value = field(index);
        }
        heap.alloc_object_from_run_header(self.header, self.prototype, &values, fields)
    }

    /// This plan extended by the field `store` adds, `None` once it names all it can.
    pub(super) fn with_field(mut self, store: &crate::bytecode::InitializerStore, value: Value) -> Option<Self> {
        let field = usize::from(self.fields);
        let slot = self.parameters.get_mut(field)?;
        match store.value {
            crate::bytecode::InitializerValue::Parameter { parameter, fallback } => {
                *slot = parameter;
                if let Some(fallback) = fallback {
                    self.kind = ConstructPlanKind::Defaulted;
                    self.defaulted |= 1 << field;
                    self.fallbacks[field] = Value::number(fallback);
                }
            }
            crate::bytecode::InitializerValue::Constant(_) => {
                self.constant |= 1 << field;
                self.fallbacks[field] = value;
            }
            crate::bytecode::InitializerValue::Numeric { parameter, operator, constant } => {
                *slot = parameter;
                self.kind = ConstructPlanKind::Defaulted;
                self.computed |= 1 << field;
                self.operators[field] = operator;
                self.fallbacks[field] = Value::number(constant);
            }
        }
        self.fields += 1;
        Some(self)
    }
}

pub(super) const EMPTY_CONSTRUCT_PLAN: ConstructPlan = ConstructPlan {
    callee: Value::UNDEFINED,
    epoch: NO_CONSTRUCT_PLAN_EPOCH,
    callee_layout: (0, 0),
    prototype: Value::UNDEFINED,
    shape: 0,
    header: InlineObjectHeader::EMPTY,
    kind: ConstructPlanKind::Passed,
    fields: 0,
    parameters: [0; CONSTRUCT_PLAN_FIELDS],
    defaulted: 0,
    constant: 0,
    computed: 0,
    operators: [0; CONSTRUCT_PLAN_FIELDS],
    fallbacks: [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS],
    forwarded: None,
    super_parent: Value::UNDEFINED,
};

/// The Number a computed initializer store (`InitializerValue::Numeric`) gives its field:
/// `operator` of the parameter's `passed` value and `constant`, as the body's `Binary`
/// computes it on two Numbers; `None` for any other argument, whose operator may run guest code.
#[inline(always)]
fn numeric_initializer(passed: Value, operator: u8, constant: f64) -> Option<Value> {
    match crate::vm::operations::static_number_binary(u32::from(operator), passed.as_number()?, constant)? {
        crate::bytecode::Constant::Number(number) => Some(Value::number(number)),
        _ => None,
    }
}

/// The value an initializer store (`InitializerStore`) gives its field from the parameter's
/// `passed` value: `fallback` in place of a falsy one where the store has it, else as passed.
#[inline(always)]
pub(super) fn initializer_value<H: Host>(vm: &Vm<H>, passed: Value, fallback: Option<Value>) -> Value {
    match fallback {
        Some(fallback) if !vm.truthy(passed) => fallback,
        _ => passed,
    }
}

/// The value initializer `store` gives its field from the parameters' values `argument`:
/// `None` where the store leaves it to the body (`lane_add_initializer_fields`). The running
/// program is the store's (`Vm::active_constant`).
#[inline(always)]
pub(super) fn initializer_store_value<H: Host>(vm: &Vm<H>, store: &crate::bytecode::InitializerStore, argument: impl Fn(u16) -> Value) -> Option<Value> {
    match store.value {
        crate::bytecode::InitializerValue::Parameter { parameter, fallback } => {
            Some(initializer_value(vm, argument(parameter), fallback.map(Value::number)))
        }
        crate::bytecode::InitializerValue::Constant(constant) => constant.value(|index| vm.active_constant(index)),
        crate::bytecode::InitializerValue::Numeric { parameter, operator, constant } => numeric_initializer(argument(parameter), operator, constant),
    }
}

/// The adds an initializing body (`ScriptCallTarget::initializes`) of `function` makes to
/// `this`, each the parameter its store names (`argument`) as a cached add that runs no guest
/// code, passed to `each` in order. `false` at the first add the caches do not answer: the
/// adds made so far are the body's own first stores of the same values, which an activation
/// of the body then makes again as stores, leaving the object as the activation alone would.
#[inline(always)]
pub(super) fn lane_add_initializer_fields<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    this: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
    mut each: impl FnMut(&crate::bytecode::InitializerStore, Value),
) -> bool {
    let Some(stores) = code.function(function as usize).initializer_stores(&code.constants) else {
        return false;
    };
    for store in stores {
        let Some(value) = initializer_store_value(vm, store, &argument) else {
            return false;
        };
        if !vm.field_add_atom_eligible(store.atom) || !vm.primary_field_add_hit(code, this, store.atom, value, store.site) {
            return false;
        }
        each(store, value);
    }
    true
}

/// `lane_initialize_fields` while `callee`'s plan holds: the finished receiver, its fields
/// the arguments the plan's parameters name. A plan holds while its callee's layout and
/// `prototype` do: the receiver's template is derived from those alone, and the epoch keeps
/// the adds it planned free of setters. Apart from the planning, whose frame is larger.
#[inline(always)]
pub(super) fn lane_planned_construction<H: Host>(
    vm: &mut Vm<H>,
    callee: Value,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    planned_construction(vm, callee, lane_construct_argument(r, i))
}

/// `lane_planned_construction` from the parameters' values `argument` gives.
#[inline(always)]
fn planned_construction<H: Host>(vm: &mut Vm<H>, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Passed {
        return lane_defaulted_construction(vm, index, argument);
    }
    Some(plan.allocate(&mut vm.heap, |field| plan.passed_value(field, &argument)))
}

/// Whether the running lane's frame constructs in place (`LaneContext::script_constructs`),
/// which a body's constructions inherit (`lane_body_construction`).
///
/// # Safety
/// `context` is the running lane's context.
pub(in crate::vm) unsafe fn lane_context_constructs<H: Host>(context: *mut ()) -> bool {
    if !crate::DYNAMIC_EXECUTION_ENABLED { return false; }
    let cx = context.cast::<LaneContext<H>>();
    unsafe { (*cx).script_calls && (*cx).script_constructs }
}

/// A lane body's `new callee(...)` of the `argc` arguments at `arguments`, in `code`, the
/// running residual: the callee's construct plan (`planned_construction`), else the receiver
/// its initializing body's cached adds finish (`initializer_construction`), which leaves the
/// plan; `None` (a construction the lane makes otherwise, or a collection due, which the lane
/// runs) to leave the body at the construction, before any effect.
///
/// # Safety
/// `arguments` are the body's `argc` registers.
pub(in crate::vm) unsafe fn lane_body_construction<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, callee: Value, arguments: *const Value, argc: u16) -> Option<Value> {
    if vm.construct_target.is_some() || vm.heap.should_collect() {
        return None;
    }
    // SAFETY: the caller passes the body's argument registers.
    let argument = |parameter: u16| if parameter < argc { unsafe { *arguments.add(usize::from(parameter)) } } else { Value::UNDEFINED };
    // A forwarding constructor's plan (`this.initialize(...)`) holds with its forwarding.
    let this = planned_construction(vm, callee, argument)
        .or_else(|| lane_forwarded_fields(vm, callee, argument))
        .or_else(|| initializer_construction(vm, code, callee, argument))?;
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
fn initializer_construction<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let (program, function, _) = vm.same_realm_user_target(callee)?;
    if program != vm.active_program || function == super::ROOT_FUNCTION_ID
        || !Vm::<H>::inline_constructible(code.function_get(function as usize)?) {
        return None;
    }
    let (this, added) = lane_initialize_fields(vm, code, callee, callee, function, argument)?;
    added.then_some(this)
}

/// `lane_planned_construction` under the plan at `index` with defaulted fields, whose values
/// it computes first; `None` for a forwarding constructor's plan, which holds only with its
/// forwarding (`lane_forwarded_fields`). Out of line, so plans of plain fields keep their
/// code.
#[inline(never)]
fn lane_defaulted_construction<H: Host>(vm: &mut Vm<H>, index: usize, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind == ConstructPlanKind::Forwarding {
        return None;
    }
    let values = plan.values(vm, argument)?;
    Some(plan.allocate(&mut vm.heap, |field| values[field]))
}

/// A construction's parameter `k` as a plain activation sees it: argument `k`, or undefined
/// when not passed.
#[inline(always)]
pub(super) fn lane_construct_argument(r: Registers, i: &LaneInstruction) -> impl Fn(u16) -> Value + '_ {
    let argc = ImmediateLayout::argument_count(i.imm());
    move |parameter| if parameter < argc { r.get(i.c + parameter) } else { Value::UNDEFINED }
}

/// The construct plan cache entry `callee` owns, when it names a heap cell.
pub(super) fn construct_plan_index(callee: Value) -> Option<usize> {
    callee.heap_index().map(|index| index as usize & (CONSTRUCT_PLANS - 1))
}

/// `callee`'s construct plan while it holds: the entry names `callee`, the prototype
/// layout keeps the plan's epoch and `callee`'s own layout still yields the planned
/// `prototype`, from which the receiver's template is derived.
#[inline(always)]
pub(super) fn holding_construct_plan<H: Host>(vm: &Vm<H>, callee: Value) -> Option<ConstructPlan> {
    holding_construct_plan_index(vm, callee).map(|index| vm.lane_stack.construct_plans[index])
}

/// The construct plan cache index of `callee`'s plan while it holds (`holding_construct_plan`).
#[inline(always)]
pub(super) fn holding_construct_plan_index<H: Host>(vm: &Vm<H>, callee: Value) -> Option<usize> {
    let index = construct_plan_index(callee)?;
    let plan = &vm.lane_stack.construct_plans[index];
    (plan.callee == callee
        && plan.epoch == crate::heap::prototype_layout_epoch()
        && vm.function_prototype_in_layout(callee, plan.callee_layout) == Some(plan.prototype)
        && (plan.super_parent.is_undefined()
            || vm.object_data(callee).is_some_and(|class| class.proto() == plan.super_parent)))
    .then_some(index)
}
