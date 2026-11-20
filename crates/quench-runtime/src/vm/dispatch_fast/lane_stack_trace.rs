//! A stack capture beside the lane's activations (frame-switches.md §5, cause #5): an error
//! construction, `Error.captureStackTrace` or any other capture a native running beside the
//! activations makes (`DeferredLaneCall`) reads the activations from the lane's frame walk
//! (`LaneStack::frames_innermost`) instead of making them VM frames first
//! (`Vm::materialize_deferred_lane`). Each activation reads as the frames its materialization
//! would push (`Vm::materialize_lane_activations`): the same program, function, call context and
//! receiver, the pc its caller's frame would resume at, and the field site a nested read's
//! getter call reads from (`Vm::called_field_source`), so the captured call sites, and the trace
//! text formatted from them, are the ones the materialized frames give. A skipped forwarding
//! constructor (`Completion::Forwarder`) reads as the two frames it becomes, a generator's or a
//! job's windowed activation as its owner's frame record, and a job's run has no root frame
//! beneath its activations. A Wasm frame, or a lane in a state the walk does not cover,
//! declines, and the capture materializes as before.
use super::*;
use super::super::activation::{FrameRecord, SuspendKind};
use super::super::error::FrameSite;

/// The frames a stack capture reads beside a lane's activations (`Vm::lane_stack_sites`).
pub(in crate::vm) struct LaneSites {
    /// The source pc of the lane's root frame, the innermost VM frame: its call of the outermost
    /// activation, or the deferred call where no activation is (`Vm::resume_caller_frame`,
    /// `Vm::resume_deferred_caller`). `None` for a job's run, which has no root frame: its bottom
    /// activation becomes the general loop's entry frame above the VM frames as they are.
    pub(in crate::vm) root_pc: Option<usize>,
    /// The activations outermost first, each as the frames its materialization pushes.
    pub(in crate::vm) sites: Vec<FrameSite>,
}

/// The source pc of the frame an activation's caller becomes, paused at the call of `above`
/// (`ActivationView::caller_pc`): `resume_caller_frame` resumes it at `caller_resume`'s pc, after the call
/// record, or at the `yield*` itself for a delegate step, and a resumed frame's source pc is
/// the one before (`Frame::source_instruction_pc`).
fn paused_at(above: &ActivationView) -> Option<usize> {
    let call_pc = above.caller_pc()?;
    Some(above.record().caller_resume(call_pc).1.saturating_sub(1))
}

impl<H: Host> Vm<H> {
    /// The frames a stack capture reads while a native runs beside the lane's activations
    /// (`lane_deferred`), read from the frame walk: the root frame's pc and the sites of the
    /// activations' frames. `None`, changing nothing but linking native frames (as the
    /// materialization's first step does, `link_natives`), where no native runs beside
    /// activations, where the lane runs a deferred native's callback (`NativeBoundary`), where
    /// frames were pushed since the call deferred, or where an activation is no script
    /// activation (Wasm) or its frame's state cannot be read: the caller then materializes.
    pub(in crate::vm) fn lane_stack_sites(&mut self) -> Option<LaneSites> {
        let DeferredLaneCall { cx, call_pc, frames } = self.lane_deferred?;
        let cx = cx.cast::<LaneContext<H>>();
        // SAFETY: the deferred call's lane runs, so its context lives.
        let (root, innermost_program) = unsafe { ((*cx).root, (*cx).program) };
        if frames != self.frames.len()
            || root.frame().is_some_and(|root_frame| root_frame + 1 != frames)
            || self.lane_stack.native_boundary.is_some()
        {
            return None;
        }
        self.link_natives(cx);
        // Innermost first; each frame a copy (`ActivationView`), read where it is.
        let walked: Vec<ActivationView> = self.lane_stack.frames_innermost().collect();
        if walked.iter().any(|frame| frame.script().is_none()) {
            return None;
        }
        // Each frame resumes after the call of the frame above it, the innermost after the
        // deferred call; a job's bottom activation has no caller frame.
        let root_pc = match (root, walked.last()) {
            (LaneRoot::Job(_), _) => None,
            (LaneRoot::Frame(_), Some(outermost)) => Some(paused_at(outermost)?),
            (LaneRoot::Frame(_), None) => Some(call_pc),
        };
        let mut sites = Vec::with_capacity(walked.len() + 1);
        for (index, activation) in walked.iter().enumerate().rev() {
            // Its own program is the caller program of the frame above it, the innermost the lane's
            // (`ActivationState::of`).
            let (program, pc) = match index.checked_sub(1).map(|above| &walked[above]) {
                Some(above) => (above.caller_program(), paused_at(above)?),
                None => (innermost_program, call_pc),
            };
            self.push_activation_sites(root, program, activation, pc, &mut sites)?;
        }
        Some(LaneSites { root_pc, sites })
    }

