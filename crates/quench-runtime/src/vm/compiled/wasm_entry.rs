//! Entering a Wasm artifact's native code: the per-call context built from the instance's
//! memory and globals, the guarded entry, and the status turned back into a result. Callers
//! convert arguments to raw words first and the raw result word back after.
use super::*;

/// The memory a call of an artifact runs against: its base and length, the guard reservation
/// that stands in for bounds checks, and the memory's binding.
struct EntryMemory {
    base: u64,
    length: u64,
    guard: Option<(*mut u8, usize)>,
    binding: Value,
}

/// The VM callouts one native run made, counted only for diagnostic artifacts.
#[derive(Default)]
struct CalloutCounts {
    import: u64,
    known: u64,
    reference: u64,
    memcpy_fast_path: u64,
}

/// Where compiled code counts its callouts (`CalloutCounts`), null when it does not.
struct CounterPointers {
    import: *mut u64,
    known: *mut u64,
    reference: *mut u64,
    memcpy_fast_path: *mut u64,
}

impl CalloutCounts {
    /// The counters compiled code bumps, or null pointers when nothing counts.
    fn pointers(&mut self, counted: bool) -> CounterPointers {
        let pointer = |counter: &mut u64| if counted { counter as *mut u64 } else { std::ptr::null_mut() };
        CounterPointers {
            import: pointer(&mut self.import),
            known: pointer(&mut self.known),
            reference: pointer(&mut self.reference),
            memcpy_fast_path: pointer(&mut self.memcpy_fast_path),
        }
    }

    fn record(&self, stats: &WasmJitDiagnostics) {
        let add = |cell: &std::cell::Cell<u64>, count: u64| cell.set(cell.get().saturating_add(count));
        add(&stats.compiled_to_vm_callouts, self.import.saturating_add(self.known).saturating_add(self.reference));
        add(&stats.compiled_import_callouts, self.import);
        add(&stats.compiled_known_callouts, self.known);
        add(&stats.compiled_reference_callouts, self.reference);
        add(&stats.emscripten_memcpy_fast_path_hits, self.memcpy_fast_path);
    }
}

impl<H: Host> Vm<H> {
    /// Run function `id` of `compiled` in instance `env` on the raw words `raw`: the raw result
    /// word, or `None` when this instance's memory does not suit the artifact (the call then
    /// belongs to the interpreter).
    pub(in crate::vm) fn enter_compiled_wasm(
        &mut self,
        compiled: &Rc<CompiledProgram>,
        program: ProgramId,
        env: Value,
        id: u32,
        raw: &[u64],
    ) -> Option<Result<u64, JsError>> {
        let result = self.run_compiled_entry(compiled, program, env, id, raw);
        if let Some(stats) = &compiled.diagnostics
            && result.is_none()
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(id as usize)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        result
    }

    fn run_compiled_entry(
        &mut self,
        compiled: &Rc<CompiledProgram>,
        program: ProgramId,
        env: Value,
        id: u32,
        raw: &[u64],
    ) -> Option<Result<u64, JsError>> {
        let function = compiled.function(id as usize)?;
        let memory = self.entry_memory(compiled, env)?;
        let globals = self.entry_globals(compiled, env)?;
        let mut counts = CalloutCounts::default();
        let mut context = self.wasm_call_context(compiled, program, env, &memory, &globals);
        if let Some(stats) = &compiled.diagnostics {
            attach_diagnostics::<H>(&mut context, stats, counts.pointers(true));
        }
        let mut out = 0u64;
        let sample_started = compiled.note_native_entry(id as usize).then(std::time::Instant::now);
        // SAFETY: the entry is compiled code for this function's signature; the context,
        // arguments and result outlive the call, and the memory's base stays put while
        // compiled code runs (a grow stays within the reservation).
        let status = unsafe {
            enter_native(&mut context, memory.base, memory.length, raw.as_ptr(), &mut out, NativeEntryMode::Resumable, function.entry)
        };
        if let Some(started) = sample_started {
            compiled.finish_native_entry_sample(id as usize, started);
        }
        if let Some(stats) = &compiled.diagnostics {
            counts.record(stats);
        }
        Some(entry_status(&mut context, status).map(|()| out))
    }

