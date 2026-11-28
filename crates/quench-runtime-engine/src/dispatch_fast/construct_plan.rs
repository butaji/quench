//! Construct plans: a constructor's receiver allocated finished, its initializer stores made at once.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::plan_fields::{field_value, FieldClass};
pub(crate) mod nested;
pub(crate) mod adds;
pub(crate) mod site;
pub(crate) use adds::{add_initializer_fields_in_lane, add_stores_in_lane};
pub(crate) use site::{site_class_in_lane, planned_site_in_lane, planned_quick_site_in_lane, initialize_site_in_lane, inline_receiver_site_in_lane};
pub(crate) use nested::{finish_forwarded_in_lane, nested_construction, nested_root_holds};

pub(crate) use quench_runtime::vm::engine_types::ConstructPlan;
use quench_runtime::vm::engine_types::FixedCellPlan;

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
        self.allocate_receiver(vm, &values, fields)
    }

    /// The receiver this plan allocates with `values`, as `values` computed them: no second
    /// array gathered from them.
    #[inline(always)]
    pub(crate) fn allocate_values(&self, vm: &mut Vm, values: &[Value; CONSTRUCT_PLAN_FIELDS]) -> Value {
        self.allocate_receiver(vm, values, usize::from(self.fields))
    }

    /// The receiver of `values`' first `fields` fields. A plan whose class's header is recorded (`recorded`) allocates
    /// it alone; the first construction of an unrecorded class records it (`record_unrecorded`).
    #[inline(always)]
    fn allocate_receiver(&self, vm: &mut Vm, values: &[Value; CONSTRUCT_PLAN_FIELDS], fields: usize) -> Value {
        let this = match self.fixed {
            // A fixed class's instance is its union cell, each field at its slot (`Vm::allocate_fixed_cell`).
            Some(fixed) => vm.allocate_fixed_cell(fixed.shape, self.prototype, &fixed.slots[..fields], &values[..fields]),
            None => vm.allocate_planned_object(self.header, self.prototype, values, fields),
        };
        if !self.recorded {
            self.record_unrecorded(vm, this);
        }
        this
    }

    /// The first construction of a class whose header is not recorded: records it (`Vm::record_class_header`, which
    /// resolves the class's planned arms) and marks the plan in the lane stack recorded, so its later constructions
    /// take `allocate_receiver` alone. A construction of a constructor that is no user function records nothing.
    #[cold]
    #[inline(never)]
    fn record_unrecorded(&self, vm: &mut Vm, this: Value) {
        vm.record_class_header(self.callee, self.class, this);
        if let Some(index) = construct_plan_index(self.callee, self.class) && vm.lane_stack.construct_plans[index].callee == self.callee && vm.lane_stack.construct_plans[index].class == self.class {
            vm.lane_stack.construct_plans[index].recorded = true;
        }
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
            fixed: fields.fixed,
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

mod lookups;
pub(crate) use lookups::*;
mod stores;
pub(crate) use stores::*;