    /// Appends the frames `activation` of `program`, paused at source pc `pc`, becomes where the
    /// activations materialize (`materialize_lane_activations`): its owner's frame record for a
    /// windowed one (`push_window_frame`), the skipped forwarding constructor's frame below the
    /// method's for a forwarder, else the frame `push_script_frame_with_registers` pushes. Each
    /// is paused at a call, so it carries the field site its call reads from.
    fn push_activation_sites(
        &mut self,
        root: LaneRoot,
        program: ProgramId,
        activation: &ActivationView,
        pc: usize,
        sites: &mut Vec<FrameSite>,
    ) -> Option<()> {
        let script = activation.script()?;
        if script.completion.is_windowed() {
            let record = self.windowed_record(root, &script)?;
            let (program, function) = (record.program, record.function);
            let site = FrameSite { program, function, context: record.context, this: record.this(), pc, source_field: None };
            sites.push(FrameSite { source_field: self.called_field_source(program, function, pc), ..site });
            return Some(());
        }
        let window = Registers(activation.window_ptr());
        let (this, env) = (window.this(), window.env());
        if script.completion == Completion::Forwarder {
            // The construction's skipped forwarding constructor, pending at its forwarding call:
            // its descriptor's for a native frame (`NativeFrames`), else its side record's.
            let record = activation.record();
            // SAFETY: a native frame's callee outlives the run.
            let forwarder = unsafe { record.callee.as_ref() }
                .map_or_else(|| activation.payload().forwarder(), |callee| callee.forwarder)?;
            let forwarder_pc = forwarder.method.call_pc as usize;
            sites.push(FrameSite {
                program,
                function: forwarder.function,
                context: CallContext::user_function(forwarder.function, forwarder.callee).constructed(),
                this: self.bound_receiver(program, forwarder.function, this, forwarder.env)?,
                pc: forwarder_pc,
                source_field: self.called_field_source(program, forwarder.function, forwarder_pc),
            });
        }
        let function = activation.function();
        let context = CallContext::user_function(function, script.callee);
        sites.push(FrameSite {
            program,
            function,
            context: if script.completion.constructs() { context.constructed() } else { context },
            this: self.bound_receiver(program, function, this, env)?,
            pc,
            source_field: self.called_field_source(program, function, pc),
        });
        Some(())
    }

    /// The frame record a windowed activation of a lane run with `root` runs over, read where its
    /// owner keeps it: the job's (an async function's await state, a generator's record) for a
    /// job's root, else its generator's (`push_window_frame`).
    fn windowed_record(&self, root: LaneRoot, script: &ScriptActivation) -> Option<&FrameRecord> {
        let owner = match root {
            LaneRoot::Job(job) if script.completion == Completion::ResumesJob => match job.kind {
                SuspendKind::AsyncFunction => return self.heap.await_state(job.object)?.running_frame(),
                SuspendKind::AsyncGenerator | SuspendKind::Generator => job.object,
            },
            _ => script.callee,
        };
        self.heap.iterator(owner)?.ext.generator.as_deref()?.windowed_frame()
    }

