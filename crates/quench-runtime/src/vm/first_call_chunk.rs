//! Functions whose load deferred their compilation (`CallFirstChunk`): the load
//! compiled a stub in their place, and the first call compiles the function's source as a
//! program of its own, a chunk, whose function then stands for it. Every closure of the
//! stub shares the chunk, compiled once.
use super::*;
use std::rc::{Rc, Weak};

/// The chunk of each stub, by the stub's program and function: the programs are held weakly
/// and checked on each read, as either id may name another program once its own is reclaimed.
/// The least function text, in bytes, whose first call may run its body a run at a time.
const STREAMED_RUNS_MIN: u32 = 256 << 10;

/// The stubs whose first call ran their body a run at a time, which a later call compiles
/// whole: a body that runs again keeps its code.
pub(super) type StreamedStubs = FxHashSet<(ProgramId, u32)>;

pub(super) type FirstChunks = FxHashMap<(ProgramId, u32), (Weak<ResidualProgram>, Weak<ResidualProgram>, FunctionKind)>;

impl Vm {
    /// `i`, at `pc`, the stub body of frame `f`'s callee (`CallFirstChunk`): points the
    /// callee at its chunk's function, compiling the chunk if the callee still runs the stub,
    /// then calls or constructs it as the stub was, with the arguments array of register B.
    pub(super) fn call_first_chunk(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let frame = &mut *self.frames[f];
        (frame.pc, frame.pc_is_resume) = (pc, true);
        let (context, this, stub) = (frame.context, frame.this, (frame.program, frame.function));
        let callee = context.callable().ok_or_else(|| JsError::validation("a first-call stub runs without a callee".into()))?;
        let args = self.array_values(self.stub_arguments(f, i))?;
        if self.heap.function(callee).is_some_and(|record| matches!(record.kind, FunctionKind::User(program, function) if (program, function) == stub)) {
            if !context.is_constructor()
                && let Some(value) = self.call_streamed_runs(p, stub, callee, this, &args)?
            {
                self.write(f, i.result_register(), value);
                return Ok(());
            }
            let chunk = self.compile_first_chunk(p, stub)?;
            if let Some(record) = self.heap.function_mut(callee) {
                record.kind = chunk;
            }
        }
        let value = if context.is_constructor() {
            // A `super()` call constructs the stub with its derived class as `new.target`.
            let new_target = Some(self.read(f, i.register_c())).filter(|value| !value.is_undefined()).unwrap_or(callee);
            self.construct_value_with_new_target(p, callee, new_target, &args)?
        } else {
            self.call_value(p, callee, this, &args)?
        };
        self.write(f, i.result_register(), value);
        Ok(())
    }

    /// The arguments array of frame `f`'s stub body, whose `CallFirstChunk` is `i`: its rest
    /// parameter's local, slot 0 as the stub's only parameter, which binding made from the values
    /// passed. Register B, which the body loaded from it, is read only where the slot does not
    /// hold the array (a parameter promoted to a register clears its slot): a lane that ran the
    /// load in a frameless activation and left at this instruction, which no row stages, hands
    /// the general loop the frame's own register file without the load's write (babel-wtb with
    /// stubbed arrows read a stale value there).
    fn stub_arguments(&self, f: usize, i: WideInstruction) -> Value {
        self.frames[f].locals.first().copied()
            .filter(|value| self.heap.array_elements(*value).is_some())
            .unwrap_or_else(|| self.read(f, i.register_b()))
    }

