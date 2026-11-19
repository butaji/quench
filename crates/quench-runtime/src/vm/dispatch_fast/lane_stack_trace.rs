//! A stack capture beside the lane's activations (frame-switches.md §5, cause #5): an error
//! construction, `Error.captureStackTrace` or any other capture a native running beside the
//! activations makes (`DeferredLaneCall`) reads the activations from the lane's frame walk
//! (`LaneStack::frames_innermost`) instead of making them VM frames first
//! (`Vm::materialize_deferred_lane`). Each activation reads as the frame its materialization
//! would push (`Vm::materialize_lane_activations`): the same program, function, call context and
//! receiver, and the pc its caller's frame would resume at, so the captured call sites, and the
//! trace text formatted from them, are the ones the materialized frames give. An activation
//! whose materialization pushes something else (a skipped forwarding constructor's extra frame,
//! a generator's or a job's window frame, a Wasm frame) or a lane in a state the walk does not
//! cover declines, and the capture materializes as before.
use super::*;
use super::super::error::FrameSite;

/// The frames a stack capture reads beside a lane's activations (`Vm::lane_stack_sites`).
pub(in crate::vm) struct LaneSites {
    /// The source pc of the lane's root frame, the innermost VM frame: its call of the outermost
    /// activation, or the deferred call where no activation is (`Vm::resume_caller_frame`,
    /// `Vm::resume_deferred_caller`).
    pub(in crate::vm) root_pc: usize,
    /// The activations outermost first, each as the frame its materialization pushes.
    pub(in crate::vm) sites: Vec<FrameSite>,
}

/// One activation as the walk yields it, innermost first, before its receiver is resolved.
struct WalkedActivation {
    function: u32,
    caller_program: ProgramId,
    script: ScriptActivation,
    /// The pc of its call record in its caller's body (`Frame::caller_pc`).
    caller_pc: usize,
    window: Registers,
}

impl<H: Host> Vm<H> {
    /// The frames a stack capture reads while a native runs beside the lane's activations
    /// (`lane_deferred`), read from the frame walk: the root frame's pc and one site per
    /// activation. `None`, changing nothing but linking native frames (as the materialization's
    /// first step does, `link_natives`), where no native runs beside activations, where the
    /// lane is a job's or runs a deferred native's callback (`NativeBoundary`), where frames
    /// were pushed since the call deferred, or where an activation materializes as anything but
    /// one plain script frame (`Completion::Forwarder`, a windowed completion, Wasm): the
    /// caller then materializes.
    pub(in crate::vm) fn lane_stack_sites(&mut self) -> Option<LaneSites> {
        let DeferredLaneCall { cx, call_pc, frames } = self.lane_deferred?;
        let cx = cx.cast::<LaneContext<H>>();
        // SAFETY: the deferred call's lane runs, so its context lives.
        let (root, innermost_program) = unsafe { ((*cx).root, (*cx).program) };
        let root_frame = root.frame()?;
        if frames != self.frames.len() || root_frame + 1 != frames || self.lane_stack.native_boundary.is_some() {
            return None;
        }
        self.link_natives(cx);
        let mut walked: Vec<WalkedActivation> = Vec::with_capacity(self.lane_stack.activation_count());
        for frame in self.lane_stack.frames_innermost() {
            let ActivationKind::Script(script) = frame.kind() else { return None };
            if script.completion.is_windowed() || script.completion == Completion::Forwarder {
                return None;
            }
            walked.push(WalkedActivation {
                function: frame.function(),
                caller_program: frame.caller_program(),
                script,
                caller_pc: frame.caller_pc()?,
                window: Registers(frame.window_ptr()),
            });
        }
        // Each frame resumes after the call of the frame above it (`caller_resume`: no
        // accepted completion is a delegate's), the innermost after the deferred call.
        let root_pc = walked.last().map_or(call_pc, |outermost| outermost.caller_pc);
        let mut sites = Vec::with_capacity(walked.len());
        for (index, activation) in walked.iter().enumerate().rev() {
            let (program, pc) = match index.checked_sub(1).map(|above| &walked[above]) {
                Some(above) => (above.caller_program, above.caller_pc),
                None => (innermost_program, call_pc),
            };
            sites.push(self.activation_site(program, activation, pc)?);
        }
        Some(LaneSites { root_pc, sites })
    }

    /// The frame `activation` of `program`, paused at source pc `pc`, becomes when it
    /// materializes (`push_script_frame_with_registers`): its context, and its receiver as
    /// `activate_plain_frame` or `activate_general_frame` binds it from the window's.
    fn activation_site(&mut self, program: ProgramId, activation: &WalkedActivation, pc: usize) -> Option<FrameSite> {
        let (arrow, derived, strict, plain) = {
            let code = self.programs.residual(program)?;
            let function = code.function_get(activation.function as usize)?;
            let plain = activation.function != super::super::ROOT_FUNCTION_ID && function.plain_activation(code);
            (function.is_arrow, function.derived_constructor, function.strict, plain)
        };
        let (this, env) = (activation.window.this(), activation.window.env());
        let this = if arrow {
            self.captured_lexical_this(env).unwrap_or(this)
        } else if derived && !plain {
            Value::DELETED
        } else {
            self.call_this_value(this, strict).ok()?
        };
        let script = activation.script;
        let context = CallContext::user_function(activation.function, script.callee);
        Some(FrameSite {
            program,
            function: activation.function,
            context: if script.completion.constructs() { context.constructed() } else { context },
            this,
            pc,
            source_field: None,
        })
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
    /// to the one the first, cold round captured, and the frames are Node's (v22).
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
          function round() {
            return [outer(), viaCapture(), viaGetter(), viaCallback(), viaConstructor(), viaCall(), viaMethod(), viaEval(), viaAsync()];
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
            return names(first[0], 5) + "|" + names(first[1], 3);
          }
          print(run());
        "#;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(source, "lane-stack-captures.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["inner,middle,outer,round,run|viaCapture,round,run"]);
    }
}
