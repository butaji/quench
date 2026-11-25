//! The constructions an ahead lane body keeps as their field values (`lane_region::virtual_objects`):
//! a `new F(..)` the closed world proves constructs `F` (`AotFacts::construct_target`), where `F`'s
//! body only stores its parameters and immediate constants into `this` (`initializer_stores`). The
//! plan is the one the VM's construct plan of `F`'s closure makes of the same stores
//! (`dispatch_fast::scalar_construct_plan`): its fields in store order, each a parameter as passed
//! or a constant. The code guards the callee by the image's symbol of `F`'s closure
//! (`HeapValueRecipe::ClosureOf`) and asks the VM whether that closure's plan holds where the
//! construction runs (`LaneHelper::ConstructPlanHolds`), so a closure whose plan the VM has not
//! made yet, or another closure, only leaves to the base body, whose construction makes it.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{InitializerConstant, InitializerStore, InitializerValue};
use crate::aot_image::symbols::{
    HeapValueRecipe, ShapeRecipe, ATTRIBUTE_CONFIGURABLE, ATTRIBUTE_ENUMERABLE, ATTRIBUTE_WRITABLE,
};

/// The attribute bits of a field an initializer's store adds: a plain data property.
const ADDED_FIELD: u8 = ATTRIBUTE_WRITABLE | ATTRIBUTE_ENUMERABLE | ATTRIBUTE_CONFIGURABLE;

/// By `(function, pc)` of each construction in `unit` (its caller's or an inlined frame's), the
/// plan the image keeps its receiver by, its callee and shape interned in `symbols`.
pub(crate) fn construct_plans(program: &ResidualProgram, unit: &lane_unit::Unit, function: u32, facts: &AotFacts,
    symbols: &mut SymbolTable) -> BTreeMap<(u32, u32), ScalarConstructPlan> {
    let sites: Vec<(u32, u32)> = unit.origins.iter().map(|origin| match *origin {
        Origin::Caller(record) => (function, record),
        Origin::Inlined { frame, pc } => (unit.frames[frame].target.function, pc),
    }).collect();
    sites.into_iter().filter_map(|(caller, pc)| {
        let callee = facts.function_facts(caller).construct_target(pc)?;
        Some(((caller, pc), image_plan(program, callee, symbols)?))
    }).collect()
}

/// The plan of a construction of function `callee`, where its body only initializes `this` with
/// parameters as passed and immediate constants, each field once, as many as a construct plan holds.
fn image_plan(program: &ResidualProgram, callee: u32, symbols: &mut SymbolTable) -> Option<ScalarConstructPlan> {
    let stores = program.function_get(callee as usize)?.initializer_stores(&program.constants)?;
    if stores.len() > crate::dispatch_fast::CONSTRUCT_PLAN_FIELDS { return None; }
    if stores.iter().enumerate().any(|(index, store)| stores[..index].iter().any(|earlier| earlier.atom == store.atom)) { return None; }
    let fields = stores.iter().map(|store| Some((store.atom, field_of(store)?))).collect::<Option<_>>()?;
    let Symbol::HeapValue(closure) = symbols.heap_value(HeapValueRecipe::ClosureOf(callee)) else { return None };
    let Symbol::HeapValue(prototype) = symbols.heap_value(HeapValueRecipe::PrototypeOf(callee)) else { return None };
    let recipe = ShapeRecipe { prototype: Some(prototype), fields: stores.iter().map(|store| (store.atom, ADDED_FIELD)).collect() };
    let Symbol::Shape(shape) = symbols.shape(recipe) else { return None };
    Some(ScalarConstructPlan { callee: PlanCallee::Image(closure), shape, fields })
}

/// Where a store's field takes its value in the VM's plan: a parameter nothing defaults, or a
/// constant that needs no heap cell; `None` for a store the plan computes or allocates.
fn field_of(store: &InitializerStore) -> Option<ScalarField> {
    match store.value {
        InitializerValue::Parameter { parameter, fallback: None } => Some(ScalarField::Parameter(parameter)),
        InitializerValue::Constant(constant) if !matches!(constant, InitializerConstant::String(_)) =>
            constant.value(|_| None).map(ScalarField::Constant),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(program: &ResidualProgram, name: &str) -> u32 {
        let is = |id: usize| quench_runtime::test_support::with_record(program, id, |function| function.name.is_some_and(|atom| &program.atoms[atom as usize] == name));
        (0..program.function_count()).find(|&id| is(id)).expect("a function with that name") as u32
    }

    #[test]
    fn an_initializer_of_parameters_and_immediates_is_kept_by_the_image() {
        let program = quench_runtime::Engine::specialize("function P(x, y) { this.x = x; this.y = y; this.z = 0; } new P(1, 2);", "plan.js").unwrap();
        let mut symbols = SymbolTable::new(&program);
        let plan = image_plan(&program, named(&program, "P"), &mut symbols).expect("a plan");
        assert!(matches!(plan.callee, PlanCallee::Image(_)));
        let fields: Vec<ScalarField> = plan.fields.iter().map(|(_, field)| *field).collect();
        assert_eq!(fields, [ScalarField::Parameter(0), ScalarField::Parameter(1), ScalarField::Constant(quench_runtime::Value::number(0.0))]);
    }

    #[test]
    fn a_string_store_keeps_no_plan() {
        let program = quench_runtime::Engine::specialize("function S(x) { this.x = x; this.s = \"a\"; } new S(1);", "plan.js").unwrap();
        let mut symbols = SymbolTable::new(&program);
        assert!(image_plan(&program, named(&program, "S"), &mut symbols).is_none());
    }
}
