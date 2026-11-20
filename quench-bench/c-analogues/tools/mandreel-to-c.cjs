#!/usr/bin/env node
'use strict';
// Back-translates JetStream's Octane/mandreel.js (Mandreel's LLVM-to-JS output of Bullet) into C.
//
// usage: node tools/mandreel-to-c.cjs <JetStream>/Octane/mandreel.js support/mandreel-gen.c
//
// Every JS value is a C double, as in JS. Heap accesses are macros over one byte array, with the
// typed-array conversions (ToInt8, ToInt32, Math.fround, ...) done by the store and load macros
// of support/mandreel-rt.h. `|0`, `>>`, `>>>`, `<<`, `&`, `|`, `^`, `~` call the JS_* helpers,
// which implement ToInt32/ToUint32 exactly. Each JS function becomes `double F_<name>(double...)`
// with its declared arity. Calls keep JS evaluation order: any call, assignment or update used as
// a value is hoisted into a temporary before the statement, and an operand is spilled to a
// temporary when a later operand has hoisted statements. Labelled break/continue become goto.
// Loops are emitted as `for (;;)` with the test inside, so the test may have hoisted statements.
//
// A function that uses a JS feature outside the numeric subset (objects, strings, closures,
// typeof, try, ...) is emitted as a stub that aborts when called. The benchmark never calls one:
// tools/mandreel-validate.cjs lists the functions the run calls, and the C build fails loudly if
// any of them is a stub. Glue that is needed (strings, timers, the loader) is written by hand in
// quench-bench/c-analogues/mandreel.c and named in OVERRIDES below.
//
// The parser is acorn: ACORN_PATH, else the sandbox tools, else the layout-corpus install.

const fs = require('fs');
const path = require('path');

function loadAcorn() {
  const candidates = [
    process.env.ACORN_PATH,
    '/opt/node-tools/node_modules/acorn',
    path.join(__dirname, '..', '..', 'tools', 'layout-corpus', 'node_modules', 'acorn'),
    'acorn',
  ].filter(Boolean);
  for (const candidate of candidates) {
    try { return require(candidate); } catch (error) { /* try the next */ }
  }
  throw new Error('acorn not found: set ACORN_PATH or run npm install in tools/layout-corpus');
}

const acorn = loadAcorn();

// Functions that the hand-written C in mandreel.c provides; the generator emits only prototypes.
// Value: the arity the C definition takes (the JS parameter count).
const OVERRIDES = new Set([
  'setupMandreel',              // startApp's loader chain, replaced by the equivalent C setup
  'Mandreel_flushTimeouts',     // the timeout queue is never filled in this run
  'updateMandreelStats',        // pause statistics: the array push is not modelled
  'Mandreel_checkState',        // end-of-iteration checksum, with the alert path in C
  '__mandreel_process_async_calls', // async call queues are never filled in this run
  'puts', 'mandreel_fopen',     // string arguments (get_string_from_ptr) and the empty cache table
]);

// Globals left out of the state hashes. mandreelSampleTimeStart is a wall-clock value. The other two
// hold a function or an object in node, which the C represents by a non-zero marker (the truthiness
// the C code tests); the C writes the marker in setupMandreel (mandreel.c).
const UNHASHED_GLOBALS = new Set(['mandreelSampleTimeStart', 'mandreelAppStartStateFunc', 'mandreelFatPreloadRequest']);

// Translated functions with external linkage: the ones mandreel.c calls. Every other one is static.
const SMALL_FUNCTION_LINES = 24;
const EXPORTED_FUNCS = new Set(['runMandreel', 'my_mandreel_call_constructors', 'mandreel_call_constructors', 'startMandreelTimer', 'mandreelAppStartRenderWebGL']);

// Typed-array globals, which the C heap replaces: assignments of a view or buffer to them do nothing.
const HEAP_VIEWS = new Set(['heap', 'heap8', 'heapU8', 'heap16', 'heapU16', 'heap32', 'heapU32', 'heapFloat', 'heapDouble', 'my_heap', 'my_heap8', 'my_heap32']);

// C keywords and identifiers the generator never emits as plain names.
const C_RESERVED = new Set(['auto', 'break', 'case', 'char', 'const', 'continue', 'default', 'do', 'double', 'else', 'enum', 'extern', 'float', 'for', 'goto', 'if', 'inline', 'int', 'long', 'register', 'restrict', 'return', 'short', 'signed', 'sizeof', 'static', 'struct', 'switch', 'typedef', 'union', 'unsigned', 'void', 'volatile', 'while', 'NAN', 'INFINITY']);

// Typed-array globals of the JS heap and the macro suffix that reads and writes them.
const HEAP_ARRAYS = {
  heap8: 'I8', heapU8: 'U8', heap16: 'I16', heapU16: 'U16', heap32: 'I32', heapU32: 'U32',
  heapFloat: 'F32', heapDouble: 'F64',
  my_heap8: 'MI8', my_heap32: 'MI32',
};

// Math functions the benchmark uses, with their C counterparts (see mandreel-rt.h for the rest).
const MATH1 = { floor: 'floor', ceil: 'ceil', abs: 'fabs', sqrt: 'sqrt', cos: 'cos', sin: 'sin', tan: 'tan', acos: 'acos', asin: 'asin', atan: 'atan', exp: 'exp', log: 'log', trunc: 'trunc', round: 'JS_ROUND', fround: 'JS_FROUND', sign: 'JS_SIGN' };
const MATH2 = { pow: 'JS_POW', atan2: 'atan2', imul: 'JS_IMUL', min: 'JS_MIN', max: 'JS_MAX' };
const MATH_CONST = { PI: 'M_PI', E: 'M_E', LN2: 'M_LN2', LN10: 'M_LN10', SQRT2: 'M_SQRT2', SQRT1_2: 'M_SQRT1_2' };

class Unsupported extends Error {}

function cName(name) {
  return 'l_' + name.replace(/\$/g, '_S_');
}
function fnName(name) {
  return 'F_' + name.replace(/\$/g, '_S_');
}
function globalName(name) {
  return 'V_' + name.replace(/\$/g, '_S_');
}
function numLit(v) {
  if (Number.isNaN(v)) return 'NAN';
  if (v === Infinity) return 'INFINITY';
  if (v === -Infinity) return '(-INFINITY)';
  if (Object.is(v, -0)) return '(-0.0)';
  let s = String(v);
  if (!/[.eEn]/.test(s)) s += '.0';
  return v < 0 ? `(${s})` : s;
}
function isSimple(s) {
  return /^(-?[0-9][0-9.e+]*|\(-?[0-9][0-9.e+]*\)|t_\d+|NAN|INFINITY|\(-INFINITY\))$/.test(s);
}
function strVar(name) {
  return 'S_' + name.replace(/\$/g, '_S_');
}
function cString(s) {
  return '"' + s.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n').replace(/\?/g, '\\?') + '"';
}

// Collects the names declared with `var` in a function body, not descending into nested functions.
function collectVars(node, out) {
  if (!node || typeof node.type !== 'string') return;
  if (node.type === 'FunctionDeclaration' || node.type === 'FunctionExpression' || node.type === 'ArrowFunctionExpression') return;
  if (node.type === 'VariableDeclaration') for (const d of node.declarations) out.add(d.id.name);
  for (const key of Object.keys(node)) {
    if (key === 'type' || key === 'start' || key === 'end') continue;
    const v = node[key];
    if (Array.isArray(v)) v.forEach((x) => collectVars(x, out));
    else if (v && typeof v.type === 'string') collectVars(v, out);
  }
}

// `x == undefined` with x one of the typed-array views, which are never undefined.
function isUndefinedTest(n, names) {
  const isView = (e) => e.type === 'Identifier' && names.has(e.name);
  const isUndef = (e) => e.type === 'Identifier' && e.name === 'undefined';
  return (isView(n.left) && isUndef(n.right)) || (isUndef(n.left) && isView(n.right));
}

function hasEffects(node) {
  let found = false;
  walk(node, (n) => { if (n.type === 'CallExpression' || n.type === 'AssignmentExpression' || n.type === 'UpdateExpression') found = true; });
  return found;
}

