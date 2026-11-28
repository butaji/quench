use super::*;

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

/// Whether program code may read field `atom` of the instances `callee` constructs (`AotFacts::field_has_reader`),
/// settled once when a plan is built. Where the callee has no class of the running program, or its program has no
/// facts, there is no fact to ask, and every field has a reader, as `field_has_reader` answers without a reader set.
pub(crate) fn field_reader(vm: &Vm, callee: Value) -> impl Fn(u32) -> bool {
    let (facts, class) = match vm.same_realm_user_target(callee).filter(|&(program, _, _)| program == vm.active_program) {
        Some((program, class, _)) => (vm.programs.aot_facts(program), Some(class)),
        None => (None, None),
    };
    move |atom| class.map_or(true, |class| facts.as_ref().map_or(true, |facts| {
        facts.field_has_reader(quench_runtime::typed_ir::FactKey::Field { class, instance: true, atom })
    }))
}

/// `initialize_fields_in_lane` of the stores `adds` makes to the receiver it is given, each passed
/// to its `each` as a cached add runs, a nested construction's value (`InitializerValue::Construct`)
/// made by its `nested` from the callee's closure environment `env`: the receiver, and whether
/// every store was a cached add.
pub(crate) fn initialize_stores_in_lane(
    vm: &mut Vm,
    callee: Value,
    class: u32,
    constructor: Value,
    env: Value,
    adds: impl FnOnce(&mut Vm, Value, bool, &mut dyn FnMut(&mut Vm, &quench_runtime::bytecode::NestedConstruct) -> Option<Value>, &mut dyn FnMut(&quench_runtime::bytecode::InitializerStore, Value)) -> bool,
) -> Option<(Value, bool)> {
    let plan_index = construct_plan_index(callee, class);
    let template = vm.inline_construct_template_by(callee, constructor, class)?;
    let (prototype, source_shape) = (template.proto(), template.shape());
    // A fixed class's union (R4-F, `Vm::fixed_union`): its receiver is the union cell from birth, all slots absent,
    // and each store is written to its union slot by atom once the adds have made them. No record is made.
    let union = vm.fixed_union(constructor, class, source_shape);
    let this = match union {
        Some(union) => vm.allocate_fixed_cell(union.shape, prototype, &[], &[]),
        None => vm.allocate_object(template),
    };
    let mut planned = Some(ConstructPlan {
        callee,
        callee_layout: (0, 0),
        prototype,
        shape: source_shape,
        class,
        ..EMPTY_CONSTRUCT_PLAN
    });
    // Each store's name and value, in store order: the plan's fields while it holds, and a fixed class's slot writes.
    let mut atoms = [0; quench_runtime::heap::MAX_CELL_FIELDS];
    let mut values = [Value::UNDEFINED; quench_runtime::heap::MAX_CELL_FIELDS];
    let mut stored = 0;
    let nested = std::cell::Cell::new(None);
    let added = adds(vm, this, union.is_some(),
        &mut |vm, construct| {
            let (value, field) = nested::nested_construct_in_lane(vm, env, construct)?;
            nested.set(Some(field));
            Some(value)
        },
        &mut |store, value| {
            if let (Some(atom), Some(stored_value)) = (atoms.get_mut(stored), values.get_mut(stored)) {
                *atom = store.atom;
                *stored_value = value;
            }
            stored += 1;
            planned = planned.and_then(|plan| match store.value {
                quench_runtime::bytecode::InitializerValue::Construct(_) => plan.with_nested(nested.take()?),
                _ => plan.with_field(store, value),
            });
        });
    if !added {
        return Some((this, false));
    }
    if let Some(union) = union {
        for (&atom, &value) in atoms.iter().zip(&values).take(stored) {
            let slot = union.slot_of(atom).expect("a fixed class's store names one of its union slots");
            vm.store_fixed_cell_slot(this, slot, atom, value);
        }
    }
    let Some(mut plan) = planned else {
        return Some((this, true));
    };
    if let Some(index) = plan_index
        && let Some(layout) = vm.function_prototype_layout(callee)
    {
        // The fixed cell's slots: each store's union slot (`FixedUnion::cell_slots`), for the plan's later instances.
        let fixed = match union {
            None => None,
            Some(union) => Some(FixedCellPlan {
                shape: union.shape,
                slots: union.cell_slots(&atoms[..usize::from(plan.fields)]).expect("a fixed class's store names one of its union slots"),
            }),
        };
        plan.callee_layout = layout;
        plan.shape = match fixed {
            Some(fixed) => fixed.shape,
            None => vm.heap_reads().object(this).map_or(source_shape, |object| object.shape()),
        };
        plan.fixed = fixed;
        // A nested field's root is read at each construction (`defaulted_construction_in_lane`), and a fixed
        // cell's fields are gathered into its slots there (`allocate_receiver`).
        if plan.nested.is_some() || fixed.is_some() {
            plan.kind = ConstructPlanKind::Defaulted;
        }
        vm.lane_stack.construct_plans[index] = plan.finish();
        watch_construct_plan(vm, index, true, &[callee, plan.prototype]);
        if let Some(nested) = plan.nested {
            watch_construct_plan(vm, index, false, nested.held());
        }
        if fixed.is_some() {
            // The receiver is the cell already made; its class is recorded as the stored plan records it.
            let stored_plan = vm.lane_stack.construct_plans[index];
            if !stored_plan.recorded {
                stored_plan.record_unrecorded(vm, this);
            }
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
