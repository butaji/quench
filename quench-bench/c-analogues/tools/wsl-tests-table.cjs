// Records the WSL tests (WSL/Test.js, 285 tests run in doTest's order) as the sequence of operations each one performs,
// and writes them as the C table wsl-tests-table.h. The helpers the tests call (doPrep, doLex, makeInt..makeEnum,
// callFunction, checkInt..checkDouble, checkNumber, checkEnum, checkLexerToken, checkFail) are wrapped in the VM context:
// each wrapper records its operation before it calls the original, so an operation that throws is in the table too.
// The checks and makers inside another check record nothing (depth gate). A checkFail records its predicate: the error
// class and the substring of its e.message.indexOf(...) test, read from the predicate's source.
//
// Operations (wsl-tests-table.h): P prepare(text); A argument (a kind and value, or a previous call's result); C call
// (program, result index, name, the number of pending arguments); K a check on a result (kind, expected); FB/FE a
// checkFail block; LX doLex(text); LT checkLexerToken on a token of a doLex result.
//
// Usage: node wsl-tests-table.cjs <jetstream root> <output .h>
"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const root = process.argv[2];
const outPath = process.argv[3];
if (!root || !outPath) {
    console.error("usage: node wsl-tests-table.cjs <jetstream root> <output .h>");
    process.exit(2);
}

const driver = fs.readFileSync(path.join(root, "JetStreamDriver.js"), "utf8");
const planStart = driver.indexOf('name: "WSL"');
const planEnd = driver.indexOf("tags:", planStart);
const files = [...driver.slice(planStart, planEnd).matchAll(/"\.\/(WSL\/[^"]+)"/g)].map(m => m[1]);

const ctx = vm.createContext({ console });
for (const f of files)
    new vm.Script(fs.readFileSync(path.join(root, f), "utf8"), { filename: f }).runInContext(ctx);

// ---- recording state ----
const ops = [];
let depth = 0;
let nprog = 0, nres = 0, nlex = 0;
let unobserved = 0;   // reads and writes the test makes through engine objects (pointers, boxes): not recorded
const progIds = new WeakMap();   // program -> prepare id
const valDesc = new WeakMap();   // TypedValue -> {kind, value} or {res}
const lexIds = new WeakMap();    // doLex result array -> lex id
const tokRef = new WeakMap();    // token -> {lex, index}

const ARG_KIND = { int: 0, uint: 1, uint8: 2, bool: 3, float: 4, double: 5, enum: 6, res: 7 };
const CHECK_KIND = { int: 0, uint: 1, uint8: 2, bool: 3, float: 4, double: 5, enum: 6, number: 7 };
const CLS_KIND = { WTypeError: 0, WTrapError: 1, WSyntaxError: 2 };

const origDoPrep = ctx.doPrep;
ctx.doPrep = function (code) {
    if (depth === 0) ops.push({ op: "P", text: code });
    const id = nprog++;
    depth++;
    try {
        const p = origDoPrep(code);
        progIds.set(p, id);
        return p;
    } finally {
        depth--;
    }
};

const origDoLex = ctx.doLex;
ctx.doLex = function (code) {
    const id = nlex++;
    if (depth === 0) ops.push({ op: "LX", lex: id, text: code });
    depth++;
    try {
        const arr = origDoLex(code);
        lexIds.set(arr, id);
        arr.forEach((tok, i) => tokRef.set(tok, { lex: id, index: i }));
        return arr;
    } finally {
        depth--;
    }
};

for (const [name, kind] of [["makeInt", "int"], ["makeUint", "uint"], ["makeUint8", "uint8"],
    ["makeBool", "bool"], ["makeFloat", "float"], ["makeDouble", "double"]]) {
    const orig = ctx[name];
    ctx[name] = function (program, value) {
        depth++;
        let tv;
        try { tv = orig(program, value); } finally { depth--; }
        valDesc.set(tv, { kind, value });
        return tv;
    };
}
{
    const orig = ctx.makeEnum;
    ctx.makeEnum = function (program, enumName, value) {
        depth++;
        let tv, v;
        try { tv = orig(program, enumName, value); v = tv.value; } finally { depth--; }
        valDesc.set(tv, { kind: "enum", value: v });
        return tv;
    };
}

