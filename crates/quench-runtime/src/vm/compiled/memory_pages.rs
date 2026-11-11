//! `memory.size` and `memory.grow` in compiled Wasm: the size reads the context's length; a
//! grow calls out to the VM, which grows a memory compiled code holds only within its
//! reservation (`Analysis::callouts`), so the base stays and only the context's length moves.
use super::*;

/// The `compiled_memory_grow` result that stands for a raised VM error: no page count.
const GROW_RAISED: i64 = i64::MIN;

/// `memory.grow` of the compiled memory by `delta` pages: the old size in pages, -1 when the
/// memory cannot grow, or `GROW_RAISED` once the VM's error waits in the context.
unsafe extern "C" fn compiled_memory_grow<H: Host>(context: *mut CallContext, delta: u64) -> i64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose VM is suspended in
    // that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let grown = vm.wasm_memory_grow(context.memory, delta);
    ACTIVE_CONTEXT.with(|current| current.set(active));
    refresh_memory_length(context, vm);
    match grown {
        Ok(pages) => pages.map_or(-1, |pages| pages as i64),
        Err(error) => {
            context.raised = Some(Raised::Error(error));
            GROW_RAISED
        }
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Check a size or grow names the function's compiled memory, and a grow's delta is an
    /// i32 where callouts keep the memory's base.
    pub(super) fn pages_operands(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &AnalysisState,
        memory_register: Option<u16>,
        memory_slot: Option<u32>,
    ) -> Option<()> {
        let memory = instruction.b();
        let owned = Some(memory) == memory_register
            && state.get(memory, function.registers) == Some(Slot::Memory)
            && memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot));
        let grows = instruction.op().class() == Class::WasmMemoryGrow;
        let delta = || state.get(instruction.c(), function.registers) == Some(Slot::Value(Kind::I32));
        (owned && (!grows || (self.callouts && delta()))).then_some(())
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// `memory.size`, or `memory.grow` by the pages in register `delta`, into `result`.
    pub(super) fn memory_pages(&mut self, op: Op, result: u16, delta: u16) {
        let pages = if op.class() == Class::WasmMemoryGrow {
            self.memory_grow(delta)
        } else {
            let builder = &mut self.body.core.builder;
            let bytes = builder.ins().load(types::I64, MemFlagsData::trusted(), self.body.core.context, CONTEXT_MEMORY_LENGTH);
            builder.ins().ushr_imm_u(bytes, i64::from(crate::wasm::memory::WASM_PAGE_BITS))
        };
        let pages = self.body.core.builder.ins().ireduce(types::I32, pages);
        self.write(result, Kind::I32, pages);
    }

    /// The old size in pages of a grow through `compiled_memory_grow`, leaving on a raised error.
    fn memory_grow(&mut self, delta: u16) -> ir::Value {
        let delta = self.read(delta, Kind::I32);
        let builder = &mut self.body.core.builder;
        let delta = builder.ins().uextend(types::I64, delta);
        let mut signature = Signature::new(self.call_conv);
        signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
        signature.returns.push(AbiParam::new(types::I64));
        let signature = builder.import_signature(signature);
        let target = builder.ins().iconst(types::I64, compiled_memory_grow::<H> as *const () as usize as i64);
        let call = builder.ins().call_indirect(signature, target, &[self.body.core.context, delta]);
        let pages = builder.inst_results(call)[0];
        let raised = builder.ins().icmp_imm_s(IntCC::Equal, pages, GROW_RAISED);
        let raised_status = builder.ins().iconst(types::I64, RAISED_STATUS);
        let returned = builder.ins().iconst(types::I64, 0);
        let status = builder.ins().select(raised, raised_status, returned);
        self.propagate(status);
        pages
    }
}