    /// The memory `compiled` addresses in instance `env`, or `None` when its view does not suit
    /// the artifact's code: no guard for code without bounds checks, or another fixed base.
    fn entry_memory(&self, compiled: &CompiledProgram, env: Value) -> Option<EntryMemory> {
        let Some(slot) = compiled.memory_slot else {
            return Some(EntryMemory { base: 0, length: 0, guard: None, binding: Value::UNDEFINED });
        };
        let binding = self.heap.environment_slot(env, slot as usize)?;
        let (view, guard) = super::dispatch_fast::direct_memory_and_guard(self, binding)?;
        let unguarded = compiled.guarded && guard.is_none();
        let moved = compiled.fixed_base.is_some_and(|fixed| guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed)));
        (!unguarded && !moved).then_some(EntryMemory { base: view.0 as u64, length: view.1 as u64, guard, binding })
    }

    /// The value cells of the globals `compiled` accesses in instance `env`, in its order.
    fn entry_globals(&mut self, compiled: &CompiledProgram, env: Value) -> Option<smallvec::SmallVec<[*mut Value; 8]>> {
        let mut globals = smallvec::SmallVec::new();
        for slot in &compiled.globals {
            let binding = self.heap.environment_slot(env, *slot as usize)?;
            match self.heap.get_mut(binding)? {
                Cell::WasmGlobal { value, .. } => globals.push(value as *mut Value),
                _ => return None,
            }
        }
        Some(globals)
    }

    /// The context a native run of `compiled` in instance `env` reads.
    fn wasm_call_context(
        &mut self,
        compiled: &Rc<CompiledProgram>,
        program: ProgramId,
        env: Value,
        memory: &EntryMemory,
        globals: &[*mut Value],
    ) -> CallContext {
        let poll = self.execution_interrupt.poll_page();
        let (guard_start, guard_end) = memory.guard.map_or((0, 0), |(start, size)| (start as usize, start as usize + size));
        CallContext {
            stack_limit: native_stack_limit(),
            interrupt: self.execution_interrupt.atomic_flag(),
            table: compiled.table.as_ptr(),
            vm: (self as *mut Self).cast(),
            env,
            entry_stack: 0,
            guard_start,
            guard_end,
            globals: globals.as_ptr(),
            callout: import_callout::<H> as Callout as usize,
            call_reference: reference_callout::<H> as ReferenceCallout as usize,
            compiled: Rc::as_ptr(compiled),
            program,
            memory: memory.binding,
            memory_length: memory.length,
            raised: None,
            poll_start: poll.map_or(0, |(start, _)| start),
            poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
            reference_epoch: self.wasm_reference_epoch.as_ptr(),
            reference_fast_caches: compiled.reference_fast_caches_ptr(),
            ..CallContext::native_adapter(std::ptr::null_mut(), std::ptr::null(), None)
        }
    }

    /// The artifact holding compiled code for function `id` of `program`, once one exists:
    /// the module's, else a tier's. Finding none compiles nothing (`run_compiled` does).
    pub(in crate::vm) fn compiled_wasm_holding(&self, program: ProgramId, id: u32) -> Option<Rc<CompiledProgram>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        self.programs.compiled(program).flatten().filter(|compiled| compiled.has_function(id as usize))
            .or_else(|| self.programs.wasm_tier_containing(program, id))
    }

    /// The interpreter's encoding of a compiled function's raw result word.
    pub(super) fn encode_compiled_result(
        &mut self,
        program: ProgramId,
        result: Option<crate::WasmType>,
        out: u64,
    ) -> Result<Value, JsError> {
        let missing = || JsError::validation("missing compiled Wasm result encoding".into());
        Ok(match result {
            Some(ty) => match Kind::of(ty).ok_or_else(missing)? {
                Kind::I32 => Value::integer(out as u32 as i32),
                Kind::I64 => self.encode_bits64(out),
            },
            None => self.programs.constant(program, VOID_RESULT_CONSTANT).ok_or_else(missing)?,
        })
    }
}

/// A native run's status as its outcome: a normal return, termination, the VM error (or panic)
/// a callout raised, or a trap.
fn entry_status(context: &mut CallContext, status: u32) -> Result<(), JsError> {
    match i64::from(status) {
        0 => Ok(()),
        TERMINATED_STATUS => Err(JsError::execution_terminated()),
        RAISED_STATUS => match context.raised.take() {
            Some(Raised::Error(error)) => Err(error),
            Some(Raised::Panic(panic)) => std::panic::resume_unwind(panic),
            None => Err(JsError::validation("compiled Wasm raised without an error".into())),
        },
        trap => Err(JsError::wasm_trap_error(TRAPS[trap as usize - 1])),
    }
}

/// Point a diagnostic artifact's context at its counters and call-site records.
fn attach_diagnostics<H: Host>(context: &mut CallContext, stats: &WasmJitDiagnostics, counters: CounterPointers) {
    context.import_callouts = counters.import;
    context.known_callouts = counters.known;
    context.reference_callouts = counters.reference;
    context.emscripten_memcpy_fast_path_hits = counters.memcpy_fast_path;
    context.call_reference_diagnostic = diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize;
    context.reference_call_sites = stats.reference_call_sites.as_ptr();
    context.reference_call_site_count = stats.reference_call_sites.len();
    context.import_callout_counts = stats.import_callout_counts.as_ptr();
    context.import_callout_count = stats.import_callout_counts.len();
}
