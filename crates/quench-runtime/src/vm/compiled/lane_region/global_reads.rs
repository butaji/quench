//! By-name loads that read the global scope (`Vm::lane_global_reads`).
use super::*;

/// What a by-name load of the global scope reads, as planned.
#[derive(Clone, Copy)]
pub(in crate::vm) enum GlobalRead {
    /// An immutable global's constant (`builtins::immutable_globals`).
    Constant(Value),
    /// A fixed global variable of this global object (`Vm::fixed_global_variable`): no
    /// lexical binding can shadow it, so the site's field cache entry reads it.
    Fixed(Value),
    /// Any other name: the lane's own read.
    Dynamic,
}

impl Lowering<'_, '_> {
    /// `LoadName` or `LoadNameCall` at `pc` of a name the global scope resolves: an immutable
    /// global's constant; a fixed variable's slot through its site's cache entry, else the
    /// lane's own read, which leaves where it misses.
    pub(super) fn global_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let result = instruction.result_register();
        match self.global_reads[&pc] {
            GlobalRead::Constant(constant) => {
                let value = self.ins().iconst(types::I64, constant.to_bits() as i64);
                self.define(state, result, value, RegisterFact::definition(Type::immediate(constant), Representation::Tagged));
            }
            GlobalRead::Fixed(globals) => {
                let exit = self.guard_exit(state, pc as u32);
                let (slow, done) = (self.builder.create_block(), self.builder.create_block());
                self.builder.set_cold_block(slow);
                let value = self.builder.append_block_param(done, types::I64);
                let object = self.ins().iconst(types::I64, globals.to_bits() as i64);
                let read = self.cached_own_read(object, instruction.cache_site_index(), slow, done);
                self.ins().jump(done, &[ir::BlockArg::Value(read)]);
                self.builder.switch_to_block(slow);
                let read = self.lane_global_read(instruction, exit);
                self.ins().jump(done, &[ir::BlockArg::Value(read)]);
                self.builder.switch_to_block(done);
                self.set_tagged(state, result, value);
            }
            GlobalRead::Dynamic => {
                let exit = self.guard_exit(state, pc as u32);
                let value = self.lane_global_read(instruction, exit);
                self.set_tagged(state, result, value);
            }
        }
        // A call's receiver is `undefined` for a binding of the global scope.
        if instruction.op() == Op::LoadNameCall {
            let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
            self.define(state, instruction.register_b(), undefined,
                RegisterFact::definition(Type::immediate(Value::UNDEFINED), Representation::Tagged));
        }
        true
    }

    /// The lane's own read of the name (`compiled::lane_global_get`), which leaves at `exit`
    /// where it misses.
    fn lane_global_read(&mut self, instruction: WideInstruction, exit: ir::Block) -> ir::Value {
        let atom = self.ins().iconst(types::I64, i64::from(instruction.atom_index()));
        let site = self.ins().iconst(types::I64, i64::from(instruction.cache_site_index()));
        let value = self.helper_call(LaneHelper::GlobalGet, &[atom, site]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, hit, exit);
        value
    }

    /// The own data field of `object` that `site`'s field cache entry names, read where the
    /// entry's shape is the object's; anything else goes to `slow`. A narrow slot's value
    /// joins at `done`.
    fn cached_own_read(&mut self, object: ir::Value, site: u16, slow: ir::Block, done: ir::Block) -> ir::Value {
        let packed = self.builder.create_block();
        let packed_address = self.builder.append_block_param(packed, types::I64);
        let (address, _) = emit_js_cached_field_slot(&mut self.builder, self.registers_base, self.payload,
            JsFieldReceiver::Value(object), site, false, slow, None, None, Some(packed));
        let value = self.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
        let current = self.builder.current_block().expect("a read's block");
        self.builder.switch_to_block(packed);
        let narrow = self.ins().load(types::I32, MemFlagsData::trusted(), packed_address, 0);
        let table = match self.helpers {
            #[cfg(feature = "aot")]
            LaneHelpers::Bound(_, symbol) => NarrowTable::Linked(symbol),
            _ => NarrowTable::Process,
        };
        emit_narrow_read(&mut self.builder, narrow, table, done);
        self.builder.switch_to_block(current);
        value
    }
}