    /// The receiver the frame of `function` of `program` binds from its window's `this` over
    /// `env`, as `activate_plain_frame` or `activate_general_frame` binds it: an arrow's
    /// captured `this`, `DELETED` for a derived constructor's general frame, else
    /// `call_this_value`.
    fn bound_receiver(&mut self, program: ProgramId, function: u32, this: Value, env: Value) -> Option<Value> {
        let (arrow, derived, strict, plain) = {
            let code = self.programs.residual(program)?;
            let body = code.function_get(function as usize)?;
            let plain = function != super::super::ROOT_FUNCTION_ID && body.plain_activation(code);
            (body.is_arrow, body.derived_constructor, body.strict, plain)
        };
        if arrow {
            Some(self.captured_lexical_this(env).unwrap_or(this))
        } else if derived && !plain {
            Some(Value::DELETED)
        } else {
            self.call_this_value(this, strict).ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, test_host::TestHost};
    use std::{cell::RefCell, rc::Rc};

    /// frame-switches.md §5: every trace a warmed lane captures beside its activations (nested
    /// calls, a getter, an array callback, a constructor, `Error` called without `new`, a method,
    /// a direct eval, an async function before its first `await`, and `Error.captureStackTrace`
    /// trimmed through a function on the stack, as chai's AssertionError does) is byte-identical
    /// to the one the first, cold round captured, and the frames are Node's (v22). §5.2 adds a
    /// skipped forwarding constructor's method (`Completion::Forwarder`: two frames), a generator
    /// stepped by `next()` (a windowed activation), a nested read's getter (its caller's field
    /// site, `Vm::called_field_source`), defines that raise beside activations (`lane_raise`,
    /// caught in the lane or not), and an async function after its `await` (a job's root), whose
    /// traces the jobs compare after the rounds. §5.3 adds operations made in place that raise
    /// beside activations (`unwound_in_place`): `undefined.x` and a BigInt plus Number caught by
    /// the caller, `null[0]` caught in the raising function, their messages and frames Node's.
    #[test]
    fn lane_stack_captures_match_materialized_traces() {
        let source = r#"
          function Assertion(message, ssf) { this.message = message; Error.captureStackTrace(this, ssf || Assertion); }
          Assertion.prototype = Object.create(Error.prototype);
          function assertOk(v) { return v ? null : new Assertion("not ok", assertOk); }
          function viaCapture() { return assertOk(false).stack; }
          function inner() { return new Error("inner").stack; }
          function middle() { return inner(); }
          function outer() { return middle(); }
          var holder = { get prop() { return new TypeError("getter").stack; } };
          function viaGetter() { return holder.prop; }
          function viaCallback() { var s; [1].forEach(function each() { s = new RangeError("cb").stack; }); return s; }
          function Thing() { this.trace = new Error("ctor").stack; }
          function viaConstructor() { return new Thing().trace; }
          function viaCall() { return Error("call").stack; }
          function viaMethod() { return { m: function m() { return new Error("m").stack; } }.m(); }
          function viaEval() { return eval("new Error('ev').stack"); }
          var asyncTrace;
          async function settles() { asyncTrace = new Error("async").stack; }
          function viaAsync() { settles(); return asyncTrace; }
          function Klass() { this.initialize.apply(this, arguments); }
          Klass.prototype.initialize = function initialize(m) { this.trace = new Error(m).stack; };
          function viaForwarder() { return new Klass("fw").trace; }
          function* traces() { for (;;) yield new Error("gen").stack; }
          var stepped = traces();
          function viaGenerator() { return stepped.next().value; }
          var nest = { inner: holder };
          function viaNestedGetter() { var o = nest; return o.inner.prop; }
          class Sealing { a = Object.preventExtensions(this); b = 2; }
          function viaDefine() { try { new Sealing(); } catch (e) { return e.stack; } }
          class Base { constructor(o) { return o; } }
          class Stamp extends Base { #x = 1; }
          function viaPrivate() { var o = {}; new Stamp(o); try { new Stamp(o); } catch (e) { return e.stack; } }
          function caughtHere() { try { new Sealing(); return "none"; } catch (e) { return e.message; } }
          function viaCaught() { return caughtHere(); }
          function readField(o) { var v = o.x; return v; }
          function viaUndefinedField() { try { readField(undefined); return "none"; } catch (e) { return e.stack; } }
          function readIndex(o) { try { var v = o[0]; return v; } catch (e) { return e.stack; } }
          function viaNullIndex() { return readIndex(null); }
          function mix(a, b) { var s = a + b; return s; }
          function viaMixedAdd() { try { mix(1n, 1); return "none"; } catch (e) { return e.stack; } }
          var jobTraces = [];
          async function afterAwait() { await null; jobTraces.push(new Error("job").stack); }
          function round() {
            afterAwait();
            return [outer(), viaCapture(), viaGetter(), viaCallback(), viaConstructor(), viaCall(), viaMethod(), viaEval(), viaAsync(),
              viaForwarder(), viaGenerator(), viaNestedGetter(), viaDefine(), viaPrivate(), viaCaught(),
              viaUndefinedField(), viaNullIndex(), viaMixedAdd()];
          }
          function names(trace, n) {
            return trace.split("\n").slice(1, 1 + n).map(function (l) { return l.trim().split(" ")[1]; }).join(",");
          }
          function run() {
            var first = round();
            for (var i = 0; i < 3000; i++) {
              var got = round();
              for (var k = 0; k < got.length; k++) {
                if (got[k] !== first[k]) return "round " + i + " case " + k + "\n" + first[k] + "\n--\n" + got[k];
              }
            }
            var raised = first.slice(15).map(function (t) { return t.split("\n")[0] + "@" + names(t, 3); });
            return [names(first[0], 5), names(first[1], 3)].concat(raised).join("|");
          }
          print(run());
          Promise.resolve().then(function () {
            for (var j = 1; j < jobTraces.length; j++) {
              if (jobTraces[j] !== jobTraces[0]) return print("job " + j + "\n" + jobTraces[0] + "\n--\n" + jobTraces[j]);
            }
            print(jobTraces.length + " jobs|" + names(jobTraces[0], 1));
          });
        "#;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(source, "lane-stack-captures.js").unwrap();
        vm.execute(&program).unwrap();
        let rounds = concat!(
            "inner,middle,outer,round,run|viaCapture,round,run",
            "|TypeError: Cannot read properties of undefined (reading 'x')@readField,viaUndefinedField,round",
            "|TypeError: Cannot read properties of null (reading '0')@readIndex,viaNullIndex,round",
            "|TypeError: Cannot mix BigInt and other types, use explicit conversions@mix,viaMixedAdd,round",
        );
        assert_eq!(output.borrow().as_slice(), [rounds, "3001 jobs|afterAwait"]);
    }
}
