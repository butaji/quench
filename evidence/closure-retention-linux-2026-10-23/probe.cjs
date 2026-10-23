// Closure retention probe: run with `--expose-gc` under Node and Quench.
// `mark(label, object, expected)` registers a WeakRef only; expected=true means the
// object must stay alive while the case's closure is reachable.
const cases = {};
const big = (tag) => ({ tag });
const refs = [];
let current = '';
function mark(label, object, expected) {
  refs.push({ name: current, label, expected, weak: new WeakRef(object) });
  return object;
}

cases.user_example = () => {
  function heavyTask() {
    const massiveData = mark('massiveData', new Array(16).fill('data'), true);
    return function () { return massiveData.length; };
  }
  return heavyTask();
};
cases.sibling_capture = () => {
  function f() { const b = mark('big', big('s'), false); const a = () => b.tag; a(); return () => 1; }
  return f();
};
cases.uncaptured_local = () => {
  function f() { const b = mark('big', big('u'), false); return () => 1; }
  return f();
};
cases.uncaptured_parameter = () => {
  function f(x) { return () => 1; }
  return f(mark('big', big('p'), false));
};
cases.arguments_sibling = () => {
  function f(x) { const g = () => arguments[0]; g(); return () => 1; }
  return f(mark('big', big('a'), false));
};
cases.arguments_used = () => {
  function f(x) { return () => arguments.length; }
  return f(mark('big', big('au'), true));
};
cases.this_unused = () => {
  function f() { return () => 1; }
  return f.call(mark('this', big('t'), false));
};
cases.this_sibling_arrow = () => {
  function f() { const g = () => this; g(); return function () { return 1; }; }
  return f.call(mark('this', big('ts'), false));
};
cases.this_used = () => {
  function f() { return () => this.tag; }
  return f.call(mark('this', big('tu'), true));
};
cases.block_sibling = () => {
  function f() { const k = 1; { const b = mark('big', big('b'), false); const g = () => b; g(); } return () => k; }
  return f();
};
cases.loop_sibling = () => {
  function f() {
    let n = 0;
    for (let i = 0; i < 3; i++) { const b = mark('big' + i, big('l' + i), false); const g = () => b; g(); n++; }
    return () => n;
  }
  return f();
};
cases.loop_kept = () => {
  function f() {
    let keep;
    for (let i = 0; i < 3; i++) { const b = mark('big' + i, big('k' + i), i === 1); const c = () => b; if (i === 1) keep = c; }
    return keep;
  }
  return f();
};
cases.catch_sibling = () => {
  function f() { try { throw mark('big', big('c'), false); } catch (e) { const g = () => e; g(); } return () => 1; }
  return f();
};
cases.generator_closure = () => {
  function* gen() { const b = mark('big', big('g'), false); yield () => 1; }
  return gen().next().value;
};
cases.generator_done_closure = () => {
  function* gen() { const b = mark('big', big('gd'), false); yield () => 1; }
  const it = gen();
  const c = it.next().value; it.next();
  return c;
};
cases.async_closure = () => {
  let out;
  async function f() { const b = mark('big', big('as'), false); out = () => 1; }
  f();
  return out;
};
cases.class_constructor = () => {
  class A { constructor() { const b = mark('big', big('cc'), false); this.f = () => 1; } }
  return new A().f;
};
cases.class_method = () => {
  class A { m() { const b = mark('big', big('cm'), false); return () => 1; } }
  return new A().m();
};
cases.eval_retains = () => {
  function f() { const b = mark('big', big('e'), true); return () => eval('b'); }
  return f();
};
cases.with_retains = () => {
  const fn = new Function('mark', `
    const b = mark('big', {tag:'w'}, true); const o = {};
    return function () { with (o) { return b; } };`);
  return fn(mark);
};

const kept = [];
for (const [name, make] of Object.entries(cases)) {
  current = name;
  kept.push(make());
}

function report(phase, expect) {
  gc(); gc();
  const rows = [];
  for (const ref of refs) {
    const alive = ref.weak.deref() !== undefined;
    const want = expect(ref);
    rows.push(`${phase} ${ref.name}.${ref.label} alive=${alive} want=${want}${alive === want ? '' : ' MISMATCH'}`);
  }
  console.log(rows.join('\n'));
}

setTimeout(() => {
  report('held', (ref) => ref.expected);
  // Sanity: the kept closures still work.
  if (kept[0]() !== 16) throw new Error('user example closure broken');
  kept.length = 0;
  setTimeout(() => report('dropped', () => false), 0);
}, 0);