function walk(node, visit) {
  if (!node || typeof node.type !== 'string') return;
  visit(node);
  for (const key of Object.keys(node)) {
    if (key === 'type' || key === 'start' || key === 'end') continue;
    const v = node[key];
    if (Array.isArray(v)) v.forEach((x) => walk(x, visit));
    else if (v && typeof v.type === 'string') walk(v, visit);
  }
}

class Program {
  constructor(src, ast) {
    this.src = src;
    this.ast = ast;
    this.topFuncs = new Map();   // name -> FunctionDeclaration
    this.topVars = new Set();    // names declared by top-level var
    this.fnvars = new Set();     // names holding JS function values (C: void *)
    this.globals = new Set();    // every global variable (declared or implicit)
    this.labelCounter = 0;
    this.strings = [];           // string literal tables emitted for emit_string
    this.stringVars = new Set();   // string globals (C const char *)
    this.problems = new Map();   // function name -> reason it is a stub
    this.stmtStubs = [];         // 'function: reason' for statements replaced by aborts
    this.emitTables = [];        // data-emit runs (see FnGen.emitRun), as arrays of {data, count, width}
    this.emitRuns = new Map();   // first statement of a run -> its js_emit_ops call
    this.callJoin = new Map();   // function -> per parameter: join of the argument types of its direct calls
    this.paramTypes = new Map(); // function -> per parameter: 'i', 'l' or 'd' (see generate())
    this.excluded = new Set();   // functions whose signature stays double (called from C or through pointers)
  }

  analyze() {
    for (const s of this.ast.body) {
      if (s.type === 'FunctionDeclaration') this.topFuncs.set(s.id.name, s);
      if (s.type === 'VariableDeclaration') for (const d of s.declarations) this.topVars.add(d.id.name);
    }
    for (const name of this.topVars) if (this.topFuncs.has(name)) throw new Error('name is both var and function: ' + name);
    // String globals: a top-level var whose every assignment is a string literal (or the top-level
    // initialiser). They are C `const char *` variables, read by fill_ptr_from_string(ptr, NAME).
    const literalOrUnknown = new Map(); // name -> true for string-initialised vars
    for (const s of this.ast.body) {
      if (s.type !== 'VariableDeclaration') continue;
      for (const d of s.declarations) {
        if (d.init && d.init.type === 'Literal' && typeof d.init.value === 'string') literalOrUnknown.set(d.id.name, true);
      }
    }
    // Other assignments to them are statements that abort if reached (see assign()).
    for (const [name, ok] of literalOrUnknown) if (ok && !this.topFuncs.has(name)) this.stringVars.add(name);
    // Implicit globals: assignment targets that neither the top level nor the enclosing function declares.
    const declared = new Set([...this.topFuncs.keys(), ...this.topVars]);
    const assigned = new Set();
    for (const s of this.ast.body) {
      if (s.type !== 'FunctionDeclaration' && s.type !== 'ExpressionStatement' && s.type !== 'VariableDeclaration') continue;
      const locals = new Set(s.type === 'FunctionDeclaration' ? s.params.map((p) => p.name) : []);
      if (s.type === 'FunctionDeclaration') collectVars(s.body, locals);
      walk(s, (n) => {
        if (n.type === 'AssignmentExpression' && n.left.type === 'Identifier' && !locals.has(n.left.name)) assigned.add(n.left.name);
      });
    }
    for (const name of this.topVars) this.globals.add(name);
    for (const name of assigned) if (!declared.has(name)) this.globals.add(name);
    // (string globals stay in this.globals; they are emitted as const char * below)
    const assignedAll = new Set();
    walk(this.ast, (n) => { if (n.type === 'AssignmentExpression' && n.left.type === 'Identifier') assignedAll.add(n.left.name); });
    // Function values: a global assigned a top-level function (or another function value) is a void *.
    const fnNames = new Set(this.topFuncs.keys());
    for (const name of assignedAll) if (fnNames.has(name)) this.fnvars.add(name);
    let changed = true;
    while (changed) {
      changed = false;
      walk(this.ast, (n) => {
        if (n.type !== 'AssignmentExpression' || n.operator !== '=' || n.left.type !== 'Identifier' || n.right.type !== 'Identifier') return;
        const left = n.left.name, right = n.right.name;
        if (this.fnvars.has(right) || fnNames.has(right)) {
          if (!this.fnvars.has(left) && !this.isLocalAt(n, left)) { this.fnvars.add(left); changed = true; }
        }
      });
    }
  }

  // Records the type of argument k of a direct call of `name` (the join over all call sites).
  joinCall(name, k, t) {
    let a = this.callJoin.get(name);
    if (!a) { a = []; this.callJoin.set(name, a); }
    a[k] = join(a[k], t);
  }

  // Function values held in globals, including reassigned function declarations, are globals too.
  finishGlobals() {
    for (const name of this.fnvars) this.globals.add(name);
  }

  isLocalSomewhere(name) {
    for (const s of this.ast.body) {
      if (s.type !== 'FunctionDeclaration') continue;
      if (s.params.some((p) => p.name === name)) return true;
      const vars = new Set();
      collectVars(s.body, vars);
      if (vars.has(name)) return true;
    }
    return false;
  }

  // Whether `name` is a local of the top-level function containing `node` (approximate: any function that contains it).
  isLocalAt(node, name) {
    for (const s of this.ast.body) {
      if (s.type !== 'FunctionDeclaration') continue;
      if (node.start < s.start || node.end > s.end) continue;
      if (s.params.some((p) => p.name === name)) return true;
      const vars = new Set();
      collectVars(s.body, vars);
      return vars.has(name);
    }
    return false;
  }
}

// Translates one function body (or the top-level script) into C statements.

// ---- typed values ----
// An expression is {c, t, dep, val}: C text c of JS-value type t: 'i' int32_t (exact int32),
// 'l' int64_t (an exact integer below 2^53; dep counts the sums it came from), 'd' double.
// val is set for numeric literals. Locals take the join of their assigned types, so a loop
// counter or an address stays in integer registers; everything else is double, as in JS.
const RANK = { i: 0, l: 1, d: 2 };
function join(a, b) {
  if (a === undefined) return b;
  if (b === undefined) return a;
  return RANK[a] >= RANK[b] ? a : b;
}
// double, int32 and int64 views of a typed value (exact for i and l).
function D(e) { return e.t === 'd' ? e.c : `((double)${e.c})`; }
function I(e) { return e.t === 'i' ? e.c : e.t === 'l' ? `((int32_t)${e.c})` : `js_toint32(${e.c})`; }
function L(e) { return `((int64_t)${e.c})`; }
function litObj(v) {
  if (Object.is(v, -0)) return { c: '(-0.0)', t: 'd', dep: 0 };
  if (Number.isNaN(v)) return { c: 'NAN', t: 'd', dep: 0 };
  if (v === Infinity) return { c: 'INFINITY', t: 'd', dep: 0 };
  if (v === -Infinity) return { c: '(-INFINITY)', t: 'd', dep: 0 };
  if (Number.isInteger(v) && Math.abs(v) <= 2147483647) return { c: v < 0 ? `(${v})` : `${v}`, t: 'i', dep: 0, val: v };
  if (Number.isInteger(v) && Math.abs(v) <= 9007199254740992) return { c: `(${v}LL)`, t: 'l', dep: 0, val: v };
  return { c: numLit(v), t: 'd', dep: 0, val: v };
}
// Heap element types: load type, and the store conversion.
const HEAP_LOAD = { I8: 'i', U8: 'i', I16: 'i', U16: 'i', I32: 'i', U32: 'l', F32: 'd', F64: 'd', MI8: 'i', MI32: 'i' };
function heapStoreConv(arr, e) { return arr === 'F32' || arr === 'F64' ? D(e) : I(e); }
function loadExpr(arr, index) {
  if (arr === 'U32') return `LL_U32(${index})`;
  if (arr === 'F32' || arr === 'F64') return `LD_${arr}(${index})`;
  return `LI_${arr}(${index})`;
}
const SHIFT_NAMES = { '<<': 1, '>>': 1, '>>>': 1 };

