//! The record admissions a body makes once at its entry (`invariant_field_records`): each receiver an access names gets its fixed
//! class's cell, or its record where its reads need one. A class-keyed body (`class_keyed_reads`) admits no record at all.

use super::*;

impl Lowering<'_, '_> {
    /// Bind physical receiver facts once. The expanded unit's write set owns
    /// invariance, including renamed callee registers and synthesized returns.
    pub(super) fn invariant_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver, this: Variable,
        miss: &mut Option<ir::Block>, receiver_only: bool) -> Vec<Option<ObjectRecord>> {
        let count = usize::from(self.unit.registers);
        // A class-keyed body admits no record: its reads start from the live cell (`class_keyed_reads`), so the only owners
        // admitted are the fixed classes' cells, which need no test.
        let class_keyed = self.class_keyed_reads(instructions);
        let mut owners: BitSet = BitSet::with_capacity(count + 1);
        // Per owner, the fixed class every access names (`fixed_receiver_class`); None once an access names none, or two classes.
        let mut named: Vec<Option<Option<u32>>> = vec![None; count + 1];
        let mut reachable = false;
        for (pc, instruction) in instructions.iter().enumerate() {
            if self.blocks[pc].is_some() { reachable = self.facts.at(pc).is_some(); }
            if !reachable { continue; }
            let owner = self.field_owner(pc, *instruction);
            if let Some(owner) = owner.filter(|owner| *owner == count
                || (!receiver_only && *owner < usize::from(self.unit.caller_registers) && !self.written.contains(*owner))) {
                owners.insert(owner);
                let receiver_of = if owner == count { quench_runtime::staged::Receiver::This { path: None } }
                    else { quench_runtime::staged::Receiver::Register(owner as Register) };
                let class = self.fixed_receiver_class(pc, receiver_of);
                named[owner] = Some(match named[owner] {
                    None => class,
                    Some(held) if held == class => held,
                    Some(_) => None,
                });
            }
            if instruction.returns_from_frame() || instruction.op().control_flow_layout() == ControlFlowLayout::Terminal {
                reachable = false;
            }
        }
        let mut admitted = vec![None; count + 1];
        let mut classes = None;
        for owner in owners.iter() {
            // A fixed class's receiver is its class cell from birth (`ObjectRecord::prove_fixed`): no presence, class or shape test.
            if named[owner].flatten().is_some() {
                admitted[owner] = Some(if owner == count {
                    let bits = headers::this_word(&mut self.builder, this);
                    self.fixed_record(bits)
                } else {
                    let base = self.registers_base;
                    let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(owner as Register));
                    self.fixed_record(bits)
                });
                continue;
            }
            if class_keyed { continue; }
            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
            admitted[owner] = Some(if owner == count {
                // emit_lane_receiver already owns family/range/base admission.
                let record = self.builder.use_var(receiver.object);
                let present = self.raw_compare_imm(Relation::NotEqual, record, 0);
                emit_js_guard(&mut self.builder, present, exit);
                ObjectRecord::Address(record)
            } else {
                // An unwritten register holds its entry value at either start, in the file.
                let base = self.registers_base;
                let bits = self.raw_load(Lane::I64, regions::frame(), base, register_offset(owner as Register));
                let class_base = *classes.get_or_insert_with(|| emit_js_payload_word(&mut self.builder,
                    self.payload, std::mem::offset_of!(JsStepDispatch, head.class_base)));
                ObjectRecord::prove(&mut self.builder, bits, class_base, None, exit)
            });
        }
        admitted
    }
}
