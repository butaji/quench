//! JavaScript's calls of Wasm exports from the lane (`quench_runtime`'s `wasm_exports`): results,
//! coercions, traps, exceptions in both directions, reentrancy and memory growth match Node's.
use quench_runtime::{ExecutionMode, ExecutionRequest, Host, Runtime, WasmModule};
use std::cell::RefCell;
use std::rc::Rc;

/// A host that compiles Wasm for the `WebAssembly` namespace and collects `print`'s lines.
struct PrintingHost(Rc<RefCell<Vec<String>>>);

impl Host for PrintingHost {
    fn write_line(&mut self, line: &str) {
        self.0.borrow_mut().push(line.to_owned());
    }
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn compile_wasm(&mut self, bytes: &[u8], mode: ExecutionMode) -> Result<WasmModule, String> {
        let module = crate::Engine::new().compile(bytes).map_err(|error| error.to_string())?;
        module.lower_shared_module_for(mode).map_err(|error| error.to_string())
    }
}

/// An `i32` export module called from loops, getters, callbacks and deep call chains, repeated
/// until every call site is quickened.
const FIXTURE: &str = r#"
// Guest fixture: JS calls of Wasm exports, compared between Node and Quench.
const leb = (n) => { const o = []; do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; o.push(b); } while (n); return o; };
const str = (s) => [...leb(s.length), ...[...s].map((c) => c.charCodeAt(0))];
const vec = (items) => [...leb(items.length), ...items.flat()];
const section = (id, body) => [id, ...leb(body.length), ...body];
function build({ types, imports = [], funcs, memory = false, exports }) {
  const typeSec = section(1, vec(types.map(([p, r]) => [0x60, ...vec(p.map((x) => [x])), ...vec(r.map((x) => [x]))])));
  const impSec = imports.length ? section(2, vec(imports.map(([m, n, t]) => [...str(m), ...str(n), 0x00, ...leb(t)]))) : [];
  const funSec = section(3, vec(funcs.map((f) => leb(f.type))));
  const memSec = memory ? section(5, vec([[0x00, 1]])) : [];
  const expSec = section(7, vec(exports.map(([n, k, i]) => [...str(n), k, ...leb(i)])));
  const codeSec = section(10, vec(funcs.map((f) => { const body = [...vec((f.locals || []).map((c) => [c, 0x7f])), ...f.code, 0x0b]; return [...leb(body.length), ...body]; })));
  return new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...typeSec, ...impSec, ...funSec, ...memSec, ...expSec, ...codeSec]);
}
const I = 0x7f;
// types: 0 (i32,i32)->i32, 1 (i32)->i32, 2 ()->i32, 3 ()->()
const bytes = build({
  types: [[[I, I], [I]], [[I], [I]], [[], [I]], [[], []]],
  imports: [["env", "cb", 1]],
  funcs: [
    { type: 0, code: [0x20, 0, 0x20, 1, 0x6a] },                 // 1: add
    { type: 0, code: [0x20, 0, 0x20, 1, 0x6d] },                 // 2: div_s (traps)
    { type: 2, code: [0x00] },                                   // 3: unreachable
    { type: 1, code: [0x20, 0, 0x41, 1, 0x6a, 0x10, 4] },        // 4: recurse forever
    { type: 1, code: [0x20, 0, 0x10, 0] },                       // 5: callback into the import
    { type: 1, locals: [1], code: [0x02, 0x40, 0x03, 0x40, 0x20, 0, 0x45, 0x0d, 1, 0x20, 1, 0x20, 0, 0x6a, 0x21, 1, 0x20, 0, 0x41, 1, 0x6b, 0x21, 0, 0x0c, 0, 0x0b, 0x0b, 0x20, 1] }, // 6: sum to n
    { type: 0, code: [0x20, 0, 0x20, 1, 0x36, 2, 0, 0x20, 0, 0x28, 2, 0] }, // 7: store then load
    { type: 2, code: [0x41, 1, 0x40, 0] },                       // 8: memory.grow(1)
    { type: 3, code: [0x01] },                                   // 9: void
    { type: 2, code: [0x41, 0x7f] },                             // 10: -1
  ],
  memory: true,
  exports: [["add", 0, 1], ["div", 0, 2], ["trap", 0, 3], ["recurse", 0, 4], ["callback", 0, 5], ["sum", 0, 6], ["store", 0, 7], ["grow", 0, 8], ["nop", 0, 9], ["minus", 0, 10], ["mem", 2, 0]],
});
const out = [];
const log = (...a) => out.push(a.map(String).join(" "));
let hook = (n) => n + 1;
const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), { env: { cb: (n) => hook(n) } });
const x = instance.exports;
function guard(f) { try { return f(); } catch (e) { return e === undefined ? "UNDEF" : e.constructor.name + (e instanceof TypeError ? ":" + e.message : ""); } }