// Buffers the test makes or reads and writes (EBuffer is a class binding of the context, so it is subclassed in place).
const bufIds = new WeakMap();
let nbuf = 0;
const scalar = v => typeof v === "number" || typeof v === "boolean";
function bufId(b) {
    const id = bufIds.get(b);
    if (id === undefined) throw new Error("recorder: an EBuffer the test has not made");
    return id;
}
// the buffer of a call's result (made by the call): reads of it are RGET, by the result id
const resBufs = new WeakMap();
ctx.__rec = {
    buf(b, size) {
        if (depth !== 0) return;
        bufIds.set(b, nbuf);
        ops.push({ op: "BUF", buf: nbuf++, size });
    },
    bset(b, i, v) {
        if (depth !== 0) return;
        if (!scalar(v) || (!bufIds.has(b) && !resBufs.has(b))) { unobserved++; return; }   // an engine box, or a pointer
        if (resBufs.has(b)) throw new Error("recorder: write to the buffer of a call result");
        ops.push({ op: "BSET", buf: bufId(b), idx: i, value: v });
    },
    bget(b, i, v) {
        if (depth !== 0) return;
        if ((!scalar(v) && v !== undefined) || (!bufIds.has(b) && !resBufs.has(b))) { unobserved++; return; }
        if (resBufs.has(b)) {
            ops.push({ op: "RGET", res: resBufs.get(b), idx: i, value: v === undefined ? null : v });
            return;
        }
        ops.push({ op: "BGET", buf: bufId(b), idx: i, value: v === undefined ? null : v });
    },
};
vm.runInContext(`(function() {
    const Orig = EBuffer;
    EBuffer = class extends Orig {
        constructor(size) { super(size); __rec.buf(this, size); }
        set(i, v) { __rec.bset(this, i, v); super.set(i, v); }
        get(i) { const v = super.get(i); __rec.bget(this, i, v); return v; }
    };
})()`, ctx);
{
    // TypedValue.box builds the test's own argument boxes (described when they are passed); its EPtr.box is not an operation
    const TV = vm.runInContext("TypedValue", ctx);
    const boxOrig = TV.box;
    TV.box = function (...args) {
        depth++;
        try { return boxOrig.apply(this, args); } finally { depth--; }
    };
}
{
    const desc = Object.getOwnPropertyDescriptor(vm.runInContext("TypedValue.prototype", ctx), "value");
    Object.defineProperty(vm.runInContext("TypedValue.prototype", ctx), "value", {
        get() {
            const d = valDesc.get(this);
            if (depth === 0) {
                if (!d || d.res === undefined) throw new Error("recorder: value of a typed value that is not a call result");
                depth++;
                let v;
                try { v = desc.get.call(this); } finally { depth--; }
                if (scalar(v) || v === null || v === undefined) ops.push({ op: "RV", res: d.res, value: v === undefined ? null : v });
                else unobserved++;   // a pointer result: the test reads through it (not recorded)
                return v;
            }
            return desc.get.call(this);
        },
    });
}

// A TypedValue the test builds itself: a pointer or an array reference to a buffer (its cell holds the EPtr or the
// EArrayRef), or a struct whose ePtr is the buffer's EPtr.
function describeTyped(tv, name) {
    const T = vm.runInContext("({ PtrType, ArrayRefType, StructType, TypeRef, EPtr, EArrayRef })", ctx);
    const t = tv.type;
    const ptrOf = e => {
        if (!(e instanceof T.EPtr)) throw new Error("recorder: unsupported pointer in " + name);
        return { buf: bufId(e.buffer), off: e.offset };
    };
    if (t instanceof T.PtrType) {
        const v = tv.ePtr.loadValue();
        if (!(v instanceof T.EPtr)) throw new Error("recorder: pointer argument that is not an EPtr in " + name);
        return { kind: "ptr", space: t.addressSpace, elem: String(t.elementType), ...ptrOf(v) };
    }
    if (t instanceof T.ArrayRefType) {
        const v = tv.ePtr.loadValue();
        if (!(v instanceof T.EArrayRef)) throw new Error("recorder: array reference argument in " + name);
        return { kind: "arrref", space: t.addressSpace, elem: String(t.elementType), len: v.length, ...ptrOf(v.ptr) };
    }
    if (t instanceof T.StructType) return { kind: "struct", name: t.name, ...ptrOf(tv.ePtr) };
    // an instance of a generic struct: its name and its type arguments' native names, as Foo<int32,uint8>
    if (t instanceof T.TypeRef && t.type instanceof T.StructType)
        return { kind: "struct", name: t.name + "<" + t.typeArguments.map(a => a.name).join(",") + ">", ...ptrOf(tv.ePtr) };
    throw new Error("recorder: unknown argument to callFunction(" + name + "): " + t);
}

