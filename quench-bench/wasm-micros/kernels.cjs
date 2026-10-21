// Generic WebAssembly kernels for the link-order training corpus (never a scored bench):
// integer and floating-point loops, linear memory of each width, direct and indirect calls,
// a branch-table state machine, globals and memory growth. Training runs this file without and
// with `--jit`, so the interpreter's Wasm handlers and the compiler's pipeline are laid out
// with the rest of the executed code. The modules are built with V8's `wasm-module-builder.js`
// from Node's WPT fixtures.
"use strict";
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const builderPath = path.join(
  __dirname, "../../tests/node/test/fixtures/wpt/wasm/jsapi/wasm-module-builder.js");
vm.runInThisContext(fs.readFileSync(builderPath, "utf8"), { filename: builderPath });

// Each kernel runs this many rounds: enough to reach every loop's steady state in both modes.
const ROUNDS = Number(process.argv[2] ?? 3);
const local = (index) => [kExprLocalGet, index];
const set = (index) => [kExprLocalSet, index];
const tee = (index) => [kExprLocalTee, index];
const i32 = (value) => wasmI32Const(value);
// `while (local < limit) { body; local += 1 }` over i32 locals.
const countedLoop = (counter, limit, body) => [
  ...i32(0), ...set(counter),
  kExprBlock, kWasmStmt,
  kExprLoop, kWasmStmt,
  ...local(counter), ...local(limit), kExprI32GeS, kExprBrIf, 1,
  ...body,
  ...local(counter), ...i32(1), kExprI32Add, ...set(counter),
  kExprBr, 0,
  kExprEnd,
  kExprEnd,
];
const load = (opcode, offset) => [opcode, 0, offset];

const builder = new WasmModuleBuilder();
builder.addMemory(16, 64, false);
builder.addGlobal(kWasmI32, true, wasmI32Const(1)).exportAs("seed");

// Arithmetic: a multiply-xor-shift hash over a counter.
builder.addFunction("hash", kSig_i_i).addLocals({ i32_count: 2 }).addBody([
  ...countedLoop(1, 0, [
    ...local(2), ...i32(31), kExprI32Mul,
    ...local(1), ...local(1), ...i32(3), kExprI32ShrU, kExprI32Xor, kExprI32Add,
    ...i32(13), kExprI32Rol, ...set(2),
  ]),
  ...local(2),
]).exportFunc();

// Memory: fill `n` words, then sum them through every load width.
builder.addFunction("memory", kSig_i_i).addLocals({ i32_count: 3 }).addBody([
  ...countedLoop(1, 0, [
    ...local(1), ...i32(2), kExprI32Shl, ...local(1), ...i32(7), kExprI32Mul,
    kExprI32StoreMem, 2, 0,
  ]),
  ...countedLoop(1, 0, [
    ...local(1), ...i32(2), kExprI32Shl, ...tee(3), ...load(kExprI32LoadMem, 0),
    ...local(3), ...load(kExprI32LoadMem8U, 1), kExprI32Add,
    ...local(3), ...load(kExprI32LoadMem16S, 2), kExprI32Add,
    ...local(2), kExprI32Add, ...set(2),
    ...local(3), ...local(2), kExprI32StoreMem8, 0, 3,
  ]),
  ...local(2),
]).exportFunc();

// Direct calls: naive Fibonacci.
const fib = builder.addFunction("fib", kSig_i_i).addBody([
  ...local(0), ...i32(2), kExprI32LtS,
  kExprIf, kWasmI32,
  ...local(0),
  kExprElse,
  ...local(0), ...i32(1), kExprI32Sub, kExprCallFunction, 2,
  ...local(0), ...i32(2), kExprI32Sub, kExprCallFunction, 2,
  kExprI32Add,
  kExprEnd,
]).exportFunc();
if (fib.index !== 2) throw new Error("fib must be function 2 for its own calls");

