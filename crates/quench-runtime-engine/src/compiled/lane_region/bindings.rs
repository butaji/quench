//! The admission of an inlined callee bound by its function (`CalleeBinding::Function`):
//! the VM's cached in-loop call target answers inline, its binding helper otherwise; a call whose
//! call plan (`compile::call_plan`) names exactly its target admits with no compare, the plan's
//! miss deciding what retires the code (`plan_call`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;
use quench_runtime::compile::aot_facts::AotFacts;
use quench_runtime::compile::call_plan::{call_plans, Arm, CallPlan, Miss};

/// The call plans of a program's functions, computed on first lookup of each function (`call_plans`) from the
/// program's facts and signatures. Empty for every function where the facts or their signatures did not settle.
pub(crate) struct CallPlans<'a> {
    program: &'a quench_runtime::ResidualProgram,
    facts: Option<&'a AotFacts>,
    /// The program's call graph, which decides the callees a plan's arms may inline (`lane_unit::inlines_whole`).
    graph: Option<&'a quench_runtime::compile::call_graph::CallGraph>,
    functions: std::cell::RefCell<std::collections::BTreeMap<u32, std::collections::BTreeMap<u32, CallPlan>>>,
}

impl<'a> CallPlans<'a> {
    /// The plans of `program`, from `facts` (the program's AOT facts) and `graph` (its call graph), none computed yet.
    pub(crate) fn new(program: &'a quench_runtime::ResidualProgram, facts: Option<&'a AotFacts>,
        graph: Option<&'a quench_runtime::compile::call_graph::CallGraph>) -> Self {
        Self { program, facts, graph, functions: Default::default() }
    }

    /// The plan of the call at `pc` of `function`, if a plan covers it.
    pub(crate) fn at(&self, function: u32, pc: u32) -> Option<CallPlan> {
        let mut functions = self.functions.borrow_mut();
        let plans = functions.entry(function).or_insert_with(|| self.plans_of(function));
        plans.get(&pc).cloned()
    }

    /// Every plan of `function` (`call_plans`), with inlinability from the program's bodies (`inlines_whole`).
    fn plans_of(&self, function: u32) -> std::collections::BTreeMap<u32, CallPlan> {
        let Some(facts) = self.facts else { return Default::default() };
        let Some(signatures) = facts.signatures() else { return Default::default() };
        let inlinable = |callee: u32| super::super::lane_unit::inlines_whole(self.graph, self.program, callee);
        call_plans(self.program, facts, signatures, function, &inlinable)
    }
}

impl Lowering<'_, '_> {
    /// The miss of inlined `frame`'s call when its plan names exactly the frame's target as its one arm, with
    /// no class: `Miss::None` where no key it read can move, else the key whose edge retires the code. None where
    /// the plan names another target, a class arm, or no plan covers the call.
    pub(crate) fn planned_target(&self, frame: usize) -> Option<Miss> {
        let call = self.unit.frames[frame];
        if call.target.callee != CalleeBinding::Function { return None; }
        let caller = call.parent.map_or(self.function_id, |parent| self.unit.frames[parent].target.function);
        let plan = self.plans.at(caller, call.call)?;
        match &*plan.arms {
            [Arm { class: None, function }] if *function == call.target.function => Some(plan.miss),
            _ => None,
        }
    }

    /// Whether inlined `frame`'s call admits its target with no compare (`planned_target`). Where no key it read
    /// can move, the block holds that under the image's invalidation word (`hold_stable`), read once, never per call.
    /// Where a key is live, the block subscribes it (`StableUses::cells`), and an edge that moves it retires the code.
    pub(crate) fn plan_call(&mut self, state: &mut State, exit: ir::Block, frame: usize) -> bool {
        match self.planned_target(frame) {
            Some(Miss::None) => { self.hold_stable(state, exit); true }
            Some(Miss::Edge(key)) => { self.stable.cells.insert(key); true }
            None => false,
        }
    }

    pub(crate) fn function_binding(&mut self, callee: ir::Value, function: u32) -> ir::Value {
        let program = self.raw_load(Lane::I32, regions::dispatch(), self.payload, std::mem::offset_of!(JsStepDispatch, program_id) as i32);
        let program = self.raw_unary(Unary::ZeroExtend(Lane::I64), program);
        let probed = function != super::super::ROOT_FUNCTION_ID;
        let function = self.raw_const(Lane::I64, i64::from(function));
        let admitted = if self.call_budget.is_some() && probed {
            let (slow, join) = (self.builder.create_block(), self.builder.create_block());
            self.builder.set_cold_block(slow);
            let admitted = self.builder.append_block_param(join, types::I64);
            let hit = self.cached_call_target(program, callee, function);
            let one = self.raw_const(Lane::I64, 1);
            self.branch_if(hit, (join, &[one]), (slow, &[]));
            self.builder.switch_to_block(slow);
            let helper = self.helpers.target(&mut self.builder, LaneHelper::AdmittedFunctionBinding);
            let called = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function]);
            self.goto(join, &[called]);
            self.builder.switch_to_block(join);
            admitted
        } else {
            let depth = self.unit.recovery_depth().max(1) as i64;
            let depth = self.raw_const(Lane::I64, depth);
            let helper = self.helpers.target(&mut self.builder, LaneHelper::FunctionBinding);
            emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function, depth])
        };
        self.raw_compare_imm(Relation::NotEqual, admitted, 0)
    }

    /// Whether the VM's in-loop call target of `callee` names `function` of `program` with no
    /// termination requested: a same-realm closure of that body (`Vm::in_loop_call_target`),
    /// as `admitted_function_binding_in_lane` decides, without its call.
    fn cached_call_target(&mut self, program: ir::Value, callee: ir::Value, function: ir::Value) -> ir::Value {
        type Target = super::super::InLoopCallTarget;
        let table = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, call_targets));
        let slot = self.raw_binary_imm(Binary::And, callee, (super::super::CALL_TARGET_CACHE_SIZE - 1) as i64);
        let offset = self.raw_binary_imm(Binary::Mul, slot, std::mem::size_of::<Target>() as i64);
        let entry = self.raw_binary(Binary::Add, table, offset);
        let field = |offset: usize| i32::try_from(offset).expect("call target field offset");
        let named = self.raw_load(Lane::I64, regions::heap_meta(), entry, field(std::mem::offset_of!(Target, callee)));
        let owner = self.raw_load_extended(Lane::I32, Extend::Zero, Lane::I64, regions::heap_meta(), entry, field(std::mem::offset_of!(Target, program)));
        let body = self.raw_load_extended(Lane::I32, Extend::Zero, Lane::I64, regions::heap_meta(), entry, field(std::mem::offset_of!(Target, function)));
        let same = self.raw_compare(Relation::Equal, named, callee);
        let owned = self.raw_compare(Relation::Equal, owner, program);
        let bound = self.raw_compare(Relation::Equal, body, function);
        let hit = self.raw_binary(Binary::And, same, owned);
        let hit = self.raw_binary(Binary::And, hit, bound);
        if self.fault_poll { return hit; }
        let flag = self.interrupt;
        let requested = self.raw_atomic_load(Lane::I8, regions::dispatch(), flag);
        let clear = self.raw_compare_imm(Relation::Equal, requested, 0);
        self.raw_binary(Binary::And, hit, clear)
    }
}