const origCall = ctx.callFunction;
ctx.callFunction = function (program, name, typeArguments, argumentList) {
    if (typeArguments.length)
        throw new Error("recorder: type arguments in callFunction(" + name + ")");
    const prog = progIds.get(program);
    if (prog === undefined) throw new Error("recorder: callFunction on an unknown program");
    depth++;
    let args;
    try {
        args = argumentList.map(a => {
            const d = valDesc.get(a);
            if (d) return d;
            return describeTyped(a, name);
        });
    } finally {
        depth--;
    }
    const res = nres++;
    if (depth === 0) {
        for (const d of args) {
            if (d.res !== undefined) ops.push({ op: "A", kind: "res", res: d.res });
            else if (d.kind === "ptr" || d.kind === "arrref" || d.kind === "struct") ops.push({ op: "T", ...d });
            else ops.push({ op: "A", kind: d.kind, value: d.value });
        }
        ops.push({ op: "C", prog, res, name, nargs: args.length });
    }
    depth++;
    try {
        const r = origCall(program, name, typeArguments, argumentList);
        valDesc.set(r, { res });
        if (r.ePtr) resBufs.set(r.ePtr.buffer, res);
        return r;
    } finally {
        depth--;
    }
};

for (const [name, kind] of [["checkInt", "int"], ["checkUint", "uint"], ["checkUint8", "uint8"],
    ["checkBool", "bool"], ["checkFloat", "float"], ["checkDouble", "double"], ["checkEnum", "enum"],
    ["checkNumber", "number"]]) {
    const orig = ctx[name];
    ctx[name] = function (program, result, expected) {
        const d = valDesc.get(result);
        if (!d || d.res === undefined) throw new Error("recorder: " + name + " on a value that is not a call result");
        if (depth === 0) ops.push({ op: "K", kind, res: d.res, expected });
        depth++;
        try {
            return orig(program, result, expected);
        } finally {
            depth--;
        }
    };
}

{
    const orig = ctx.checkLexerToken;
    ctx.checkLexerToken = function (result, expectedIndex, expectedKind, expectedText) {
        const r = tokRef.get(result);
        if (!r) throw new Error("recorder: checkLexerToken on a token that is not from doLex");
        if (depth === 0) ops.push({ op: "LT", lex: r.lex, index: r.index, expectedIndex, expectedKind, expectedText });
        depth++;
        try {
            return orig(result, expectedIndex, expectedKind, expectedText);
        } finally {
            depth--;
        }
    };
}

{
    const orig = ctx.checkFail;
    ctx.checkFail = function (callback, predicate) {
        const src = predicate.toString();
        const cls = /instanceof\s+(\w+)/.exec(src);
        if (!cls || !(cls[1] in CLS_KIND)) throw new Error("recorder: checkFail predicate class in " + src);
        const sub = /indexOf\("((?:[^"\\]|\\.)*)"\)/.exec(src);
        const subText = sub ? JSON.parse('"' + sub[1] + '"') : null;
        if (depth === 0) ops.push({ op: "FB", cls: CLS_KIND[cls[1]], sub: subText });
        orig(callback, predicate);
        if (depth === 0) ops.push({ op: "FE" });
    };
}

// ---- the tests, in doTest's order ----
// tests is a top-level let of Test.js (not a property of the global), so it is reached through the context's script
const names = Array.from(vm.runInContext("(function() { const n = []; for (const s in tests) n.push(s); return n; })()", ctx));
names.sort();
const testOps = [];
for (const name of names) {
    ops.length = 0;
    depth = 0;
    nprog = 0;
    nres = 0;
    nlex = 0;
    try {
        vm.runInContext("tests[" + JSON.stringify(name) + "]()", ctx);
    } catch (e) {
        console.error("test " + name + " failed while recording: " + e.message);
        process.exit(1);
    }
    testOps.push({ name, ops: ops.slice() });
}

// ---- C output ----
function cstr(s) {
    let out = "\"";
    for (const b of Buffer.from(s, "utf8")) {
        if (b === 0x22) out += "\\\"";
        else if (b === 0x5c) out += "\\\\";
        else if (b >= 0x20 && b < 0x7f) out += String.fromCharCode(b);
        else out += "\\" + b.toString(8).padStart(3, "0");
    }
    return out + "\"";
}
function cdouble(v) {
    if (typeof v === "boolean") return v ? "1.0" : "0.0";
    if (v !== v) return "NAN";
    if (v === Infinity) return "INFINITY";
    if (v === -Infinity) return "-INFINITY";
    if (Object.is(v, -0)) return "-0.0";
    if (Number.isInteger(v) && Math.abs(v) < 1e15) return v.toFixed(0) + ".0";
    return v.toPrecision(17);
}