// Indirect calls: four binary operators dispatched by the counter.
const sig = builder.addType(kSig_i_ii);
const operators = [kExprI32Add, kExprI32Sub, kExprI32Xor, kExprI32Mul].map((opcode, index) =>
  builder.addFunction(`op${index}`, sig).addBody([...local(0), ...local(1), opcode]).index);
builder.appendToTable(operators);
builder.addFunction("dispatch", kSig_i_i).addLocals({ i32_count: 2 }).addBody([
  ...countedLoop(1, 0, [
    ...local(2), ...local(1),
    ...local(1), ...i32(3), kExprI32And, kExprCallIndirect, sig, 0,
    ...set(2),
  ]),
  ...local(2),
]).exportFunc();

// A branch-table state machine over the bytes `memory` wrote.
builder.addFunction("states", kSig_i_i).addLocals({ i32_count: 3 }).addBody([
  ...countedLoop(1, 0, [
    kExprBlock, kWasmStmt,
    kExprBlock, kWasmStmt,
    kExprBlock, kWasmStmt,
    kExprBlock, kWasmStmt,
    ...local(1), ...load(kExprI32LoadMem8U, 0), ...local(2), kExprI32Add, ...i32(3), kExprI32And,
    kExprBrTable, 3, 0, 1, 2, 3,
    kExprEnd,
    ...local(2), ...i32(1), kExprI32Add, ...set(2), kExprBr, 2,
    kExprEnd,
    ...local(3), ...local(1), kExprI32Xor, ...set(3), kExprBr, 1,
    kExprEnd,
    ...local(2), ...i32(2), kExprI32Shl, ...set(2), kExprBr, 0,
    kExprEnd,
    ...local(3), ...i32(1), kExprI32Add, ...set(3),
  ]),
  ...local(2), ...local(3), kExprI32Add,
]).exportFunc();

// 64-bit integers and doubles: a mixing loop and a Newton iteration.
builder.addFunction("wide", kSig_i_i).addLocals({ i32_count: 1, i64_count: 1, f64_count: 1 }).addBody([
  ...wasmF64Const(1.5), kExprLocalSet, 3,
  ...countedLoop(1, 0, [
    kExprLocalGet, 2, ...local(1), kExprI64UConvertI32, kExprI64Add,
    kExprI64Const, 0x87, 0x01, kExprI64Mul, kExprI64Const, 17, kExprI64Rol, kExprLocalSet, 2,
    kExprLocalGet, 3, ...wasmF64Const(2), kExprLocalGet, 3, kExprF64Div, kExprF64Add,
    ...wasmF64Const(0.5), kExprF64Mul, kExprF64Sqrt, ...wasmF64Const(1), kExprF64Add,
    kExprLocalSet, 3,
  ]),
  kExprLocalGet, 2, kExprI32ConvertI64,
  kExprLocalGet, 3, kExprI32SConvertF64, kExprI32Add,
]).exportFunc();

// Globals, select and memory growth.
builder.addFunction("globals", kSig_i_i).addLocals({ i32_count: 1 }).addBody([
  ...countedLoop(1, 0, [
    kExprGlobalGet, 0, ...local(1), kExprI32Add,
    kExprGlobalGet, 0, ...i32(1), kExprI32Shl,
    ...local(1), ...i32(1), kExprI32And, kExprSelect,
    kExprGlobalSet, 0,
  ]),
  ...i32(0), kExprMemoryGrow, 0, kExprDrop,
  kExprGlobalGet, 0,
]).exportFunc();

const kernels = builder.instantiate().exports;
let checksum = 0;
for (let round = 0; round < ROUNDS; round++) {
  checksum ^= kernels.hash(200000);
  checksum ^= kernels.memory(20000);
  checksum ^= kernels.fib(22);
  checksum ^= kernels.dispatch(200000);
  checksum ^= kernels.states(20000);
  checksum ^= kernels.wide(100000);
  checksum ^= kernels.globals(200000);
}
console.log(`wasm kernels ${ROUNDS} rounds: ${checksum >>> 0}`);
