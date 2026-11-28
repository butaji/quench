//! Calls whose bytecode conditionally appends their parameter to one captured binding.
//! The bytecode proves the operation; ordinary capture storage and concatenation own its
//! effects. A miss leaves the whole call to its activation before changing the binding.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

// The load record retains its capture immediate when quickened, so the target needs no separate copy of its
// capture slot. The recognized body's positions are the shared form's (`compile::body_forms::appends_capture`).
const LOAD_CAPTURE_PC: usize = 2;

/// The recognized call, without activating it. No guest code or collection runs here.
///
/// # Safety
/// `target` belongs to this lane's program and has entry `AppendsCapture`; its load record
/// and capture storage remain live until a collection, which concatenation only schedules.
#[inline(always)]
pub(crate) unsafe fn run(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
) -> Option<Value> {
    let chunk = arguments.get(0).unwrap_or(Value::UNDEFINED);
    if chunk.is_null() {
        return Some(Value::TRUE);
    }
    // This leaf allocates, so retain the call's ordinary collection/stack gate even
    // though a hit writes no activation. A due collection must reach the general path.
    if !activation_room_in_lane(unsafe { vm(cx) }, target) {
        return None;
    }
    let (storage, slot, previous) = unsafe { writable_binding(target) }?;
    let value = concat_value(cx, previous, chunk)?;
    // SAFETY: no guest code or collection can have changed this initialized binding.
    unsafe { quench_runtime::tier_surface::EnvironmentSlots::write_storage(storage, slot, value) }
        .expect("admitted capture remains initialized without guest effects");
    Some(Value::TRUE)
}

/// The initialized mutable capture named by the recognized body's load/store pair.
/// Its storage remains stable until guest code or collection runs.
unsafe fn writable_binding(target: &ScriptCallTarget) -> Option<(*const usize, usize, Value)> {
    let slots = capture_stores(target.capture)?;
    // SAFETY: AppendsCapture preparation proves the load and its capture immediate.
    let imm = unsafe { &*(*target.view).records().add(LOAD_CAPTURE_PC) }.imm();
    let slot = usize::from(ImmediateLayout::capture_slot(imm));
    // SAFETY: capture_stores admits direct writes; the shared read rejects TDZ/markers.
    let previous = unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, u16::try_from(slot).ok()?) }?;
    Some((slots, slot, previous))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::compile::body_forms::appends_capture;
    use quench_runtime::{Engine, test_host::TestHost};
    use std::{cell::RefCell, rc::Rc};

    /// Coercion, TDZ, `const`, reentrant appends and collection through one leaf.
    const EFFECTS_SOURCE: &str = r##"const out = [];
