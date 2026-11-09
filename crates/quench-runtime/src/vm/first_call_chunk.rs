//! Functions whose load deferred their compilation (`CallFirstChunk`): the load
//! compiled a stub in their place, and the first call compiles the function's source as a
//! program of its own, a chunk, whose function then stands for it. Every closure of the
//! stub shares the chunk, compiled once.
use super::*;
use std::rc::{Rc, Weak};

/// The chunk of each stub, by the stub's program and function: the programs are held weakly
/// and checked on each read, as either id may name another program once its own is reclaimed.
pub(super) type FirstChunks = FxHashMap<(ProgramId, u32), (Weak<ResidualProgram>, Weak<ResidualProgram>, FunctionKind)>;

impl<H: Host> Vm<H> {
    /// `i`, at `pc`, the stub body of frame `f`'s callee (`CallFirstChunk`): points the
    /// callee at its chunk's function, compiling the chunk if the callee still runs the stub,
    /// then calls or constructs it as the stub was, with the arguments array of register B.
    pub(super) fn call_first_chunk(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let frame = &mut *self.frames[f];
        (frame.pc, frame.pc_is_resume) = (pc, true);
        let (context, this, stub) = (frame.context, frame.this, (frame.program, frame.function));
        let callee = context.callable().ok_or_else(|| JsError::validation("a first-call stub runs without a callee".into()))?;
        let args = self.array_values(self.read(f, i.register_b()))?;
        if self.heap.function(callee).is_some_and(|record| matches!(record.kind, FunctionKind::User(program, function) if (program, function) == stub)) {
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

    /// The chunk of stub `function` of program `p` (`stub.0`): the function's source alone,
    /// compiled as a Script at its place in `p`'s text.
    fn compile_first_chunk(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32)) -> Result<FunctionKind, JsError> {
        let live = |id: ProgramId, held: &Weak<ResidualProgram>| self.programs.get(id).is_some_and(|now| held.upgrade().is_some_and(|held| Rc::ptr_eq(&now, &held)));
        if let Some((stub, chunk, kind @ FunctionKind::User(id, _))) = self.first_chunks.get(&(program, function))
            && live(program, stub)
            && live(*id, chunk)
        {
            return Ok(*kind);
        }
        let kind = self.compile_first_chunk_anew(p, (program, function))?;
        if let FunctionKind::User(id, _) = kind
            && let (Some(stub), Some(chunk)) = (self.programs.get(program), self.programs.get(id))
        {
            self.first_chunks.retain(|_, (stub, chunk, _)| stub.strong_count() > 0 && chunk.strong_count() > 0);
            self.first_chunks.insert((program, function), (Rc::downgrade(&stub), Rc::downgrade(&chunk), kind));
        }
        Ok(kind)
    }

    fn compile_first_chunk_anew(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32)) -> Result<FunctionKind, JsError> {
        let span = p.functions[function as usize].header().source_span
            .ok_or_else(|| JsError::validation("a first-call stub keeps no source".into()))?;
        let residual = crate::Engine::specialize_first_call_chunk(p, function, span, &*self)
            .map_err(|_| JsError::validation("a first-call chunk does not compile".into()))?;
        let target = residual
            .functions
            .iter()
            .position(|slot| slot.header().parent == Some(0) && slot.header().source_span.is_some_and(|own| own.start == span.start))
            .ok_or_else(|| JsError::validation("a first-call chunk holds no function".into()))?;
        let chunk = self.store_dynamic_program(residual).ok_or_else(|| self.type_error(p, "dynamic program store is full".into()))?;
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