// Translates one function body (or the top-level script) into C statements.
class FnGen {
  constructor(program, fnNode, params, types) {
    this.p = program;
    this.fn = fnNode;
    this.params = new Set(params);
    this.locals = new Set(params);
    this.types = types || new Map(); // local -> type, from the previous pass
    this.paramTy = new Map();        // parameter -> its C type ('i', 'l' or 'd'; see generate())
    this.contrib = new Map();        // local -> join of the types assigned in this pass
    this.tempTypes = [];
    this.pre = [];
    this.loops = [];
    this.labelMap = new Map();
    this.stmtStubs = [];
    if (fnNode) {
      const vars = new Set(); collectVars(fnNode.body, vars); for (const v of vars) this.locals.add(v);
      const ptys = this.p.paramTypes.get(fnNode.id.name);
      fnNode.params.forEach((prm, k) => this.paramTy.set(prm.name, ptys[k]));
    }
  }

  temp(type = 'd') {
    this.tempTypes.push(type);
    return `t_${this.tempTypes.length - 1}`;
  }

  localType(name) {
    return this.types.get(name) || 'i'; // not assigned yet in any pass: optimistic, fixed by the fixpoint
  }

  note(name, type) {
    this.contrib.set(name, join(this.contrib.get(name), type));
  }

  src(n) { return this.p.src.slice(n.start, n.end); }

  // ---- expressions ----

  identifier(name) {
    if (name === 'undefined' || name === 'NaN') return { c: 'NAN', t: 'd', dep: 0 };
    if (name === 'Infinity') return { c: 'INFINITY', t: 'd', dep: 0 };
    if (this.locals.has(name)) return { c: cName(name), t: this.params.has(name) ? this.paramTy.get(name) : this.localType(name), dep: 0 };
    if (this.p.fnvars.has(name)) throw new Unsupported(`function value ${name} read as a value`);
    if (this.p.stringVars.has(name)) throw new Unsupported(`string ${name} read as a number`);
    if (this.p.globals.has(name)) return { c: globalName(name), t: 'd', dep: 0 };
    if (this.p.topFuncs.has(name)) throw new Unsupported(`function ${name} read as a value`);
    throw new Unsupported(`free identifier ${name}`);
  }

  ex(n) {
    switch (n.type) {
      case 'Literal':
        if (typeof n.value === 'number') return litObj(n.value);
        if (typeof n.value === 'boolean') return { c: n.value ? '1' : '0', t: 'i', dep: 0, val: n.value ? 1 : 0 };
        throw new Unsupported(`literal ${typeof n.value}`);
      case 'Identifier':
        return this.identifier(n.name);
      case 'MemberExpression': {
        if (!n.computed && n.object.type === 'Identifier' && n.object.name === 'Math' && MATH_CONST[n.property.name]) return { c: MATH_CONST[n.property.name], t: 'd', dep: 0 };
        if (n.computed && n.object.type === 'Identifier' && HEAP_ARRAYS[n.object.name]) {
          const arr = HEAP_ARRAYS[n.object.name];
          const idx = this.ex(n.property);
          return { c: loadExpr(arr, idx.c), t: HEAP_LOAD[arr], dep: 0 };
        }
        throw new Unsupported(`member ${this.src(n).slice(0, 40)}`);
      }
      case 'BinaryExpression': {
        const [a, b] = this.exList([n.left, n.right]);
        if (n.operator === '==' || n.operator === '!=') this.checkNullish(n);
        return this.binop(n.operator, a, b, n);
      }
      case 'LogicalExpression':
        return this.logicalValue(n);
      case 'UnaryExpression':
        return this.unary(n);
      case 'UpdateExpression':
      case 'AssignmentExpression':
        return this.effectValue(n);
      case 'ConditionalExpression':
        return this.conditionalValue(n);
      case 'CallExpression':
        return this.hoist(this.call(n));
      default:
        throw new Unsupported(`expression ${n.type}`);
    }
  }

  hoist(obj) {
    const t = this.temp(obj.t);
    this.pre.push(`${t} = ${obj.c};`);
    return { c: t, t: obj.t, dep: 0 };
  }

  // Evaluates `nodes` left to right; earlier operands are spilled to temporaries when a later one hoists.
  exList(nodes) {
    const out = [];
    for (let i = 0; i < nodes.length; i++) {
      const mark = this.pre.length;
      const s = this.ex(nodes[i]);
      if (this.pre.length > mark) {
        let pos = mark;
        for (let j = 0; j < out.length; j++) {
          if (!isSimpleObj(out[j])) {
            const t = this.temp(out[j].t);
            this.pre.splice(pos++, 0, `${t} = ${out[j].c};`);
            out[j] = { c: t, t: out[j].t, dep: 0 };
          }
        }
      }
      out.push(s);
    }
    return out;
  }

  // The result of a binary operator on two typed values.
  binop(op, a, b, n) {
    const ints = a.t !== 'd' && b.t !== 'd';
    switch (op) {
      case '+': case '-':
        if (ints && a.dep < 8 && b.dep < 8) return { c: `(${L(a)} ${op} ${L(b)})`, t: 'l', dep: Math.max(a.dep, b.dep) + 1 };
        return { c: `(${D(a)} ${op} ${D(b)})`, t: 'd', dep: 0 };
      case '*': case '/':
        return { c: `(${D(a)} ${op} ${D(b)})`, t: 'd', dep: 0 };
      case '%':
        return { c: `fmod(${D(a)}, ${D(b)})`, t: 'd', dep: 0 };
      case '|':
        if (b.val === 0) return { c: I(a), t: 'i', dep: 0 };
        return { c: `(${I(a)} | ${I(b)})`, t: 'i', dep: 0 };
      case '&': return { c: `(${I(a)} & ${I(b)})`, t: 'i', dep: 0 };
      case '^': return { c: `(${I(a)} ^ ${I(b)})`, t: 'i', dep: 0 };
      case '<<': return { c: `((int32_t)((uint32_t)${I(a)} << ${shiftCount(b)}))`, t: 'i', dep: 0 };
      case '>>': return { c: `(${I(a)} >> ${shiftCount(b)})`, t: 'i', dep: 0 };
      case '>>>': return { c: `((int64_t)((uint32_t)${I(a)} >> ${shiftCount(b)}))`, t: 'l', dep: 0 };
      case '<': case '>': case '<=': case '>=': case '==': case '!=': case '===': case '!==':
        return { c: `(${this.compare(op, a, b)} ? 1 : 0)`, t: 'i', dep: 0 };
      default:
        throw new Unsupported(`binary ${op}`);
    }
  }

  compare(op, a, b) {
    const cop = { '===': '==', '!==': '!=' }[op] || op;
    if (a.c === b.c && a.t !== 'd') {
      // The same integer on both sides (x == x, x != x): a constant, as JS evaluates it.
      return ['==', '<=', '>='].includes(cop) ? '1' : '0';
    }
    if (a.t !== 'd' && b.t !== 'd') return `(${L(a)} ${cop} ${L(b)})`;
    // The same double on both sides (the NaN test x != x): written so that no compiler warns.
    if (a.c === b.c) return `(${D(a)} ${cop} (${D(b)} + 0.0))`;
    return `(${D(a)} ${cop} ${D(b)})`;
  }

  checkNullish(n) {
    for (const side of [n.left, n.right]) {
      if (side.type === 'Literal' && (side.value === null)) throw new Unsupported('comparison with null');
      if (side.type === 'Identifier' && side.name === 'undefined') throw new Unsupported('comparison with undefined');
    }
  }

  unary(n) {
    switch (n.operator) {
      case '-': {
        const a = this.ex(n.argument);
        // A nonzero integer literal negates exactly as an integer constant (-0 needs 0, which stays double).
        if (a.t !== 'd' && a.val !== undefined && a.val !== 0) return litObj(-a.val);
        return { c: `(-(${D(a)}))`, t: 'd', dep: 0 };
      }
      case '+': return this.ex(n.argument);
      case '!': return { c: `(!${this.cond(n.argument)})`, t: 'i', dep: 0 };
      case '~': { const a = this.ex(n.argument); return { c: `(~${I(a)})`, t: 'i', dep: 0 }; }
      default: throw new Unsupported(`unary ${n.operator}`);
    }
  }

