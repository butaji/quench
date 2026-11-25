//! The admission of an inlined callee bound by its function (`CalleeBinding::Function`):
//! the VM's cached in-loop call target answers inline, its binding helper otherwise; in an AOT
//! image whose closed world proves the call's target (`AotFacts::call_target`), nothing: the fact
//! is a dependency of the image (`closed_call`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;

impl Lowering<'_, '_> {
    /// Whether the closed world proves that inlined `frame`'s call always calls its target: its
    /// admission then compares nothing, and the block holds the fact under the image's invalidation
    /// word instead (`hold_stable`), read once, never per call.
    pub(crate) fn closed_call(&mut self, state: &mut State, exit: ir::Block, frame: usize) -> bool {
        let proved = self.closed_proves(frame);
        if proved { self.hold_stable(state, exit); }
        proved
    }

    /// Whether the closed world proves inlined `frame`'s target (`closed_call`), which no entry admits either.
    pub(crate) fn closed_proves(&self, frame: usize) -> bool {
        let Some(closed) = self.vm.closed else { return false };
        let call = self.unit.frames[frame];
        let caller = call.parent.map_or(self.function_id, |parent| self.unit.frames[parent].target.function);
        call.target.callee == CalleeBinding::Function && closed.function_facts(caller).call_target(call.call) == Some(call.target.function)
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