function make(initial) {
  let text = initial;
  return {
    push: function(chunk) { null !== chunk && (text += chunk); return !0; },
    read() { return text; },
    set(value) { text = value; }
  };
}
for (let repeat = 0; repeat < 3; repeat++) {
  const sink = make("");
  for (let index = 0; index < 80; index++) sink.push("a😀");
  out.push(sink.read().length, sink.read().slice(-3), sink.push(null));
  $262.gc();
  sink.push("!");
  out.push(sink.read().length);
}
const a = make("");
for (const value of ["x", 12, null, undefined, true, 3n]) out.push(a.push(value));
out.push(a.read());
const hints = [];
const b = make("L");
b.push({ [Symbol.toPrimitive](hint) { hints.push(hint); b.push("inner"); return "R"; } });
out.push(b.read());
b.set({ [Symbol.toPrimitive](hint) { hints.push(hint); return "Q"; } });
b.push("!");
out.push(b.read(), hints.join(","));
try { b.push(Symbol("bad")); } catch (error) { out.push(error.name, b.read()); }
const marker = {};
try { b.push({ [Symbol.toPrimitive]() { throw marker; } }); }
catch (error) { out.push(error === marker, b.read()); }
function constant() {
  const text = "C";
  return function(chunk) { null !== chunk && (text += chunk); return !0; };
}
const immutable = constant();
out.push(immutable(null));
try { immutable("x"); } catch (error) { out.push(error.name); }
function tdz() {
  const push = function(chunk) { null !== chunk && (text += chunk); return !0; };
  out.push(push(null));
  try { push("x"); } catch (error) { out.push(error.name); }
  let text = "T";
  out.push(push("x"), text);
  return push;
}
const late = tdz();
out.push(late("!"));
const numeric = make(7);
numeric.push(3);
out.push(numeric.read());
numeric.push("z");
out.push(numeric.read());
const before = make("old"), after = make("new");
const object = { push: before.push };
object.push("1"); object.push = after.push; object.push("2");
out.push(before.read(), after.read());
print(JSON.stringify(out));
"##;
    const EFFECTS_PRINTED: &str = r##"[240,"a😀",true,241,240,"a😀",true,241,240,"a😀",true,241,true,true,true,true,true,true,"x12undefinedtrue3","LR","Q!","default,default","TypeError","Q!",true,"Q!",true,"TypeError",true,"ReferenceError",true,"Tx",true,10,"10z","old1","new2"]"##;

    /// `program`'s printed lines, run by a VM in the given modes, and that VM.
    fn printed(program: &ResidualProgram, test262: bool, compiled: bool) -> (Vm, Vec<String>) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let host = TestHost::writing_to(output.clone());
        let mut vm = quench_runtime::test_support::vm(if test262 { host.with_test262() } else { host });
        if compiled {
            vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        }
        quench_runtime::test_support::execute(&mut vm, program).unwrap();
        let lines = output.borrow().clone();
        (vm, lines)
    }

    #[test]
    fn capture_append_leaf_preserves_coercion_tdz_const_and_collection() {
        let name = "capture-append-effects.js";
        for (specialized, compiled) in [(false, false), (true, false), (true, true)] {
            let program = if specialized {
                Engine::specialize(EFFECTS_SOURCE, name)
            } else {
                Engine::compile(quench_runtime::ExecutionRequest::script(EFFECTS_SOURCE, name))
            }
            .unwrap();
            let (_, lines) = printed(&program, true, compiled);
            assert_eq!(lines, [EFFECTS_PRINTED], "specialized={specialized} compiled={compiled}");
        }
    }

    /// The append target the ordinary calls of `vm`'s run resolved.
    fn append_target(vm: &Vm) -> ScriptCallTarget {
        vm.lane_stack
            .script_targets
            .entries
            .iter()
            .find(|target| target.entry == CallEntry::frameless(Frameless::AppendsCapture))
            .copied()
            .expect("ordinary calls must resolve to the append operation")
    }

    #[test]
    fn capture_append_leaf_uses_writable_capture_and_never_read_memoizes() {
        let source = r#"
          function make() {
            let text = "";
            return {
              push: function(chunk) { null !== chunk && (text += chunk); return !0; },
              read() { return text; }
            };
          }
          const sink = make();
          function drive(sink) {
            for (let index = 0; index < 128; index++) {
              if (sink.push("xy") !== true) throw new Error("return value");
            }
            print(sink.read());
            // Resolve after any collection at the final backedge or host print.
            sink.push(null);
          }
          drive(sink);
        "#;
        for compiled in [false, true] {
            let program = Engine::specialize(source, "capture-append-leaf.js").unwrap();
            let (vm, lines) = printed(&program, false, compiled);
            assert_eq!(lines, ["xy".repeat(128)]);
            let target = append_target(&vm);
            assert!(capture_stores(target.capture).is_some(), "the primitive calls must admit direct writes");
            assert!(
                vm.lane_stack.read_memos.iter().all(|memo| memo.callee != target.callee),
                "a mutating call must never populate the pure read memo"
            );
        }
    }

    #[test]
    fn capture_append_leaf_callbacks_constructors_and_apply_keep_their_completions() {
        let source = r#"
          function make() {
            let text = "";
            return {
              push: function(chunk, index, array) { null !== chunk && (text += chunk); return !0; },
              read() { return text; }
            };
          }
          const sink = make();
          const out = [sink.push(null), ["A", "B"].every(sink.push), sink.read()];
          const constructed = new sink.push("C");
          out.push(typeof constructed, constructed instanceof sink.push, sink.read());
          sink.push.call(null, "D");
          Reflect.apply(sink.push, undefined, ["E"]);
          out.push(sink.read());
          print(JSON.stringify(out));
        "#;
        for compiled in [false, true] {
            let program =
                Engine::specialize(source, "capture-append-callback-construct.js").unwrap();
            let (_, lines) = printed(&program, false, compiled);
            assert_eq!(lines, [r#"[true,true,"AB","object",true,"ABC","ABCDE"]"#]);
        }
    }

    #[test]
    fn capture_append_leaf_rejects_other_guards_stores_orders_and_returns() {
        let source = r#"
          function make() {
            let text = "", other = "";
            function append(chunk) { null !== chunk && (text += chunk); return !0; }
            function wrongGuard(chunk) { undefined !== chunk && (text += chunk); return !0; }
            function wrongCondition(chunk) { null === chunk && (text += chunk); return !0; }
            function wrongReturn(chunk) { null !== chunk && (text += chunk); return !1; }
            function wrongOrder(chunk) { null !== chunk && (text = chunk + text); return !0; }
            function wrongSlot(chunk) { null !== chunk && (other = text + chunk); return !0; }
            function wrongFinally(chunk) {
              try { null !== chunk && (text += chunk); return !0; }
              finally { other += "!"; }
            }
            function wrongCatch(chunk) {
              try { null !== chunk && (text += chunk); return !0; }
              catch { return !1; }
            }
            return [append, wrongGuard, wrongCondition, wrongReturn, wrongOrder, wrongSlot, wrongFinally, wrongCatch];
          }
          make();
        "#;
        let program = Engine::specialize(source, "capture-append-admission.js").unwrap();
        let admitted: Vec<_> = (0..program.functions.len())
            .filter_map(|index| {
                let body = program.function_get(index)?;
                appends_capture(body, &program.constants)
                    .then(|| program.atoms[body.name.unwrap() as usize].to_string())
            })
            .collect();
        assert_eq!(admitted, ["append"]);
    }
}