  // Boolean C expression (nonzero means JS-truthy).
  cond(n) {
    if (n.type === 'Literal' && typeof n.value === 'boolean') return n.value ? '1' : '0';
    if (n.type === 'BinaryExpression' && (n.operator === '==' || n.operator === '!=') && isUndefinedTest(n, HEAP_VIEWS)) {
      return n.operator === '==' ? '0' : '1';
    }
    const sc = this.stringCompare(n);
    if (sc !== null) return sc;
    if (n.type === 'BinaryExpression' && ['<', '>', '<=', '>=', '==', '!=', '===', '!=='].includes(n.operator)) {
      if (n.operator === '==' || n.operator === '!=') this.checkNullish(n);
      const [a, b] = this.exList([n.left, n.right]);
      return this.compare(n.operator, a, b);
    }
    if (n.type === 'UnaryExpression' && n.operator === '!') return `(!${this.cond(n.argument)})`;
    if (n.type === 'LogicalExpression' && (n.operator === '&&' || n.operator === '||')) {
      const left = this.cond(n.left);
      const mark = this.pre.length;
      let right;
      try {
        right = this.cond(n.right);
      } catch (error) {
        if (!(error instanceof Unsupported)) throw error;
        // Reached only when the left operand lets the right one run: abort then, as the JS would reach it.
        this.pre.length = mark;
        const t = this.temp('d');
        this.pre.push(`${t} = (${left}) ? 1.0 : 0.0;`, `if (${n.operator === '&&' ? `${t} != 0.0` : `${t} == 0.0`}) js_unsupported_at(${cString(this.name())}, ${cString(error.message)});`);
        return `(${t} != 0.0)`;
      }
      if (this.pre.length === mark) return `(${left} ${n.operator} ${right})`;
      const rightPre = this.pre.splice(mark);
      const t = this.temp('d');
      this.pre.push(`${t} = (${left}) ? 1.0 : 0.0;`);
      this.pre.push(n.operator === '&&' ? `if (${t} != 0.0) {` : `if (${t} == 0.0) {`, ...rightPre, `${t} = (${right}) ? 1.0 : 0.0;`, '}');
      return `(${t} != 0.0)`;
    }
    const v = this.ex(n);
    return v.t === 'd' ? `JS_TRUTHY(${v.c})` : `(${v.c} != 0)`;
  }

  // `stringVar == 'literal'` and its variants, as strcmp.
  stringCompare(n) {
    if (n.type !== 'BinaryExpression' || !['==', '!=', '===', '!=='].includes(n.operator)) return null;
    const isStrVar = (e) => e.type === 'Identifier' && this.p.stringVars.has(e.name) && !this.locals.has(e.name);
    const isStrLit = (e) => e.type === 'Literal' && typeof e.value === 'string';
    let v = null, lit = null;
    if (isStrVar(n.left) && isStrLit(n.right)) { v = n.left; lit = n.right; }
    else if (isStrVar(n.right) && isStrLit(n.left)) { v = n.right; lit = n.left; }
    if (!v) return null;
    const eq = `(strcmp(${strVar(v.name)}, ${cString(lit.value)}) == 0)`;
    return n.operator[0] === '=' ? eq : `(!${eq})`;
  }

  logicalValue(n) {
    const left = this.ex(n.left);
    const lc = D(left);
    const mark = this.pre.length;
    const isAnd = n.operator === '&&';
    let right;
    try {
      right = this.ex(n.right);
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      this.pre.length = mark;
      const t = this.temp('d');
      this.pre.push(`${t} = ${lc};`, isAnd ? `if (JS_TRUTHY(${t})) js_unsupported_at(${cString(this.name())}, ${cString(error.message)});` : `if (!JS_TRUTHY(${t})) js_unsupported_at(${cString(this.name())}, ${cString(error.message)});`);
      return { c: t, t: 'd', dep: 0 };
    }
    if (this.pre.length === mark) {
      return { c: isAnd ? `(JS_TRUTHY(${lc}) ? ${D(right)} : ${lc})` : `(JS_TRUTHY(${lc}) ? ${lc} : ${D(right)})`, t: 'd', dep: 0 };
    }
    const rightPre = this.pre.splice(mark);
    const t = this.temp('d');
    this.pre.push(`${t} = ${lc};`, isAnd ? `if (JS_TRUTHY(${t})) {` : `if (!JS_TRUTHY(${t})) {`, ...rightPre, `${t} = ${D(right)};`, '}');
    return { c: t, t: 'd', dep: 0 };
  }

  conditionalValue(n) {
    const c = this.cond(n.test);
    const branch = (node) => {
      const m = this.pre.length;
      try {
        const v = this.ex(node);
        return { v: D(v), pre: this.pre.splice(m) };
      } catch (error) {
        if (!(error instanceof Unsupported)) throw error;
        this.pre.length = m;
        return { v: 'NAN', pre: [`js_unsupported_at(${cString(this.name())}, ${cString(error.message)});`] };
      }
    };
    const { v: a, pre: preA } = branch(n.consequent);
    const { v: b, pre: preB } = branch(n.alternate);
    if (preA.length === 0 && preB.length === 0) return { c: `(${c} ? ${a} : ${b})`, t: 'd', dep: 0 };
    const t = this.temp('d');
    this.pre.push(`if (${c}) {`, ...preA, `${t} = ${a};`, '} else {', ...preB, `${t} = ${b};`, '}');
    return { c: t, t: 'd', dep: 0 };
  }

  // An assignment or update used as a value: performed now, its value returned as a typed temporary.
  effectValue(n) {
    if (n.type === 'UpdateExpression') {
      const op = n.operator === '++' ? '+' : '-';
      const one = { type: 'Literal', value: 1 };
      if (n.prefix) {
        const r = this.storeTo(n.argument, op, one, true);
        this.pre.push(...r.lines);
        return r.value;
      }
      // Postfix: the old value is read first, then the store (the target must have no hoisted effects).
      if (hasEffects(n.argument)) throw new Unsupported('postfix update with effects in its target');
      const old = this.ex(n.argument);
      const t = this.temp(old.t);
      this.pre.push(`${t} = ${old.c};`);
      const r = this.storeTo(n.argument, op, one, false);
      this.pre.push(...r.lines);
      return { c: t, t: old.t, dep: 0 };
    }
    const r = this.assign(n, true);
    this.pre.push(...r.lines);
    return r.value;
  }

  call(n) {
    const args = n.arguments;
    const cal = n.callee;
    if (cal.type === 'Identifier') {
      const name = cal.name;
      if (name === 'register_delegate') {
        if (args.length !== 1 || args[0].type !== 'Identifier' || !this.p.topFuncs.has(args[0].name)) throw new Unsupported('register_delegate of a non-function');
        return { c: `js_register_delegate((void *)${fnName(args[0].name)})`, t: 'd', dep: 0 };
      }
      if (name === 'fill_ptr_from_string' && args.length === 2 && args[1].type === 'Identifier' && this.p.stringVars.has(args[1].name) && !this.locals.has(args[1].name)) {
        const [ptr] = this.exList([args[0]]);
        return { c: `js_fill_cstring(${D(ptr)}, ${strVar(args[1].name)})`, t: 'd', dep: 0 };
      }
      if (name === 'emit_string') {
        if (args.length !== 1 || args[0].type !== 'Literal' || typeof args[0].value !== 'string') throw new Unsupported('emit_string of a non-literal');
        const units = [];
        for (let i = 0; i < args[0].value.length; i++) units.push(args[0].value.charCodeAt(i));
        const id = this.p.strings.length;
        this.p.strings.push(units);
        return { c: `js_emit_bytes(k_str_${id}, ${units.length})`, t: 'd', dep: 0 };
      }
      if (this.p.fnvars.has(name)) return { c: this.indirectCall(globalName(name), args), t: 'd', dep: 0 };
      if (this.p.topFuncs.has(name)) {
        if (name === 'uint' && args.length === 1 && this.p.topFuncs.get('uint').params.length === 1) {
          // JS uint(v): v for v >= 0, else 2^32 + v. Inline, with the integer forms kept exact.
          const [a] = this.exList(args);
          if (a.t === 'd') return { c: `js_uint(${a.c})`, t: 'd', dep: 0 };
          return { c: `(${L(a)} >= 0 ? ${L(a)} : ${L(a)} + 4294967296LL)`, t: 'l', dep: 0 };
        }
        const arity = this.p.topFuncs.get(name).params.length;
        const exprs = this.padArgs(args, arity, name);
        return { c: `${fnName(name)}(${exprs.join(', ')})`, t: 'd', dep: 0 };
      }
      if (this.p.globals.has(name)) throw new Unsupported(`call of global ${name}`);
      throw new Unsupported(`call of free function ${name}`);
    }
    if (cal.type === 'MemberExpression' && !cal.computed && cal.object.type === 'Identifier' && cal.object.name === 'Math') {
      const m = cal.property.name;
      if (MATH1[m] && args.length === 1) return { c: `${MATH1[m]}(${D(this.ex(args[0]))})`, t: 'd', dep: 0 };
      if (MATH2[m] && args.length === 2) { const [a, b] = this.exList(args); return { c: `${MATH2[m]}(${D(a)}, ${D(b)})`, t: 'd', dep: 0 }; }
      throw new Unsupported(`Math.${m}`);
    }
    if (cal.type === 'MemberExpression' && !cal.computed && cal.object.type === 'Identifier' && cal.object.name === 'performance' && cal.property.name === 'now' && args.length === 0) {
      return { c: 'js_perf_now()', t: 'd', dep: 0 };
    }
    if (cal.type === 'MemberExpression' && cal.computed && cal.object.type === 'Identifier' && cal.object.name === '__FUNCTION_TABLE__') {
      const [idx, ...rest] = this.exList([cal.property, ...args]);
      const sig = this.sigOf(rest.length);
      return { c: `((${sig})js_fntab[(int64_t)(${idx.c})])(${rest.map(D).join(', ')})`, t: 'd', dep: 0 };
    }
    throw new Unsupported(`call of ${this.src(cal).slice(0, 40)}`);
  }