const rows = [];
const tests = [];
let nops = 0;
const counts = { P: 0, A: 0, C: 0, K: 0, FB: 0, FE: 0, LX: 0, LT: 0, BUF: 0, BSET: 0, BGET: 0, RGET: 0, RV: 0, T: 0 };
for (const t of testOps) {
    const first = nops;
    for (const o of t.ops) {
        counts[o.op]++;
        nops++;
        switch (o.op) {
        case "P": rows.push(`  {OP_PREP, 0, 0, 0, 0, ${cstr(o.text)}, ${Buffer.byteLength(o.text, "utf8")}},`); break;
        case "A":
            if (o.kind === "res") rows.push(`  {OP_ARG, ${ARG_KIND.res}, ${o.res}, 0, 0, NULL, 0},`);
            else rows.push(`  {OP_ARG, ${ARG_KIND[o.kind]}, 0, 0, ${cdouble(o.value)}, NULL, 0},`);
            break;
        case "C": rows.push(`  {OP_CALL, ${o.prog}, ${o.res}, ${o.nargs}, 0, ${cstr(o.name)}, 0},`); break;
        case "K": rows.push(`  {OP_CHECK, ${CHECK_KIND[o.kind]}, ${o.res}, ${typeof o.expected === "boolean" ? 1 : 0}, ${cdouble(o.expected)}, NULL, 0},`); break;
        case "FB": rows.push(`  {OP_FBEGIN, ${o.cls}, 0, 0, 0, ${o.sub === null ? "NULL" : cstr(o.sub)}, 0},`); break;
        case "FE": rows.push("  {OP_FEND, 0, 0, 0, 0, NULL, 0},"); break;
        case "BUF": rows.push(`  {OP_BUF, ${o.buf}, ${o.size}, 0, 0, NULL, 0},`); break;
        case "BSET": rows.push(`  {OP_BSET, ${o.buf}, ${o.idx}, ${typeof o.value === "boolean" ? 1 : 0}, ${cdouble(o.value)}, NULL, 0},`); break;
        case "BGET": rows.push(`  {OP_BGET, ${o.buf}, ${o.idx}, ${o.value === null ? 2 : typeof o.value === "boolean" ? 1 : 0}, ${o.value === null ? "0.0" : cdouble(o.value)}, NULL, 0},`); break;
        case "RGET": rows.push(`  {OP_RGET, ${o.res}, ${o.idx}, ${o.value === null ? 2 : typeof o.value === "boolean" ? 1 : 0}, ${o.value === null ? "0.0" : cdouble(o.value)}, NULL, 0},`); break;
        case "RV": rows.push(`  {OP_RESVAL, ${o.res}, 0, ${o.value === null ? 2 : typeof o.value === "boolean" ? 1 : 0}, ${o.value === null ? "0.0" : cdouble(o.value)}, NULL, 0},`); break;
        case "T": {
            const kindCode = { ptr: 0, arrref: 1, struct: 2 }[o.kind];
            const name = o.kind === "struct" ? o.name : o.elem;
            rows.push(`  {OP_TYPED, ${o.buf}, ${o.off}, ${kindCode}, ${o.len === undefined ? 0 : o.len}, ${cstr(o.space || "")}, 0, ${cstr(name)}},`);
            break;
        }
        case "LX": rows.push(`  {OP_LEX, ${o.lex}, 0, 0, 0, ${cstr(o.text)}, ${Buffer.byteLength(o.text, "utf8")}},`); break;
        case "LT": rows.push(`  {OP_LEXCHECK, ${o.lex}, ${o.index}, ${o.expectedIndex}, 0, ${cstr(o.expectedKind)}, 0, ${cstr(o.expectedText)}},`); break;
        }
    }
    tests.push(`  {${cstr(t.name)}, ${first}, ${t.ops.length}},`);
}

const header = `/* Generated by tools/wsl-tests-table.cjs from WSL/Test.js: the ${testOps.length} WSL tests as the operations each performs.
 * Do not edit by hand; regenerate with the script. Ops: ${nops} (${Object.entries(counts).map(([k, v]) => `${k} ${v}`).join(", ")}).
 * The operation kinds are in wsl-tests.h. */
#ifndef WSL_TESTS_TABLE_H
#define WSL_TESTS_TABLE_H

#include "wsl-tests.h"

static const WslOp WSL_TEST_OPS[] = {
${rows.join("\n")}
};

static const WslTest WSL_TESTS[] = {
${tests.join("\n")}
};

#define WSL_NTESTS ${testOps.length}
#define WSL_NOPS ${nops}

#endif
`;
fs.writeFileSync(outPath, header);
console.log(`tests ${testOps.length} ops ${nops} ` + Object.entries(counts).map(([k, v]) => `${k} ${v}`).join(" ") + ` unobserved ${unobserved}`);