function arith(n) { let s = 0; for (let i = 0; i < n; i++) s = (s + x.add(i, 3)) | 0; return s; }
for (let k = 0; k < 3; k++) log("arith", arith(200));
function coerce() {
  return [x.add(), x.add(1), x.add(1, 2, 3), x.add(3.7, 2.2), x.add("5", "6"), x.add(-1, -2), x.add(2147483647, 1), x.add(4294967297, 1),
    x.add({ valueOf() { return 7; } }, 1), x.add(null, true), x.add(NaN, Infinity), x.add(undefined, 9), x.minus(), x.nop()];
}
for (let k = 0; k < 3; k++) log("coerce", coerce());
function nops(n) { let c = 0; for (let i = 0; i < n; i++) { if (x.nop() === undefined) c++; } return c; }
for (let k = 0; k < 3; k++) log("nops", nops(100), nops(100));
function mixed(n) { let s = 0; for (let i = 0; i < n; i++) { s += x.add(i, 1); x.nop(); s += x.minus(); } return s; }
for (let k = 0; k < 3; k++) log("mixed", mixed(100), mixed(100));
function traps() {
  return [guard(() => x.div(1, 0)), guard(() => x.div(-2147483648, -1)), guard(() => x.trap()), guard(() => x.div(9, 3))];
}
for (let k = 0; k < 3; k++) log("traps", traps());
function catchInLoop() { let c = 0; for (let i = 0; i < 50; i++) { try { c += x.div(i, i % 5); } catch (e) { c += e instanceof WebAssembly.RuntimeError ? 1000 : -1; } } return c; }
for (let k = 0; k < 3; k++) log("loop-trap", catchInLoop());
for (let k = 0; k < 3; k++) log("sum", x.sum(100000), x.sum(0), x.sum(-1 >>> 20));
for (let k = 0; k < 3; k++) log("mem", x.store(8, 77 + k), new Int32Array(x.mem.buffer)[2], x.grow(), x.mem.buffer.byteLength, x.store(65536 + 8, 5));
log("oob", guard(() => x.store(1 << 30, 1)));
// Reentrancy: wasm calls JS, which calls wasm again; exceptions cross both ways.
hook = (n) => x.add(n, 100);
for (let k = 0; k < 3; k++) log("reenter", x.callback(5));
hook = (n) => { throw new TypeError("from js " + n); };
log("throw", guard(() => x.callback(1)), (() => { try { x.callback(2); } catch (e) { return [e.constructor.name, e.message]; } })());
hook = (n) => x.div(n, 0);
log("trap-through-js", guard(() => x.callback(3)));
let depth = 0;
hook = (n) => { depth++; return x.callback(n + 1); };
log("deep", guard(() => x.callback(0)), depth > 100);
hook = (n) => n + 1;
// `arguments`, getters and getters that call exports.
function viaArguments() { return x.add(arguments[0], arguments[1]) + arguments.length; }
const holder = { get v() { return x.add(20, 22); }, set v(n) { this.s = x.add(n, n); } };
function getterLoop() { let t = 0; for (let i = 0; i < 30; i++) { t += holder.v; holder.v = i; t += holder.s; } return t; }
for (let k = 0; k < 3; k++) log("misc", viaArguments(4, 5, 6), getterLoop(), [1, 2, 3].map((v) => x.add(v, v)), [3, 4].reduce((a, v) => x.add(a, v), 0), x.add.call(null, 8, 9), x.add.apply(null, [1, 1]), Reflect.apply(x.add, null, [2, 5]));
// A trap inside a deeper JS call chain keeps the frames on the stack.
function level(n) { return n == 0 ? x.trap() : level(n - 1) + 1; }
const caught = (() => { try { level(5); } catch (e) { return [e.constructor.name, typeof e.stack, e.stack.split("\n").length > 2]; } })();
log("chain", caught);
// A function that is both a caller of exports and called through them.
function viaHook(n) { return x.callback(n); }
hook = viaHook;
log("self", guard(() => x.callback(0)) );
hook = (n) => n + 1;
log("tail", (function () { return x.add(1, 2); })(), (() => x.add(5, 5))());
log("done");
print(out.join("\n"));
"#;

/// Node 24's output of `FIXTURE`.
const EXPECTED: &str = r#"arith 20500
arith 20500
arith 20500
coerce 0,1,3,5,11,-3,-2147483648,2,8,1,0,9,-1,
coerce 0,1,3,5,11,-3,-2147483648,2,8,1,0,9,-1,
coerce 0,1,3,5,11,-3,-2147483648,2,8,1,0,9,-1,
nops 100 100
nops 100 100
nops 100 100
mixed 4950 4950
mixed 4950 4950
mixed 4950 4950
traps RuntimeError,RuntimeError,RuntimeError,3
traps RuntimeError,RuntimeError,RuntimeError,3
traps RuntimeError,RuntimeError,RuntimeError,3
loop-trap 10500
loop-trap 10500
loop-trap 10500
sum 705082704 0 8386560
sum 705082704 0 8386560
sum 705082704 0 8386560
mem 77 77 1 131072 5
mem 78 78 2 196608 5
mem 79 79 3 262144 5
oob RuntimeError
reenter 105
reenter 105
reenter 105
throw TypeError:from js 1 TypeError,from js 2
trap-through-js RuntimeError
deep RangeError true
misc 12 2130 2,4,6 7 17 2 7
misc 12 2130 2,4,6 7 17 2 7
misc 12 2130 2,4,6 7 17 2 7
chain RuntimeError,string,true
self RangeError
tail 3 10
done"#;

#[test]
fn exports_called_from_javascript_match_node_in_both_modes() {
    for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
        std::thread::Builder::new()
            .stack_size(quench_runtime::WORKER_STACK_SIZE)
            .spawn(move || {
                let lines = Rc::new(RefCell::new(Vec::new()));
                let mut runtime = Runtime::new(PrintingHost(lines.clone()));
                runtime.set_execution_mode(mode);
                runtime.compile_and_execute(ExecutionRequest::script(FIXTURE, "wasm-export-lane.js")).unwrap();
                assert_eq!(lines.borrow().join("\n"), EXPECTED, "{mode:?}");
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
