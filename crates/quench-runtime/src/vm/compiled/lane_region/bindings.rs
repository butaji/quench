//! The admission of an inlined callee bound by its function (`CalleeBinding::Function`):
//! the VM's cached in-loop call target answers inline, its binding helper otherwise.
use super::*;

impl Lowering<'_, '_> {
    pub(super) fn function_binding(&mut self, callee: ir::Value, function: u32) -> ir::Value {
        let program = self.builder.ins().load(types::I32, MemFlagsData::trusted(), self.payload,
            std::mem::offset_of!(JsStepDispatch, program_id) as i32);
        let program = self.ins().uextend(types::I64, program);
        let probed = function != super::super::ROOT_FUNCTION_ID;
        let function = self.ins().iconst(types::I64, i64::from(function));
        let admitted = if self.call_budget.is_some() && probed {
            let (slow, join) = (self.builder.create_block(), self.builder.create_block());
            self.builder.set_cold_block(slow);
            let admitted = self.builder.append_block_param(join, types::I64);
            let hit = self.cached_call_target(program, callee, function);
            let one = self.ins().iconst(types::I64, 1);
            self.ins().brif(hit, join, &[ir::BlockArg::Value(one)], slow, &[]);
            self.builder.switch_to_block(slow);
            let helper = self.helpers.address(self.builder.ins(), LaneHelper::AdmittedFunctionBinding);
            let called = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function]);
            self.ins().jump(join, &[ir::BlockArg::Value(called)]);
            self.builder.switch_to_block(join);
            admitted
        } else {
            let depth = self.unit.recovery_depth().max(1) as i64;
            let depth = self.ins().iconst(types::I64, depth);
            let helper = self.helpers.address(self.builder.ins(), LaneHelper::FunctionBinding);
            emit_lane_helper_call(&mut self.builder, self.payload, helper, &[program, callee, function, depth])
        };
        self.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0)
    }

    /// Whether the VM's in-loop call target of `callee` names `function` of `program` with no
    /// termination requested: a same-realm closure of that body (`Vm::in_loop_call_target`),
    /// as `lane_admitted_function_binding` decides, without its call.
    fn cached_call_target(&mut self, program: ir::Value, callee: ir::Value, function: ir::Value) -> ir::Value {
        type Target = super::super::InLoopCallTarget;
        let table = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, call_targets));
        let slot = self.ins().band_imm_u(callee, (super::super::CALL_TARGET_CACHE_SIZE - 1) as i64);
        let offset = self.ins().imul_imm_u(slot, std::mem::size_of::<Target>() as i64);
        let entry = self.ins().iadd(table, offset);
        let field = |offset: usize| i32::try_from(offset).expect("call target field offset");
        let named = self.builder.ins().load(types::I64, MemFlagsData::trusted(), entry, field(std::mem::offset_of!(Target, callee)));
        let owner = self.builder.ins().uload32(MemFlagsData::trusted(), entry, field(std::mem::offset_of!(Target, program)));
        let body = self.builder.ins().uload32(MemFlagsData::trusted(), entry, field(std::mem::offset_of!(Target, function)));
        let same = self.ins().icmp(IntCC::Equal, named, callee);
        let owned = self.ins().icmp(IntCC::Equal, owner, program);
        let bound = self.ins().icmp(IntCC::Equal, body, function);
        let hit = self.ins().band(same, owned);
        let hit = self.ins().band(hit, bound);
        if self.fault_poll { return hit; }
        let flag = self.interrupt;
        let requested = self.ins().atomic_load(types::I8, MemFlagsData::trusted(), flag);
        let clear = self.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        self.ins().band(hit, clear)
    }
}