    /// The chunk of stub `function` of program `p` (`stub.0`): the function's source alone,
    /// compiled as a Script at its place in `p`'s text.
    pub fn compile_first_chunk(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32)) -> Result<FunctionKind, JsError> {
        let live = |id: ProgramId, held: &Weak<ResidualProgram>| self.programs.get(id).is_some_and(|now| held.upgrade().is_some_and(|held| Rc::ptr_eq(&now, &held)));
        if let Some((stub, chunk, kind @ FunctionKind::User(id, _))) = self.first_chunks.get(&(program, function))
            && live(program, stub)
            && live(*id, chunk)
        {
            return Ok(*kind);
        }
        let kind = self.compile_first_chunk_anew(p, (program, function), None)?;
        self.remember_first_chunk((program, function), kind);
        Ok(kind)
    }

    /// Records `kind` as the chunk of stub `function` of `program`.
    fn remember_first_chunk(&mut self, (program, function): (ProgramId, u32), kind: FunctionKind) {
        if let FunctionKind::User(id, _) = kind
            && let (Some(stub), Some(chunk)) = (self.programs.get(program), self.programs.get(id))
        {
            // Dead entries are swept only when the table is full, and a sweep that frees under
            // a quarter of it grows it: each entry is swept a bounded number of times. A sweep
            // per insert cost each first call the whole table (2000 stubs: 20K Ir per call).
            if self.first_chunks.len() == self.first_chunks.capacity() {
                self.first_chunks.retain(|_, (stub, chunk, _)| stub.strong_count() > 0 && chunk.strong_count() > 0);
                let capacity = self.first_chunks.capacity();
                if self.first_chunks.len() * 4 > capacity * 3 {
                    self.first_chunks.reserve(capacity);
                }
            }
            self.first_chunks.insert((program, function), (Rc::downgrade(&stub), Rc::downgrade(&chunk), kind));
        }
    }

    /// Forgets the chunks whose programs are gone, which a collection finds out.
    pub(super) fn prune_first_chunks(&mut self) {
        self.first_chunks.retain(|_, (stub, chunk, _)| stub.strong_count() > 0 && chunk.strong_count() > 0);
    }

    /// The first call of stub `stub` of program `p` (`stub.0`), callee `callee`, when its
    /// function's body streams (`StreamRun`): each run of its statements compiles as a chunk
    /// of its own, which a function of the callee's environment runs once, and is dropped.
    /// The function's code is never resident whole, as a body that runs once needs it not;
    /// a later call compiles the function whole (`compile_first_chunk`). `None` where the
    /// body does not stream, the callee then pointed at its chunk.
    fn call_streamed_runs(&mut self, p: &ResidualProgram, stub: (ProgramId, u32), callee: Value, this: Value, args: &[Value]) -> Result<Option<Value>, JsError> {
        use crate::compile::statement_stream::StreamRun;
        let Some(span) = p.functions[stub.1 as usize].header().source_span else { return Ok(None) };
        if span.end - span.start < STREAMED_RUNS_MIN || self.streamed_stubs.contains(&stub) {
            return Ok(None);
        }
        let run = StreamRun::first(span.start);
        let mut kind = self.compile_first_chunk_anew(p, stub, Some(&run))?;
        let Some((body, mut next)) = run.found.get().filter(|(_, next)| next.is_some()) else {
            self.remember_first_chunk(stub, kind);
            if let Some(record) = self.heap.function_mut(callee) {
                record.kind = kind;
            }
            return Ok(None);
        };
        self.streamed_stubs.insert(stub);
        let (realm, env, proto) = match self.heap.function(callee) {
            Some(record) => (self.record_realm(record), record.env, record.object.proto()),
            None => return Err(JsError::validation("a first-call stub runs without its function".into())),
        };
        loop {
            let function = self.alloc_function_in(crate::heap::FunctionRecord { object: Self::empty_object(proto), env, kind }, realm);
            self.call_value(p, function, this, args)?;
            let Some(from) = next else { return Ok(Some(Value::UNDEFINED)) };
            let run = StreamRun::after(span.start, from, body);
            kind = self.compile_first_chunk_anew(p, stub, Some(&run))?;
            next = run.found.get().ok_or_else(|| JsError::validation("a streamed run does not compile".into()))?.1;
        }
    }

    fn compile_first_chunk_anew(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32), run: Option<&crate::compile::statement_stream::StreamRun>) -> Result<FunctionKind, JsError> {
        #[cfg(feature = "source")]
        { self.compile_first_chunk_anew_compiled(p, (program, function), run) }
        #[cfg(not(feature = "source"))]
        {
            let _ = (p, program, function, run);
            Err(JsError::validation("dynamic source compilation requires the source feature".into()))
        }
    }

    /// An execution-only build without the source frontend compiles no source at run time.
    #[cfg(feature = "source")]
    fn compile_first_chunk_anew_compiled(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32), run: Option<&crate::compile::statement_stream::StreamRun>) -> Result<FunctionKind, JsError> {
        if run.is_none()
            && let Some(appended) = self.append_first_chunk(p, (program, function))
        {
            return Ok(appended);
        }
        let span = p.functions[function as usize].header().source_span
            .ok_or_else(|| JsError::validation("a first-call stub keeps no source".into()))?;
        // A body too deep for the stack reports it here, where it compiles, as V8 does.
        let residual = match crate::Engine::specialize_first_call_chunk(p, function, span, &*self, run) {
            Ok(residual) => residual,
            Err(errors) if errors.iter().any(crate::compile::Diagnostic::is_stack_exhausted) => return Err(self.stack_exhaustion_error()),
            Err(_) => return Err(JsError::validation("a first-call chunk does not compile".into())),
        };
        let target = residual
            .functions
            .iter()
            .position(|slot| slot.header().parent == Some(0) && slot.header().source_span.is_some_and(|own| own.start == span.start))
            .ok_or_else(|| JsError::validation("a first-call chunk holds no function".into()))?;
        let chunk = self.store_dynamic_program(residual).ok_or_else(|| self.type_error(p, "dynamic program store is full".into()))?;
        self.programs.inherit_scope(chunk, program);
        if let Some(caller) = self.programs.eval_origin(program) {
            self.programs.set_origin(chunk, program_store::CompilationOrigin::Eval { caller: Some(caller) });
        }
        if let Some(name) = self.programs.script_or_module(program) {
            self.programs.set_script_or_module(chunk, name);
        }
        Ok(FunctionKind::User(chunk, target as u32))
    }
}

#[cfg(test)]
#[path = "first_call_chunk_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "first_call_direct_tests.rs"]
mod direct_tests;