  sigOf(k) {
    return `double (*)(${k === 0 ? 'void' : Array(k).fill('double').join(', ')})`;
  }

  indirectCall(target, args) {
    const exprs = this.exList(args).map(D);
    return `((${this.sigOf(exprs.length)})${target})(${exprs.join(', ')})`;
  }

  // Argument expressions for a direct call of `callee` with `arity` parameters (JS pads with undefined and drops
  // extras). Each argument is converted to the C type of its parameter; the argument types are recorded
  // for the parameter typing in generate().
  padArgs(args, arity, callee) {
    const vals = this.exList(args.slice(0, arity));
    const tys = this.p.paramTypes.get(callee);
    const exprs = [];
    for (let k = 0; k < arity; k++) {
      if (k < vals.length) {
        this.p.joinCall(callee, k, vals[k].t);
        exprs.push(convTo(tys[k], vals[k]));
      } else {
        this.p.joinCall(callee, k, 'd'); // undefined: NaN
        exprs.push(tys[k] === 'd' ? 'NAN' : '0');
      }
    }
    for (const extra of args.slice(arity)) {
      this.pre.push(`(void)(${D(this.ex(extra))});`);
    }
    return exprs;
  }

  // ---- assignments ----

  // An assignment expression: {lines, value} (value is the typed result when wantValue).
  assign(n, wantValue) {
    const target = n.left;
    const op = n.operator === '=' ? null : n.operator.slice(0, -1);
    if (target.type === 'Identifier' && n.right.type === 'Identifier' && op === null && (this.p.topFuncs.has(n.right.name) || this.p.fnvars.has(n.right.name))) {
      if (!this.p.fnvars.has(target.name)) throw new Unsupported(`function value assigned to ${target.name}`);
      const rhs = this.p.topFuncs.has(n.right.name) && !this.p.fnvars.has(n.right.name) ? `(void *)${fnName(n.right.name)}` : globalName(n.right.name);
      return { lines: [`${globalName(target.name)} = ${rhs};`], value: { c: 'NAN', t: 'd', dep: 0 } };
    }
    if (target.type === 'Identifier' && this.p.stringVars.has(target.name) && !this.locals.has(target.name)) {
      if (n.right.type !== 'Literal' || typeof n.right.value !== 'string' || op !== null) throw new Unsupported('string variable assigned a non-literal');
      return { lines: [`${strVar(target.name)} = ${cString(n.right.value)};`], value: { c: 'NAN', t: 'd', dep: 0 } };
    }
    return this.storeTo(target, op, n.right, wantValue);
  }

  // Store to an identifier or heap element. `op` is the binary operator of a compound assignment or null.
  // Returns {lines, value}: lines hold the hoisted statements and the store, in order.
  storeTo(target, op, rightNode, wantValue) {
    const saved = this.pre;
    this.pre = [];
    let loc = null, idx = null, arr = null, lt, cur = null, local = null;
    try {
      if (target.type === 'Identifier') {
        if (this.p.fnvars.has(target.name)) throw new Unsupported(`store to function variable ${target.name}`);
        if (this.p.stringVars.has(target.name) && !this.locals.has(target.name)) throw new Unsupported('string variable in a numeric store');
        const obj = this.identifier(target.name);
        if (obj.c === 'NAN' || obj.c === 'INFINITY') throw new Unsupported('store to undefined');
        loc = obj.c;
        lt = obj.t;
        cur = obj;
        if (this.locals.has(target.name)) local = target.name;
      } else if (target.type === 'MemberExpression' && target.computed && target.object.type === 'Identifier' && HEAP_ARRAYS[target.object.name]) {
        arr = HEAP_ARRAYS[target.object.name];
        idx = this.ex(target.property);
        lt = HEAP_LOAD[arr];
      } else {
        throw new Unsupported(`assignment target ${target.type}`);
      }
      let v;
      if (op === null) {
        const mark = this.pre.length;
        v = this.ex(rightNode);
        if (idx && this.pre.length > mark && !isSimpleObj(idx)) {
          const t = this.temp(idx.t);
          this.pre.splice(mark, 0, `${t} = ${idx.c};`);
          idx = { c: t, t: idx.t, dep: 0 };
        }
      } else {
        if (idx && !isSimpleObj(idx)) {
          const t = this.temp(idx.t);
          this.pre.push(`${t} = ${idx.c};`);
          idx = { c: t, t: idx.t, dep: 0 };
        }
        const curObj = arr ? { c: loadExpr(arr, idx.c), t: lt, dep: 0 } : cur;
        const mark = this.pre.length;
        const r = this.ex(rightNode);
        let curUse = curObj;
        if (this.pre.length > mark) {
          const t = this.temp(curObj.t);
          this.pre.splice(mark, 0, `${t} = ${curObj.c};`);
          curUse = { c: t, t: curObj.t, dep: 0 };
        }
        v = this.binop(op, curUse, r, null);
      }
      if (local !== null) this.note(local, v.t);
      let value = null;
      let rhs;
      if (arr) {
        rhs = heapStoreConv(arr, v);
      } else {
        // A local takes the type it was given by the fixpoint; a global is a double.
        rhs = lt === 'd' ? D(v) : lt === 'i' ? I(v) : L(v);
      }
      if (wantValue) {
        const t = this.temp(arr ? (arr === 'F32' || arr === 'F64' ? 'd' : 'i') : lt);
        this.pre.push(`${t} = ${rhs};`);
        rhs = t;
        value = { c: t, t: arr ? (arr === 'F32' || arr === 'F64' ? 'd' : 'i') : lt, dep: 0 };
        if (arr === 'U32') value.t = 'i';
      }
      const storeLine = arr ? `ST_${arr}(${idx.c}, ${rhs});` : `${loc} = ${rhs};`;
      return { lines: [...this.pre, storeLine], value };
    } finally {
      this.pre = saved;
    }
  }

  // ---- statements: return C lines ----

  // Statement-position expression: assignments, updates and calls become statements directly.
  exprStatement(n) {
    if (n.type === 'AssignmentExpression' && n.left.type === 'Identifier' && HEAP_VIEWS.has(n.left.name) && n.right.type === 'NewExpression') {
      return [`/* ${n.left.name}: typed-array view of the static C heap */`];
    }
    const saved = this.pre;
    this.pre = [];
    let lines;
    try {
      if (n.type === 'AssignmentExpression') {
        const r = this.assign(n, false);
        lines = r.lines;
      } else if (n.type === 'UpdateExpression') {
        lines = this.storeTo(n.argument, n.operator === '++' ? '+' : '-', { type: 'Literal', value: 1 }, false).lines;
      } else if (n.type === 'CallExpression') {
        lines = [`${this.call(n).c};`];
      } else if (n.type === 'SequenceExpression') {
        lines = n.expressions.flatMap((e) => this.exprStatement(e));
      } else {
        lines = [`(void)(${D(this.ex(n))});`];
      }
      return [...this.pre, ...lines];
    } finally {
      this.pre = saved;
    }
  }

