//! Calls whose bytecode conditionally appends their parameter to one captured binding.
//! The bytecode proves the operation; ordinary capture storage and concatenation own its
//! effects. A miss leaves the whole call to its activation before changing the binding.

use super::*;

// Positions in the recognized body. The load record retains its capture immediate when
// quickened, so the target needs no separate copy of its capture slot.
const LOAD_CAPTURE_PC: usize = 2;
const RESULT_PC: usize = 5;
const RETURN_PC: usize = RESULT_PC + 1;

/// The parameter the recognized body appends: its register is the parameter's own.
const CHUNK_PARAMETER: u16 = 0;

/// Whether `body` is `null !== chunk && (binding += chunk); return true;` over its first
/// parameter and one capture of its own environment, as each instruction's role says.
pub(super) fn recognizes(body: &Function, constants: &[crate::bytecode::Constant]) -> bool {
    if body.params == 0 || !body.handlers.is_empty() {
        return false;
    }
    let Some(&[test, branch, load, add, store, result, done]) = body.code.get(..=RETURN_PC) else {
        return false;
    };
    let [test, branch, load, add, store, result, done] =
        [test, branch, load, add, store, result, done].map(|packed| decoded_instruction(body, packed));
    tests_chunk_is_not_null(test, constants)
        && skips_append_unless(branch, test)
        && loads_own_capture(load)
        && appends_chunk(add, load)
        && stores_back(store, add, load)
        && returns_true(result, done, constants)
}

/// A write that leaves the chunk parameter's register for the append to read.
fn spares_chunk(instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && instruction.result_register() != CHUNK_PARAMETER
}

/// `null !== chunk`, into a register other than the chunk's.
fn tests_chunk_is_not_null(test: WideInstruction, constants: &[crate::bytecode::Constant]) -> bool {
    test.op().is_binary()
        && spares_chunk(test)
        && test.binary_operator() == STRICT_INEQUALITY_OPERATOR
        && test.operand_b().register_index() == Some(CHUNK_PARAMETER)
        && test.operand_c().kind() == Some(crate::bytecode::OperandKind::Constant)
        && matches!(constants.get(usize::from(test.operand_c().payload())), Some(crate::bytecode::Constant::Null))
}

/// A false test jumps over the append to the result.
fn skips_append_unless(branch: WideInstruction, test: WideInstruction) -> bool {
    branch.op().is_jump_false()
        && branch.register_a() == test.result_register()
        && branch.jump_target() as usize == RESULT_PC
}

/// A capture of the body's own environment, into a register other than the chunk's.
fn loads_own_capture(load: WideInstruction) -> bool {
    load.op().is_load_capture() && spares_chunk(load) && load.capture_depth() == 0
}

/// `binding + chunk`, in that order.
fn appends_chunk(add: WideInstruction, load: WideInstruction) -> bool {
    add.op().is_binary()
        && add.result_flags() == 0
        && add.binary_operator() == oxc_ast::ast::BinaryOperator::Addition as u32
        && add.operand_b().register_index() == Some(load.result_register())
        && add.operand_c().register_index() == Some(CHUNK_PARAMETER)
}

/// The sum, stored to the capture the load read.
fn stores_back(store: WideInstruction, add: WideInstruction, load: WideInstruction) -> bool {
    store.op().is_store_capture() && store.register_a() == add.result_register() && store.imm() == load.imm()
}

/// `return true`, whether or not the append ran.
fn returns_true(result: WideInstruction, done: WideInstruction, constants: &[crate::bytecode::Constant]) -> bool {
    result.op().is_load_const()
        && result.result_flags() & !crate::bytecode::RETURN_REGISTER == 0
        && matches!(constants.get(result.constant_index()), Some(crate::bytecode::Constant::Boolean(true)))
        && done.op().is_return()
        && done.register_a() == result.result_register()
}

/// The recognized call, without activating it. No guest code or collection runs here.
///
/// # Safety
/// `target` belongs to this lane's program and has entry `AppendsCapture`; its load record
/// and capture storage remain live until a collection, which concatenation only schedules.
#[inline(always)]
pub(super) unsafe fn run<H: Host>(
    cx: *mut LaneContext<H>,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
) -> Option<Value> {
    let chunk = arguments.get(0).unwrap_or(Value::UNDEFINED);
    if chunk.is_null() {
        return Some(Value::TRUE);
    }
    // This leaf allocates, so retain the call's ordinary collection/stack gate even
    // though a hit writes no activation. A due collection must reach the general path.
    if !lane_activation_room(unsafe { vm(cx) }, target) {
        return None;
    }
    let (storage, slot, previous) = unsafe { writable_binding(target) }?;
    let value = lane_concatenation(cx, previous, chunk)?;
    // SAFETY: no guest code or collection can have changed this initialized binding.
    unsafe { crate::heap::EnvironmentSlots::write_storage(storage, slot, value) }
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
    let previous = unsafe { crate::heap::EnvironmentSlots::read_storage(slots, u16::try_from(slot).ok()?) }?;
    Some((slots, slot, previous))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, test_host::TestHost};
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
    fn printed(program: &ResidualProgram, test262: bool, compiled: bool) -> (Vm<TestHost>, Vec<String>) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let host = TestHost::writing_to(output.clone());
        let mut vm = Vm::new(if test262 { host.with_test262() } else { host });
        if compiled {
            vm.set_execution_mode(crate::ExecutionMode::Compiled);
        }
        vm.execute(program).unwrap();
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
                Engine::compile(crate::ExecutionRequest::script(EFFECTS_SOURCE, name))
            }
            .unwrap();
            let (_, lines) = printed(&program, true, compiled);
            assert_eq!(lines, [EFFECTS_PRINTED], "specialized={specialized} compiled={compiled}");
        }
    }

    /// The append target the ordinary calls of `vm`'s run resolved.
    fn append_target(vm: &Vm<TestHost>) -> ScriptCallTarget {
        vm.lane_stack
            .script_targets
            .entries
            .iter()
            .find(|target| target.entry == CallEntry::AppendsCapture)
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
                recognizes(body, &program.constants)
                    .then(|| program.atoms[body.name.unwrap() as usize].to_string())
            })
            .collect();
        assert_eq!(admitted, ["append"]);
    }
}
