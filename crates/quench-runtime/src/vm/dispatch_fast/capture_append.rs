//! Calls whose bytecode conditionally appends their parameter to one captured binding.
//! The bytecode proves the operation; ordinary capture storage and concatenation own its
//! effects. A miss leaves the whole call to its activation before changing the binding.

use super::*;

// Positions in the recognized body. The load record retains its capture immediate when
// quickened, so the target needs no separate copy of its capture slot.
const LOAD_CAPTURE_PC: usize = 2;
const RESULT_PC: usize = 5;
const RETURN_PC: usize = RESULT_PC + 1;

pub(super) fn recognizes(body: &Function, constants: &[crate::bytecode::Constant]) -> bool {
    if body.params == 0 || !body.handlers.is_empty() {
        return false;
    }
    let Some(&[test, branch, load, add, store, result, done]) = body.code.get(..=RETURN_PC) else {
        return false;
    };
    let [test, branch, load, add, store, result, done] =
        [test, branch, load, add, store, result, done]
            .map(|packed| decoded_instruction(body, packed));
    test.op() == Op::Binary
        && test.result_flags() == 0
        && test.result_register() != 0
        && test.binary_operator() == STRICT_INEQUALITY_OPERATOR
        && test.operand_b().register_index() == Some(0)
        && test.operand_c().kind() == Some(crate::bytecode::OperandKind::Constant)
        && matches!(
            constants.get(usize::from(test.operand_c().payload())),
            Some(crate::bytecode::Constant::Null)
        )
        && branch.op() == Op::JumpFalse
        && branch.register_a() == test.result_register()
        && branch.jump_target() as usize == RESULT_PC
        && load.op() == Op::LoadCapture
        && load.result_flags() == 0
        && load.result_register() != 0
        && load.capture_depth() == 0
        && add.op() == Op::Binary
        && add.result_flags() == 0
        && add.binary_operator() == oxc_ast::ast::BinaryOperator::Addition as u32
        && add.operand_b().register_index() == Some(load.result_register())
        && add.operand_c().register_index() == Some(0)
        && store.op() == Op::StoreCapture
        && store.register_a() == add.result_register()
        && store.imm() == load.imm()
        && result.op() == Op::LoadConst
        && result.result_flags() & !crate::bytecode::RETURN_REGISTER == 0
        && matches!(
            constants.get(result.constant_index()),
            Some(crate::bytecode::Constant::Boolean(true))
        )
        && done.op() == Op::Return
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
    let slots = capture_stores(target.capture)?;
    // SAFETY: the recognized load's immediate stays in its original record.
    let imm = unsafe { &*(*target.view).records().add(LOAD_CAPTURE_PC) }.imm();
    let slot = usize::from(ImmediateLayout::capture_slot(imm));
    // SAFETY: capture_stores proves direct writes; dense_plain_mut rejects TDZ/markers.
    // Retain a pointer rather than a borrow across allocation; no collection runs here.
    let binding = unsafe { &mut *slots }.dense_plain_mut(slot)? as *mut Value;
    let value = lane_concatenation(cx, unsafe { *binding }, chunk)?;
    // SAFETY: no guest code or collection can have changed this initialized binding.
    unsafe { *binding = value };
    Some(Value::TRUE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, test_host::TestHost};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn capture_append_leaf_preserves_coercion_tdz_const_and_collection() {
        let source = r##"const out = [];
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
        let expected = r##"[240,"a😀",true,241,240,"a😀",true,241,240,"a😀",true,241,true,true,true,true,true,true,"x12undefinedtrue3","LR","Q!","default,default","TypeError","Q!",true,"Q!",true,"TypeError",true,"ReferenceError",true,"Tx",true,10,"10z","old1","new2"]"##;
        for (specialized, compiled) in [(false, false), (true, false), (true, true)] {
            let program = if specialized {
                Engine::specialize(source, "capture-append-effects.js")
            } else {
                Engine::compile(crate::ExecutionRequest::script(
                    source,
                    "capture-append-effects.js",
                ))
            }
            .unwrap();
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(TestHost::writing_to(output.clone()).with_test262());
            if compiled {
                vm.set_execution_mode(crate::ExecutionMode::Compiled);
            }
            vm.execute(&program).unwrap();
            assert_eq!(
                output.borrow().as_slice(),
                [expected],
                "specialized={specialized} compiled={compiled}"
            );
        }
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
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(TestHost::writing_to(output.clone()));
            if compiled {
                vm.set_execution_mode(crate::ExecutionMode::Compiled);
            }
            vm.execute(&program).unwrap();
            assert_eq!(output.borrow().as_slice(), ["xy".repeat(128)]);
            let target = vm
                .lane_stack
                .script_targets
                .entries
                .iter()
                .find(|target| target.entry == CallEntry::AppendsCapture)
                .copied()
                .expect("ordinary calls must resolve to the append operation");
            assert!(
                capture_stores(target.capture).is_some(),
                "the primitive calls must admit direct writes"
            );
            assert!(
                vm.lane_stack
                    .read_memos
                    .iter()
                    .all(|memo| memo.callee != target.callee),
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
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(TestHost::writing_to(output.clone()));
            if compiled {
                vm.set_execution_mode(crate::ExecutionMode::Compiled);
            }
            vm.execute(&program).unwrap();
            assert_eq!(
                output.borrow().as_slice(),
                [r#"[true,true,"AB","object",true,"ABC","ABCDE"]"#]
            );
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