  block(stmts, depth) {
    const out = [];
    for (let i = 0; i < stmts.length;) {
      const run = this.emitRun(stmts, i);
      if (run) {
        out.push(run.line);
        i = run.end;
        continue;
      }
      out.push(...this.stmt(stmts[i], depth));
      i++;
    }
    return out;
  }

  // A run of data emits with literal operands (emit_8/16/32(N), emit_fill(N, K)) starting at stmts[i]:
  // their operands are known now, so the run becomes one table of {data, count, width} records that
  // js_emit_ops() writes in order, with the same stores as the calls. A run of 150k such calls in
  // one function made GCC take minutes; the table compiles in seconds. Returns null if no run starts here.
  emitRun(stmts, i) {
    const ops = [];
    let j = i;
    for (; j < stmts.length; j++) {
      const op = this.emitOp(stmts[j]);
      if (!op) break;
      ops.push(op);
    }
    if (ops.length === 0) return null;
    // The type fixpoint translates a body several times: each run gets its table once, keyed by its first statement.
    let run = this.p.emitRuns.get(stmts[i]);
    if (!run) {
      run = { line: `js_emit_ops(k_emit_${this.p.emitTables.length}, ${ops.length});`, end: j };
      this.p.emitTables.push(ops);
      this.p.emitRuns.set(stmts[i], run);
    }
    return run;
  }

  // {data, count, width} for a data emit statement with literal operands, else null.
  emitOp(s) {
    if (s.type !== 'ExpressionStatement' || s.expression.type !== 'CallExpression') return null;
    const cal = s.expression.callee, args = s.expression.arguments;
    if (cal.type !== 'Identifier' || this.locals.has(cal.name) || this.p.topFuncs.get(cal.name) === undefined) return null;
    const lit = (e) => e.type === 'Literal' && typeof e.value === 'number' && Number.isFinite(e.value);
    const widths = { emit_8: 1, emit_16: 2, emit_32: 4 };
    if (widths[cal.name] && args.length === 1 && lit(args[0])) {
      return { data: args[0].value | 0, count: 1, width: widths[cal.name] };
    }
    if (cal.name === 'emit_fill' && args.length === 2 && lit(args[0]) && lit(args[1]) &&
        Number.isInteger(args[1].value) && args[1].value >= 0 && args[1].value < 2 ** 31) {
      return { data: args[0].value | 0, count: args[1].value, width: 1 };
    }
    return null;
  }

  // A statement outside the subset becomes a call that aborts: reaching it is an error, and the
  // translated code around it is unchanged.
  stmt(s, depth, label) {
    const saved = this.pre;
    const loops = this.loops.length;
    try {
      return this.stmtInner(s, depth, label);
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      this.pre = saved;
      this.loops.length = loops;
      this.stmtStubs.push(error.message);
      return [`js_unsupported_at(${cString(this.name())}, ${cString(error.message)});`];
    }
  }

  name() {
    return this.fn ? this.fn.id.name : '<top level>';
  }

  stmtInner(s, depth, label) {
    switch (s.type) {
      case 'EmptyStatement':
        return [];
      case 'BlockStatement':
        return ['{', ...this.block(s.body, depth + 1), '}'];
      case 'ExpressionStatement':
        return this.exprStatement(s.expression);
      case 'VariableDeclaration': {
        const out = [];
        for (const d of s.declarations) {
          if (!d.init) continue;
          out.push(...this.exprStatement({ type: 'AssignmentExpression', operator: '=', left: d.id, right: d.init, start: d.start, end: d.end }));
        }
        return out;
      }
      case 'ReturnStatement': {
        const saved = this.pre;
        this.pre = [];
        const v = s.argument ? D(this.ex(s.argument)) : 'NAN';
        const out = [...this.pre, `return ${v};`];
        this.pre = saved;
        return out;
      }
      case 'IfStatement': {
        const saved = this.pre;
        this.pre = [];
        const c = this.cond(s.test);
        const pre = this.pre;
        this.pre = saved;
        const out = [...pre, `if (${c}) {`, ...this.stmtBody(s.consequent, depth)];
        if (s.alternate) out.push('} else {', ...this.stmtBody(s.alternate, depth));
        out.push('}');
        return out;
      }
      case 'WhileStatement':
        return this.loop(label, s.test, null, s.body, false, depth);
      case 'DoWhileStatement':
        return this.loop(label, s.test, null, s.body, true, depth);
      case 'ForStatement': {
        const copy = this.copyLoop(s, depth);
        if (copy) return copy;
        const init = s.init ? (s.init.type === 'VariableDeclaration' ? this.stmt(s.init, depth) : this.exprStatement(s.init)) : [];
        return ['{', ...init, ...this.loop(label, s.test, s.update, s.body, false, depth), '}'];
      }
      case 'BreakStatement': {
        if (s.label) {
          const rec = this.labelMap.get(s.label.name);
          if (!rec) throw new Unsupported('break to unknown label');
          rec.brkUsed = true;
          return [`goto ${rec.brk};`];
        }
        return ['break;'];
      }
      case 'ContinueStatement': {
        const rec = s.label ? this.labelMap.get(s.label.name) : this.loops[this.loops.length - 1];
        if (!rec || rec.cont === undefined) throw new Unsupported('continue outside a loop');
        rec.contUsed = true;
        return [`goto ${rec.cont};`];
      }
      case 'LabeledStatement': {
        const body = s.body;
        if (['WhileStatement', 'DoWhileStatement', 'ForStatement'].includes(body.type)) {
          return this.stmt(body, depth, s.label.name);
        }
        const rec = { brk: `BRK_${this.p.labelCounter++}`, brkUsed: false };
        this.labelMap.set(s.label.name, rec);
        const lines = this.stmt(body, depth);
        if (rec.brkUsed) lines.push(`${rec.brk}: ;`);
        return lines;
      }
      case 'SwitchStatement': {
        const saved = this.pre;
        this.pre = [];
        const d = D(this.ex(s.discriminant));
        const pre = this.pre;
        this.pre = saved;
        const out = [...pre, `switch (js_switch_key(${d})) {`];
        for (const c of s.cases) {
          if (c.test) {
            if (c.test.type !== 'Literal' || typeof c.test.value !== 'number') throw new Unsupported('non-literal case');
            out.push(`case ${c.test.value}:`);
          } else {
            out.push('default:');
          }
          out.push(...this.block(c.consequent, depth + 1));
        }
        out.push('}');
        return out;
      }
      case 'ThrowStatement':
        throw new Unsupported('throw');
      default:
        throw new Unsupported(`statement ${s.type}`);
    }
  }

  stmtBody(s, depth) {
    if (s.type === 'BlockStatement') return this.block(s.body, depth + 1);
    return this.stmt(s, depth + 1);
  }

  // A loop as `for (;;) { test; body; CONT: ; update; }` with BRK after it.
  // The snapshot restore of the JS runMandreel: for (var i = 0; i < B; i++) heap32[i] = my_heap32[i];
  // Both views are 32-bit integer views, so the loop copies raw words: a memcpy of 4*B bytes. The bound B
  // is evaluated once (the body writes only the heap, so it cannot change), and i ends at B as in the JS.
  // Returns null for any other loop shape; the generic loop is then used.
  copyLoop(s, depth) {
    const init = s.init, t = s.test, u = s.update;
    if (!init || init.type !== 'VariableDeclaration' || init.declarations.length !== 1) return null;
    const d = init.declarations[0];
    if (d.id.type !== 'Identifier' || !d.init || d.init.type !== 'Literal' || d.init.value !== 0) return null;
    const i = d.id.name;
    if (!t || t.type !== 'BinaryExpression' || t.operator !== '<' || t.left.type !== 'Identifier' || t.left.name !== i) return null;
    if (!u || u.type !== 'UpdateExpression' || u.operator !== '++' || u.argument.type !== 'Identifier' || u.argument.name !== i) return null;
    const body = s.body.type === 'BlockStatement' && s.body.body.length === 1 ? s.body.body[0] : s.body;
    if (body.type !== 'ExpressionStatement') return null;
    const a = body.expression;
    if (a.type !== 'AssignmentExpression' || a.operator !== '=') return null;
    const elem = (m, name) => m.type === 'MemberExpression' && m.computed && m.object.type === 'Identifier' && m.object.name === name &&
      m.property.type === 'Identifier' && m.property.name === i;
    if (!(elem(a.left, 'heap32') && elem(a.right, 'my_heap32'))) return null;
    if (this.locals.has('heap32') || this.locals.has('my_heap32')) return null;
    // The bound must be free of side effects and of hoisted calls.
    const saved = this.pre;
    this.pre = [];
    const bound = this.ex(t.right);
    const boundPre = this.pre;
    this.pre = saved;
    if (boundPre.length !== 0) return null;
    const cb = `cb_${this.p.labelCounter++}`;
    const initLines = this.stmt(init, depth);
    const assign = this.exprStatement({ type: 'AssignmentExpression', operator: '=', left: t.left, right: t.right });
    return ['{', ...initLines,
      `double ${cb} = ${D(bound)};`,
      `if (${cb} == floor(${cb}) && ${cb} >= 0.0 && ${cb} <= (double)JS_LEN32) memcpy(js_heap, js_my_heap, (size_t)${cb} * 4u);`,
      `else js_unsupported_at(${cString(this.name())}, "copy loop bound outside the heap");`,
      ...assign, '}'];
  }

  loop(label, test, update, body, isDo, depth) {
    const rec = { cont: `CONT_${this.p.labelCounter++}`, contUsed: false, brk: `BRK_${this.p.labelCounter++}`, brkUsed: false };
    if (label) this.labelMap.set(label, rec);
    this.loops.push(rec);
    try {
      return this.loopInner(rec, test, update, body, isDo, depth);
    } finally {
      const at = this.loops.indexOf(rec);
      if (at >= 0) this.loops.length = at;
    }
  }

  loopInner(rec, test, update, body, isDo, depth) {
    let testPre = [];
    let testC = null;
    if (test) {
      const saved = this.pre;
      this.pre = [];
      testC = this.cond(test);
      testPre = this.pre;
      this.pre = saved;
    }
    const bodyLines = this.stmtBody(body, depth);
    let updLines = [];
    if (update) updLines = this.exprStatement(update);
    const out = [];
    if (isDo) {
      out.push('for (;;) {', ...bodyLines);
      if (rec.contUsed) out.push(`${rec.cont}: ;`);
      out.push(...testPre);
      if (testC !== null) out.push(`if (!(${testC})) break;`);
      out.push('}');
    } else if (testPre.length === 0 && updLines.length === 0) {
      out.push(`while (${testC === null ? '1' : testC}) {`, ...bodyLines);
      if (rec.contUsed) out.push(`${rec.cont}: ;`);
      out.push('}');
    } else {
      out.push('for (;;) {', ...testPre);
      if (testC !== null) out.push(`if (!(${testC})) break;`);
      out.push(...bodyLines);
      if (rec.contUsed) out.push(`${rec.cont}: ;`);
      out.push(...updLines);
      out.push('}');
    }
    if (rec.brkUsed) out.push(`${rec.brk}: ;`);
    return out;
  }
}

// The C value of typed value e converted to the C type of a parameter of type T ('i', 'l' or 'd').
function convTo(T, e) {
  if (T === 'd') return D(e);
  if (T === 'l') return e.t === 'd' ? `((int64_t)${e.c})` : L(e);
  return I(e);
}

function isSimpleObj(e) {
  return e.val !== undefined || /^t_\d+$/.test(e.c);
}

// The shift count of a shift operator: ToUint32(count) & 31.
function shiftCount(b) {
  if (b.val !== undefined) return `${(b.val >>> 0) & 31}u`;
  if (b.t === 'd') return `(js_touint32(${b.c}) & 31u)`;
  return `((uint32_t)${b.c} & 31u)`;
}

// Declarations of a function's locals (by final type) and temporaries (by their type).
const CTYPE = { i: 'int32_t', l: 'int64_t', d: 'double' };
function declLines(gen, types) {
  const out = [];
  const unused = ' __attribute__((unused))';
  const locals = [...gen.locals].filter((n) => !gen.params.has(n)).sort();
  for (const n of locals) {
    const t = types.get(n) || 'd';
    out.push(`  ${CTYPE[t]} ${cName(n)}${unused} = ${t === 'd' ? 'NAN' : '0'};`);
  }
  gen.tempTypes.forEach((t, j) => out.push(`  ${CTYPE[t]} t_${j}${unused};`));
  return out;
}

// One function: the types of its locals are a fixpoint of their assignments (least types that hold).
function translateFunction(prog, f) {
  const params = f.params.map((p) => p.name);
  let types = new Map();
  let gen = null;
  let bodyLines = null;
  for (let iter = 0; ; iter++) {
    if (iter > 12) throw new Error(`type fixpoint does not converge in ${f.id.name}`);
    gen = new FnGen(prog, f, params, types);
    bodyLines = gen.block(f.body.body, 0);
    const next = new Map(types);
    for (const [name, t] of gen.contrib) next.set(name, join(next.get(name), t));
    let same = next.size === types.size;
    if (same) for (const [name, t] of next) if (types.get(name) !== t) { same = false; break; }
    if (same) break;
    types = next;
  }
  return { gen, types, bodyLines };
}

// Functions whose parameters keep the double signature: the ones mandreel.c calls, and any function that is
// used as a value (registered in the function table, or assigned to a global function variable), since
// those are called through a pointer with double arguments.
function computeExcluded(prog, funcs) {
  const ex = new Set([...OVERRIDES, ...EXPORTED_FUNCS]);
  const calleeIds = new Set(), declIds = new Set();
  walk(prog.ast, (n) => {
    if (n.type === 'CallExpression' && n.callee.type === 'Identifier') calleeIds.add(n.callee);
    if (n.type === 'FunctionDeclaration') declIds.add(n.id);
  });
  walk(prog.ast, (n) => {
    if (n.type === 'Identifier' && prog.topFuncs.has(n.name) && !calleeIds.has(n) && !declIds.has(n)) ex.add(n.name);
  });
  return ex;
}

// Parameter types: a parameter is 'i' (int32), 'l' (int64) or 'd' (double) by the join of the types of its
// arguments at every direct call and of the values assigned to it in the body. It starts at 'i' (or 'd' when
// excluded) and rises until a whole-program pass changes nothing, so every call site and body agrees.
const MAX_PARAM_PASSES = 8;

function nextParamTypes(prog, funcs, contribs) {
  const next = new Map();
  for (const f of funcs) {
    const name = f.id.name;
    const cj = prog.callJoin.get(name) || [];
    const ct = contribs.get(name) || [];
    next.set(name, f.params.map((p, k) => {
      if (prog.excluded.has(name)) return 'd';
      const t = join(cj[k], ct[k]);
      return t === undefined ? 'i' : t;
    }));
  }
  return next;
}

function sameParamTypes(a, b) {
  for (const [name, ts] of a) {
    const other = b.get(name);
    if (ts.length !== other.length || ts.some((t, k) => t !== other[k])) return false;
  }
  return true;
}

// One translation of every function and of the top-level script, with the parameter types in prog.paramTypes.
function translateAll(prog, funcs) {
  prog.callJoin = new Map();
  prog.strings = [];
  prog.stmtStubs = [];
  prog.problems = new Map();
  const protos = [];
  const bodies = [];
  const contribs = new Map();
  for (const f of funcs) {
    const name = f.id.name;
    const params = f.params.map((p) => p.name);
    const ptys = prog.paramTypes.get(name);
    const plist = params.length ? params.map((p, k) => CTYPE[ptys[k]] + ' ' + cName(p)).join(', ') : 'void';
    const baseSig = `double ${fnName(name)}(${plist})`;
    if (OVERRIDES.has(name)) { protos.push(baseSig + ';'); continue; }
    let lines;
    try {
      const { gen, types, bodyLines } = translateFunction(prog, f);
      contribs.set(name, params.map((p) => gen.contrib.get(p)));
      for (const m of gen.stmtStubs) prog.stmtStubs.push(`${name}: ${m}`);
      lines = [`${baseSig} {`, ...declLines(gen, types), ...bodyLines.map((l) => '  ' + l), '  return NAN;', '}'];
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      prog.problems.set(name, error.message);
      lines = [`${baseSig} {`, `  js_unsupported(${cString(name)}, ${cString(error.message)});`, '  return NAN;', '}'];
    }
    // Small functions (accessors and wrappers) are static so GCC can inline them into their callers,
    // as V8 inlines them; everything else keeps external linkage, which keeps GCC's interprocedural
    // passes over the 3000 functions cheap (making all of them static took the -O2 compile from about
    // 25 s to over 5 minutes).
    let sig = baseSig;
    if (!EXPORTED_FUNCS.has(name) && lines.length <= SMALL_FUNCTION_LINES) {
      sig = `static __attribute__((unused)) ${baseSig}`;
      lines[0] = sig + ' {';
    }
    protos.push(sig + ';');
    bodies.push(`/* JS ${name} */`);
    for (const l of lines) bodies.push(l);
    bodies.push('');
  }
  const top = topLevel(prog);
  return { prog, protos, bodies, top, contribs };
}

function generate(src) {
  const ast = acorn.parse(src, { ecmaVersion: 2022, sourceType: 'script', locations: false });
  const prog = new Program(src, ast);
  prog.analyze();
  prog.finishGlobals();
  const funcs = [...prog.topFuncs.values()];
  prog.excluded = computeExcluded(prog, funcs);
  const allDouble = () => new Map(funcs.map((f) => [f.id.name, f.params.map(() => 'd')]));
  let types = new Map(funcs.map((f) => [f.id.name, f.params.map(() => 'i')]));
  for (const f of funcs) if (prog.excluded.has(f.id.name)) types.set(f.id.name, f.params.map(() => 'd'));
  for (let pass = 0; ; pass++) {
    prog.paramTypes = types;
    if (pass + 1 >= MAX_PARAM_PASSES) {
      prog.paramTypes = allDouble();
      return translateAll(prog, funcs);
    }
    const res = translateAll(prog, funcs);
    const next = nextParamTypes(prog, funcs, res.contribs);
    if (sameParamTypes(next, types)) return res;
    types = next;
  }
}

// Top-level script: the variable initializers and calls, in source order, as js_toplevel_init().
// Browser-only statements (window setup, DOM, IndexedDB, try blocks) are skipped and listed in the header.
function topLevel(prog) {
  const gen = new FnGen(prog, null, [], new Map());
  const lines = [];
  const skipped = [];
  for (const name of prog.fnvars) {
    if (prog.topFuncs.has(name)) lines.push(`  ${globalName(name)} = (void *)${fnName(name)};`);
  }
  for (const s of prog.ast.body) {
    if (s.type === 'FunctionDeclaration') continue;
    try {
      if (s.type === 'VariableDeclaration') {
        for (const d of s.declarations) {
          if (!d.init) continue;
          if (d.init.type === 'Identifier' && prog.topFuncs.has(d.init.name)) {
            lines.push(`  ${globalName(d.id.name)} = (void *)${fnName(d.init.name)};`);
            continue;
          }
          const st = gen.exprStatement({ type: 'AssignmentExpression', operator: '=', left: d.id, right: d.init, start: d.start, end: d.end });
          lines.push(...st.map((l) => '  ' + l));
        }
      } else if (s.type === 'ExpressionStatement') {
        const st = gen.exprStatement(s.expression);
        lines.push(...st.map((l) => '  ' + l));
      } else {
        skipped.push(s.type + ' at byte ' + s.start);
      }
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      skipped.push(`${s.type} at byte ${s.start}: ${error.message}`);
    }
  }
  return { lines, skipped, tempTypes: gen.tempTypes };
}

// The whole generated file.
function emitFile(src) {
  const { prog, protos, bodies, top } = generate(src);
  const out = [];
  out.push('/* Generated by quench-bench/c-analogues/tools/mandreel-to-c.cjs from JetStream Octane/mandreel.js. Do not edit. */');
  out.push('#include "mandreel-rt.h"');
  out.push('');
  out.push('/* JS globals (var and implicit). Function-valued globals hold a function pointer. */');
  for (const name of [...prog.globals].sort()) {
    if (prog.stringVars.has(name)) out.push(`const char *${strVar(name)} = NULL;`);
    else if (prog.fnvars.has(name)) out.push(`void *${globalName(name)} = NULL;`);
    else out.push(`double ${globalName(name)} = NAN;`);
  }
  out.push('');
  out.push('/* String literals passed to emit_string(), as UTF-16 code units. */');
  prog.strings.forEach((units, i) => out.push(`static const uint16_t k_str_${i}[] = {${units.join(',')}};`));
  out.push('/* Data segment runs: {data, count, width} records written by js_emit_ops (mandreel.c). */');
  prog.emitTables.forEach((ops, i) => {
    out.push(`static const struct js_emit_op k_emit_${i}[] = {`);
    for (let k = 0; k < ops.length; k += 8) {
      out.push(ops.slice(k, k + 8).map((o) => `{${o.data},${o.count},${o.width}}`).join(',') + ',');
    }
    out.push('};');
  });
  out.push('');
  out.push('/* Hand-written in mandreel.c: functions the benchmark needs that use JS features outside the subset. */');
  for (const name of [...OVERRIDES].sort()) {
    const f = prog.topFuncs.get(name);
    if (!f) continue;
    const params = f.params.map((p) => 'double ' + cName(p.name));
    out.push(`double ${fnName(name)}(${params.length ? params.join(', ') : 'void'});`);
  }
  out.push('');
  out.push('/* Prototypes. */');
  for (const l of protos) out.push(l);
  out.push('void js_toplevel_init(void);');
  out.push('');
  out.push('/* Functions. */');
  for (const l of bodies) out.push(l);
  out.push('/* Top-level script, in source order. Calls the setup functions that mandreel.c provides. */');
  out.push('void js_toplevel_init(void) {');
  top.tempTypes.forEach((t, i) => out.push(`  ${CTYPE[t]} t_${i} __attribute__((unused));`));
  for (const l of top.lines) out.push(l);
  out.push('}');
  out.push('');
  out.push('/* Numeric JS globals, by name, for the state check against node (mandreel.c). */');
  out.push('const struct js_global_entry js_global_table[] = {');
  for (const name of [...prog.globals].sort()) {
    if (!prog.fnvars.has(name) && !prog.stringVars.has(name) && !UNHASHED_GLOBALS.has(name)) out.push(`  {${cString(name)}, &${globalName(name)}},`);
  }
  out.push('  {0, 0}');
  out.push('};');
  out.push('');
  out.push('/* Skipped top-level statements (browser glue): */');
  for (const s of top.skipped) out.push('/* ' + s.replace(/\*\//g, '') + ' */');
  out.push('');
  return { text: out.join('\n'), prog, skipped: top.skipped };
}

function main(argv) {
  const [input, output] = argv;
  if (!input || !output) {
    console.error('usage: node tools/mandreel-to-c.cjs <Octane/mandreel.js> <out.c>');
    process.exit(2);
  }
  const src = fs.readFileSync(input, 'utf8');
  const { text, prog, skipped } = emitFile(src);
  fs.writeFileSync(output, text);
  const unsupported = [...prog.problems.entries()];
  console.error(`functions: ${prog.topFuncs.size}, stub functions: ${unsupported.length}, stub statements: ${prog.stmtStubs.length}, skipped top-level: ${skipped.length}`);
  if (process.env.MANDREEL_TO_C_REPORT) for (const [name, why] of unsupported) console.error(`  stub ${name}: ${why}`);
  if (process.env.MANDREEL_TO_C_REPORT) for (const s of skipped) console.error(`  top ${s}`);
  if (process.env.MANDREEL_TO_C_REPORT) for (const s of prog.stmtStubs) console.error(`  stmt ${s}`);
}

if (require.main === module) main(process.argv.slice(2));

module.exports = { emitFile, generate, Program, Unsupported, OVERRIDES, UNHASHED_GLOBALS, HEAP_ARRAYS, C_RESERVED, cName, fnName, globalName, numLit, cString, acorn };
