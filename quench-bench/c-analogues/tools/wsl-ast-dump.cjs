// Node reference for the C WSL parser (quench-bench/c-analogues/wsl-parse.h).
//
// Loads the WSL row's own sources (the file list from JetStreamDriver.js, in order) into one
// vm context and collects the same inputs as the C analogue: the standard library, the literal
// arguments of doPrep(...) and doLex(...) in WSL/Test.js, and the ${addressSpace} expansions.
// For every input it prints one line:
//   index kind textFNV status nodes dumpFNV
// doPrep and stdlib inputs are parsed with the row's Parse.js into a fresh Program; the dump is a
// canonical pre-order walk (class name, scalar fields, list lengths, "-" for null). doLex inputs
// are lexed and their dump is the token list. Status is "ok" or "err:<error class>: <message>".
// Usage: node wsl-ast-dump.cjs [jetstream-root] [--dump N] [--bench ITERATIONS]
// (root defaults to ../../jetstream; --bench times the parse stage only, as wsl-ast-dump.c does)
"use strict";
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const argv = process.argv.slice(2);
const root = argv[0] && !argv[0].startsWith("--") ? argv[0] : path.join(__dirname, "..", "..", "jetstream");
const dumpAt = argv.indexOf("--dump") >= 0 ? +argv[argv.indexOf("--dump") + 1] : -1;
const benchAt = argv.indexOf("--bench") >= 0 ? +argv[argv.indexOf("--bench") + 1] : 0;
// --stage N: 0 parses each input (the Stage 1 dump); 1 clones the standard library (or parses a
// doPrep input into the clone and reduces it), then folds constexprs; 2 adds the NameResolver passes
// over types and protocols; 3 adds typedef resolution, recursive type checks and the synthesized
// struct and enum functions; 4 adds the NameResolver pass over functions (the full pre-check state).
const stage = argv.indexOf("--stage") >= 0 ? +argv[argv.indexOf("--stage") + 1] : 4;
const refsOn = stage >= 2;

function fnv(str) {
    let h = 0x811c9dc5;
    for (const b of Buffer.from(str, "utf8")) {
        h ^= b;
        h = Math.imul(h, 16777619) >>> 0;
    }
    return h >>> 0;
}
const hex8 = v => (v >>> 0).toString(16).padStart(8, "0");

// The WSL file list, exactly as the driver loads it.
const driver = fs.readFileSync(path.join(root, "JetStreamDriver.js"), "utf8");
const planStart = driver.indexOf('name: "WSL"');
const planEnd = driver.indexOf("tags:", planStart);
const files = [...driver.slice(planStart, planEnd).matchAll(/"\.\/(WSL\/[^"]+)"/g)].map(m => m[1]);

const ctx = vm.createContext({ console });
for (const f of files) {
    new vm.Script(fs.readFileSync(path.join(root, f), "utf8"), { filename: f }).runInContext(ctx);
}
const run = code => vm.runInContext(code, ctx);

// --unify: the pairwise unification harness (the C side is unify_harness in wsl-types.h). The first
// 60 types in the order: struct fields (statement order), function parameter and return types
// (functions map order), protocol signature types (statement order). One character per ordered
// pair: bit 0 is Node.unify, bit 1 is Node.equals.
const resolveMode = process.argv.includes("--resolve");
const unifyMode = process.argv.includes("--unify") || resolveMode || process.argv.includes("--literal") || process.argv.includes("--intrinsics") || process.argv.includes("--reasons");
// --resolve: for each function in map order, its own parameter types (AutoWrapped) resolved against
// its overload list: "index bit" (index -1 with "-" when nothing resolves; bit is whether the
// substituted return type equals the declared one).
if (resolveMode) run(`function __resolveHarness(p) {
    const NL = String.fromCharCode(10);
    let count = 0;
    for (const funcs of p.functions.values()) count += funcs.length;
    let text = "funcs " + count + NL;
    for (const funcs of p.functions.values()) for (const f of funcs) {
        const fresh = f.typeParameters.map(t => new TypeVariable(t.origin, t.name, t.protocol));
        const args = f.parameters.map(q => q.type.visit(new Substitution(f.typeParameters, fresh)).visit(new AutoWrapper()));
        try {
            const o = resolveOverloadImpl(p.functions.get(f.name), [], args, null);
            if (!o.func) { text += "-1 -" + NL; continue; }
            const sub = o.func.returnType.substituteToUnification(o.func.typeParameters, o.unificationContext).visit(new AutoWrapper());
            text += p.functions.get(f.name).indexOf(o.func) + " " + (sub.equals(f.returnType) ? 1 : 0) + NL;
        } catch (e) {
            if (!(e instanceof RangeError)) throw e;
            text += "E" + NL;
        }
    }
    return text;
}`);
const literalMode = process.argv.includes("--literal");
const intrinsicsMode = process.argv.includes("--intrinsics");
const reasonsMode = process.argv.includes("--reasons");
run(`function __harnessTypes(p) {
    const plain = t => {
        if (t instanceof TypeRef) return t.typeArguments.every(plain);
        if (t instanceof PtrType || t instanceof ArrayRefType || t instanceof ArrayType) return plain(t.elementType);
        return t instanceof TypeVariable || t instanceof StructType || t instanceof EnumType || t instanceof NativeType;
    };
    const out = [];
    const push = t => { if (out.length < 60 && plain(t)) out.push(t); };
    for (const t of p.types.values()) if (t instanceof StructType) for (const f of t.fields) push(f.type);
    for (const funcs of p.functions.values()) for (const f of funcs) { for (const q of f.parameters) push(q.type); push(f.returnType); }
    for (const pr of p.protocols.values()) for (const s of pr.signatures) { for (const q of s.parameters) push(q.type); push(s.returnType); }
    return out;
}`);
// --reasons: the toString of the first 60 harness types, the verify reason of unify for the first 15 pairs
// of them, and the failure list of every function whose own parameters match no overload.
if (reasonsMode) run(`function __reasonsHarness(p) {
    const NL = String.fromCharCode(10);
    const out = __harnessTypes(p);
    let text = "types " + out.length + NL;
    for (let i = 0; i < out.length; i++) text += "T " + i + " " + out[i].toString() + NL;
    const pairs = out.slice(0, 15);
    for (let i = 0; i < pairs.length; i++) for (let j = 0; j < pairs.length; j++) {
        let r;
        try {
            const ctx = new UnificationContext();
            if (!pairs[i].unify(ctx, pairs[j])) r = "nounify";
            else { const v = ctx.verify(); r = v.result ? "ok" : v.reason; }
        } catch (e) { if (!(e instanceof RangeError)) throw e; r = "E"; }
        text += "P " + i + " " + j + " " + r + NL;
    }
    for (const funcs of p.functions.values()) for (const f of funcs) {
        const fresh = f.typeParameters.map(t => new TypeVariable(t.origin, t.name, t.protocol));
        const args = f.parameters.map(q => q.type.visit(new Substitution(f.typeParameters, fresh)).visit(new AutoWrapper()));
        try {
            const o = resolveOverloadImpl(p.functions.get(f.name), [], args, null);
            if (!o.func) for (const failure of o.failures) text += "F " + failure.toString() + NL;
        } catch (e) { if (!(e instanceof RangeError)) throw e; text += "E" + NL; }
    }
    return text;
}`);
// --intrinsics: the native types' properties (Intrinsics.js), each value as its IEEE bits, one line per result.
if (intrinsicsMode) run(`function __intrinsicsHarness(p) {
    const NL = String.fromCharCode(10);
    const hex = x => { const d = new Float64Array([x]); return new BigUint64Array(d.buffer)[0].toString(16).padStart(16, "0"); };
    const values = [0, 1, -1, 255, 256, 65535, 2147483647, -2147483648, 2147483648, 4294967295, 4294967296, 16777217, 0.5, -2.5, 3.4e38, 1e40, -0.25];
    const formats = ["IntLiteral", "UintLiteral", "FloatLiteral", "DoubleLiteral"];
    let text = "";
    for (const name of ["void", "int32", "uint32", "uint8", "float32", "float64", "bool"]) {
        const t = p.types.get(name);
        text += name + " size " + t.size + NL;
        if (t.defaultValue !== undefined) text += name + " default " + hex(t.defaultValue) + NL;
        for (const v of values) {
            if (t.canRepresent) text += name + " canRepresent " + hex(v) + " " + (t.canRepresent(v) ? 1 : 0) + NL;
            if (t.createLiteral) text += name + " createLiteral " + hex(v) + " " + hex(t.createLiteral(null, v).value) + NL;
            if (t.successorValue) text += name + " successorValue " + hex(v) + " " + hex(t.successorValue(v)) + NL;
            for (const f of formats) if (t["formatValueFrom" + f]) text += name + " formatValue " + f + " " + hex(v) + " " + hex(t["formatValueFrom" + f](v)) + NL;
        }
        if (t.valuesEqual) for (const a of values) for (const b of values) text += name + " valuesEqual " + hex(a) + " " + hex(b) + " " + (t.valuesEqual(a, b) ? 1 : 0) + NL;
    }
    return text;
}`);
if (unifyMode && !resolveMode && !literalMode && !intrinsicsMode) run(`function __unifyHarness(p) {
    const NL = String.fromCharCode(10);
    const out = __harnessTypes(p);
    let text = "types " + out.length + NL;
    for (const a of out) {
        let row = "";
        for (const b of out) {
            // a unification that does not terminate overflows the stack: E, as in the C harness
            try { row += String.fromCharCode(48 + (a.unify(new UnificationContext(), b) ? 1 : 0) + (a.equals(b) ? 2 : 0)); }
            catch (e) { if (!(e instanceof RangeError)) throw e; row += "E"; }
        }
        text += row + NL;
    }
    return text;
}`);
// --literal: each literal type (the four GenericLiteralTypes with the values below, and NullType) against
// every harness type. The preferred type of a literal is the native type its name gives.
if (literalMode) run(`function __literalHarness(p) {
    const NL = String.fromCharCode(10);
    const out = __harnessTypes(p);
    const intValues = [0, 1, 255, 256, -1, 65535, 2147483647, -2147483648, 2147483648, 4294967295, 4294967296, 16777217, 9007199254740992];
    const fltValues = [0, 0.5, -2.5, 0.1, 16777217, 3.4e38, 1e40, 4294967296, -0.25];
    const kinds = [
        ["IntLiteralType", "int32", intValues], ["UintLiteralType", "uint32", intValues],
        ["FloatLiteralType", "float32", fltValues], ["DoubleLiteralType", "float64", fltValues]];
    const cell = (lt, t) => {
        try { return String.fromCharCode(48 + (lt.unify(new UnificationContext(), t) ? 1 : 0) + (lt.equals(t) ? 2 : 0)); }
        catch (e) { if (!(e instanceof RangeError)) throw e; return "E"; }
    };
    let text = "literal " + out.length + NL;
    kinds.forEach(([name, native, values], ki) => {
        const Cls = eval(name);
        values.forEach((value, vi) => {
            const lt = new Cls(null, value);
            lt.preferredType.type = p.types.get(native);
            let row = ki + " " + vi + " ";
            for (const t of out) row += cell(lt, t);
            text += row + NL;
        });
    });
    const nullType = new NullType(null);
    let row = "4 0 ";
    for (const t of out) row += cell(nullType, t);
    text += row + NL;
    return text;
}`);

// ---- input collection (the same rules as the C analogue) ----
const inputs = [];
inputs.push({ kind: "stdlib", text: run("standardLibrary") });
{
    const test = fs.readFileSync(path.join(root, "WSL/Test.js"), "utf8");
    const addressSpaces = run("addressSpaces");
    for (let i = 0; i < test.length; i++) {
        for (const name of ["doPrep(", "doLex("]) {
            if (!test.startsWith(name, i)) continue;
            if (i > 0 && (/\w/.test(test[i - 1]) || test[i - 1] === "$")) continue;
            let j = i + name.length;
            while (j < test.length && /\s/.test(test[j])) j++;
            if (j >= test.length) continue;
            const kind = name === "doLex(" ? "doLex" : "doPrep";
            const q = test[j];
            if (q === '"' || q === "'") {
                let k = j + 1;
                while (k < test.length && test[k] !== q) k += test[k] === "\\" ? 2 : 1;
                inputs.push({ kind, text: run("(" + test.slice(j, k + 1) + ")") });
                i = k;
            } else if (q === "`") {
                let k = j + 1;
                let interpolated = false;
                while (k < test.length && test[k] !== "`") {
                    if (test[k] === "\\") { k += 2; continue; }
                    if (test[k] === "$" && test[k + 1] === "{") {
                        interpolated = true;
                        k = test.indexOf("}", k) + 1;
                        continue;
                    }
                    k++;
                }
                const lit = test.slice(j, k + 1);
                if (interpolated) {
                    const fn = run("(function(addressSpace) { return " + lit + "; })");
                    for (const space of addressSpaces) inputs.push({ kind, text: fn(space) });
                } else
                    inputs.push({ kind, text: run("(" + lit + ")") });
                i = k;
            }
            break;
        }
    }
}

// ---- canonical dump ----
const f64bits = v => {
    const dv = new DataView(new ArrayBuffer(8));
    dv.setFloat64(0, v);
    return dv.getBigUint64(0).toString(16).padStart(16, "0");
};
// AnonymousVariables are created by ReadModifyWriteExpression; the dump names them by role.
const anonRoles = new Map();
const S = (label, get) => ({ label, get });
const K = (label, get) => ({ label, get });
// Each literal kind is its own class made by createLiteral, so the kind is read from the class.
const LITS = { int: run("IntLiteral"), uint: run("UintLiteral"), double: run("DoubleLiteral"), float: run("FloatLiteral") };
const LIT = n => Object.keys(LITS).find(k => n instanceof LITS[k]);
const SPEC = {
    FuncDef: { s: [S("name", n => n._name), S("isCast", n => n._isCast), S("shaderType", n => n._shaderType), S("isRestricted", n => n.isRestricted)],
        k: [K("returnType", n => n._returnType), K("typeParameters", n => n._typeParameters), K("parameters", n => n._parameters), K("body", n => n._body)] },
    NativeFuncInstance: { s: [S("name", n => n._name), S("isCast", n => n._isCast), S("shaderType", n => n._shaderType)],
        k: [K("returnType", n => n._returnType), K("typeParameters", n => n._typeParameters), K("parameters", n => n._parameters)] },
    NativeFunc: { s: [S("name", n => n._name), S("isCast", n => n._isCast), S("shaderType", n => n._shaderType), S("isRestricted", n => n.isRestricted)],
        k: [K("returnType", n => n._returnType), K("typeParameters", n => n._typeParameters), K("parameters", n => n._parameters)] },
    ProtocolFuncDecl: { s: [S("name", n => n._name), S("isCast", n => n._isCast), S("shaderType", n => n._shaderType)],
        k: [K("returnType", n => n._returnType), K("typeParameters", n => n._typeParameters), K("parameters", n => n._parameters)] },
    TypeRef: { s: [S("name", n => n._name)], k: [K("typeArguments", n => n._typeArguments)] },
    PtrType: { s: [S("addressSpace", n => n._addressSpace)], k: [K("elementType", n => n._elementType)] },
    ArrayRefType: { s: [S("addressSpace", n => n._addressSpace)], k: [K("elementType", n => n._elementType)] },
    ArrayType: { s: [], k: [K("elementType", n => n._elementType), K("numElements", n => n._numElements)] },
    TypeVariable: { s: [S("name", n => n._name)], k: [K("protocol", n => n._protocol)] },
    ConstexprTypeParameter: { s: [S("name", n => n._name)], k: [K("type", n => n._type)] },
    TypeOrVariableRef: { s: [S("name", n => n._name)], k: [] },
    ProtocolRef: { s: [S("name", n => n._name)], k: [] },
    TypeDef: { s: [S("name", n => n._name)], k: [K("typeParameters", n => n._typeParameters), K("type", n => n._type)] },
    StructType: { s: [S("name", n => n._name)], k: [K("typeParameters", n => n._typeParameters), K("fields", n => [...n._fields.values()])] },
    Field: { s: [S("name", n => n._name)], k: [K("type", n => n._type)] },
    EnumType: { s: [S("name", n => n._name)], k: [K("baseType", n => n._baseType), K("members", n => [...n._members.values()])] },
    EnumMember: { s: [S("name", n => n._name)], k: [K("value", n => n.value)] },
    ProtocolDecl: { s: [S("name", n => n._name)], k: [K("extends", n => n.extends), K("signatures", n => n._signatures)] },
    NativeType: { s: [S("name", n => n._name)], k: [K("typeParameters", n => n._typeParameters)] },
    FuncParameter: { s: [S("name", n => n._name)], k: [K("type", n => n._type)] },
    VariableDecl: { s: [S("name", n => n._name)], k: [K("type", n => n._type), K("initializer", n => n._initializer)] },
    Block: { s: [], k: [K("statements", n => n._statements)] },
    SwitchStatement: { s: [], k: [K("value", n => n._value), K("switchCases", n => n._switchCases)] },
    SwitchCase: { s: [], k: [K("value", n => n._value), K("body", n => n._body)] },
    Return: { s: [], k: [K("value", n => n._value)] },
    IfStatement: { s: [], k: [K("conditional", n => n._conditional), K("body", n => n._body), K("elseBody", n => n._elseBody)] },
    WhileLoop: { s: [], k: [K("conditional", n => n._conditional), K("body", n => n._body)] },
    DoWhileLoop: { s: [], k: [K("body", n => n._body), K("conditional", n => n._conditional)] },
    FunctionLikeBlock: { s: [], k: [K("returnType", n => n.returnType), K("argumentList", n => n.argumentList), K("parameters", n => n.parameters), K("body", n => n.body)] },
    ForLoop: { s: [], k: [K("initialization", n => n._initialization), K("condition", n => n._condition), K("increment", n => n._increment), K("body", n => n._body)] },
    Break: { s: [], k: [] },
    Continue: { s: [], k: [] },
    TrapStatement: { s: [], k: [] },
    CommaExpression: { s: [], k: [K("list", n => n._list)] },
    NullLiteral: { s: [], k: [] },
    BoolLiteral: { s: [S("value", n => n._value)], k: [] },
    VariableRef: { s: [S("name", n => (anonRoles.get(n.variable) || (n.variable && classOf(n.variable) === "AnonymousVariable") ? "anonVar" : n._name)), S("anon", n => (propsMode ? null : anonRoles.get(n.variable) || null))], k: [] },
    AnonymousVariable: { s: [], k: [] },
    CallExpression: { s: [S("name", n => n._name)], k: [K("typeArguments", n => n._typeArguments), K("argumentList", n => n._argumentList)] },
    DotExpression: { s: [S("fieldName", n => n._fieldName)], k: [K("base", n => n.base)] },
    IndexExpression: { s: [], k: [K("base", n => n.base), K("index", n => n._index)] },
    DereferenceExpression: { s: [], k: [K("ptr", n => n._ptr)] },
    MakePtrExpression: { s: [], k: [K("lValue", n => n._lValue)] },
    ConvertPtrToArrayRefExpression: { s: [], k: [K("lValue", n => n.lValue)] },
    MakeArrayRefExpression: { s: [], k: [K("lValue", n => n._lValue)] },
    LogicalNot: { s: [], k: [K("operand", n => n._operand)] },
    LogicalExpression: { s: [S("text", n => n._text)], k: [K("left", n => n._left), K("right", n => n._right)] },
    Assignment: { s: [], k: [K("lhs", n => n._lhs), K("rhs", n => n._rhs)] },
    ReadModifyWriteExpression: { s: [], k: [K("lValue", n => n._lValue), K("newValueExp", n => n.newValueExp), K("resultExp", n => n.resultExp)] },
    IdentityExpression: { s: [], k: [K("target", n => n._target)] },
    EnumLiteral: { s: [], k: [] },
    IntLiteral: { s: [S("kind", LIT), S("value", n => n.value)], k: [] },
    UintLiteral: { s: [S("kind", LIT), S("value", n => n.value)], k: [] },
    FloatLiteral: { s: [S("kind", LIT), S("bits", n => f64bits(n.value))], k: [] },
    DoubleLiteral: { s: [S("kind", LIT), S("bits", n => f64bits(n.value))], k: [] },
};
// --refs: the references NameResolver sets, appended after each node's own fields (and its
// children after the node's children). A target is Class@n<index> (native origin) or Class@u<index>.
const TGT = t => (t === null || t === undefined ? "-" : classOf(t) + "@" + (t.origin.originKind === "native" ? "n" : "u") + t.origin.index);
const TGTS = l => (l === null || l === undefined ? "-" : "[" + [...l].map(TGT).join(",") + "]");
const DOT_REFS = { s: [S("getOverloads", n => TGTS(n.possibleGetOverloads)), S("setOverloads", n => TGTS(n.possibleSetOverloads)), S("andOverloads", n => TGTS(n.possibleAndOverloads))], k: [] };
const LIT_REFS = { s: [S("preferred", n => (n.type.preferredType ? TGT(n.type.preferredType.type) : "-"))], k: [] };   // a literal the checker committed has a TypeRef type, with no preferredType
const REFS = {
    TypeRef: { s: [S("type", n => TGT(n.type))], k: [] },
    VariableRef: { s: [S("variable", n => TGT(n.variable))], k: [] },
    CallExpression: { s: [S("overloads", n => TGTS(n.possibleOverloads))], k: [K("returnType", n => n._returnType)] },
    DotExpression: DOT_REFS,
    IndexExpression: DOT_REFS,
    ProtocolRef: { s: [S("protocolDecl", n => TGT(n.protocolDecl))], k: [] },
    ProtocolFuncDecl: { s: [S("overloads", n => TGTS(n.possibleOverloads))], k: [] },
    Return: { s: [S("func", n => TGT(n.func))], k: [] },
    EnumLiteral: { s: [S("member", n => TGT(n.member))], k: [] },
    IntLiteral: LIT_REFS,
    UintLiteral: LIT_REFS,
    FloatLiteral: LIT_REFS,
    DoubleLiteral: LIT_REFS,
};
const classOf = n => (n.constructor.name === "GenericLiteral" ? LITCLASS[LIT(n)] : n.constructor.name);
const LITCLASS = { int: "IntLiteral", uint: "UintLiteral", double: "DoubleLiteral", float: "FloatLiteral" };
const scalarText = v => (v === null || v === undefined ? "-" : typeof v === "string" ? JSON.stringify(v) : String(v));

// The tree of each root (a statement, or any node), one root after another, from depth 1; no trailing newline.
function dumpRoots(roots) {
    const lines = [];
    const walk = (x, depth) => {
        const indent = " ".repeat(depth);
        if (x === null || x === undefined) { lines.push(indent + "-"); return; }
        if (Array.isArray(x)) {
            lines.push(indent + "list " + x.length);
            for (const el of x) walk(el, depth + 1);
            return;
        }
        const c = classOf(x);
        const spec = SPEC[c];
        if (!spec) throw new Error("dump: no spec for " + c);
        if (c === "ReadModifyWriteExpression") {
            anonRoles.set(x.oldValueVar, "old");
            anonRoles.set(x.newValueVar, "new");
        }
        const rs = refsOn && REFS[c] ? REFS[c] : { s: [], k: [] };
            const scalars = [...spec.s, ...rs.s].map(f => f.label + "=" + scalarText(f.get(x)));
            if (ebufMode) for (const k of ["ePtr", "returnEPtr", "resultEPtr"]) if (x[k]) scalars.push(k + "=" + JSON.stringify(x[k].toString()));
        lines.push(indent + c + (scalars.length ? " " + scalars.join(" ") : ""));
        for (const kid of [...spec.k, ...rs.k]) walk(kid.get(x), depth + 1);
    };
    for (const root of roots) walk(root, 1);
    return lines.join("\n");
}

function dumpProgram(program) {
    const body = dumpRoots(program._topLevelStatements);
    return body.length ? "program\n" + body : "program";
}

// --instantiate: inline()'s first loop (getUnique(f, []) for each non-generic function, in the order of
// program.functions), then every instance in the order of the funcInstantiator's _instances Map: a header
// (instance index, the generic function's class, its type argument count) and the instance's tree.
function instanceDump(program) {
    for (const funcList of program.functions.values())
        for (const f of funcList)
            if (!f.typeParameters.length) program.funcInstantiator.getUnique(f, []);
    const out = [];
    let k = 0;
    for (const [orig, list] of program.funcInstantiator._instances)
        for (const inst of list)
            out.push("instance " + k++ + " " + classOf(orig) + " " + inst.typeArguments.length + "\n" + dumpRoots([inst.func]) + "\n\n");
    return out.join("").slice(0, -1);   // the row's dump has no trailing newline
}

// The prepare pipeline (Prepare.js) up to and including name resolution: the standard library is
// parsed once and cloned per input; doPrep inputs are parsed into the clone and reduced by
// programWithUnnecessaryThingsRemoved; then constexpr folding and the three NameResolver passes.
let stdProgramCache = null;
function resolveInput(input, stage) {
    if (!stdProgramCache)
        stdProgramCache = run("(function() { var p = new Program(); parse(p, '/internal/stdlib', 'native', 27, standardLibrary); return p; })()");
    ctx.__std = stdProgramCache;
    run("__p = cloneProgram(__std)");
    if (input.kind !== "stdlib") {
        ctx.__text = input.text;
        run("parse(__p, '/internal/test', 'user', 0, __text)");
        run("__p = programWithUnnecessaryThingsRemoved(__p)");
    }
    let code = "foldConstexprs(__p);";
    if (stage >= 2) code += " __nr = createNameResolver(__p); resolveNamesInTypes(__p, __nr); resolveNamesInProtocols(__p, __nr);";
    if (stage >= 3) code += " resolveTypeDefsInTypes(__p); resolveTypeDefsInProtocols(__p); checkRecursiveTypes(__p); synthesizeStructAccessors(__p); synthesizeEnumFunctions(__p);";
    if (stage >= 4) code += " resolveNamesInFunctions(__p, __nr);";
    if (stage >= 5) code += " resolveTypeDefsInFunctions(__p);";
    if (stage >= 6) code += " flattenProtocolExtends(__p);";
    run(code);
    if (propsMode) run("check(__p); checkLiteralTypes(__p); resolveProperties(__p);");
    return run("__p");
}

function lexTokens(text) {
    ctx.__text = text;
    run("__lexer = new Lexer('/internal/test', 'native', 0, __text)");
    const out = [];
    for (;;) {
        const t = run("__lexer.next()");
        if (!t) break;
        out.push(t.kind + " " + JSON.stringify(t.text));
    }
    return out;
}

function parseInput(input) {
    let program = run("new Program()");
    ctx.__program = program;
    ctx.__text = input.text;
    // Same origin, kind and line offset as Prepare.js.
    run(input.kind === "stdlib"
        ? "parse(__program, '/internal/stdlib', 'native', 27, __text)"
        : "parse(__program, '/internal/test', 'user', 0, __text)");
    // --clone: dump cloneProgram(program) instead of the parse tree (the cloner pass)
    if (process.argv.includes("--clone") && input.kind === "stdlib") program = run("cloneProgram(__program)");
    return program;
}

// --check: the checker (Check.js) on the stage-6 state. Every Expression the checker visits is traced in
// visit order with the type its visit returned ("E class origin type"); a call also traces its chosen
// overload and result ("C name overload -> type"). A type error stops the input: its status carries the message.
const propsMode = process.argv.includes("--props");   // --props: check, the literal check and resolveProperties, then the stage-6 dump
const postMode = process.argv.includes("--post");   // --post: the checker and the post-check passes, status only
const instantiateMode = process.argv.includes("--instantiate");   // --instantiate: the post-check passes, then the instances
const checkMode = process.argv.includes("--check") || postMode;
const visitMode = process.argv.includes("--visit");   // --visit: the post-check passes, the instances, then the Visitor.js trace (one line per Node.visit)
if (visitMode) run(`function __visitHarness(p) {
    const NL = String.fromCharCode(10);
    const lines = [];
    // the instances are made before the trace starts: getUnique's own rewrites are not part of it
    for (const funcList of p.functions.values())
        for (const f of funcList)
            if (!f.typeParameters.length) p.funcInstantiator.getUnique(f, []);
    const orig = Node.prototype.visit;
    Node.prototype.visit = function(visitor) {
        lines.push(this.constructor.name);
        return orig.call(this, visitor);
    };
    try {
        p.visit(new Visitor());
        for (const [, list] of p.funcInstantiator._instances)
            for (const inst of list) inst.func.visit(new Visitor());
    } finally { Node.prototype.visit = orig; }
    return lines.join(NL);
}`);
// --late: LateChecker (LateChecker.js) over each function of inline()'s first loop: the instance is made, then, unless
// it is native, visited by LateChecker with the Node.visit trace on. Returns [status, trace lines].
const lateMode = process.argv.includes("--late");
if (lateMode) run(`function __lateHarness(p) {
    const NL = String.fromCharCode(10);
    const lines = [];
    const orig = Node.prototype.visit;
    let status = "ok";
    try {
        for (const funcList of p.functions.values())
            for (const f of funcList) {
                if (f.typeParameters.length) continue;
                const inst = p.funcInstantiator.getUnique(f, []);
                if (inst.isNative) continue;
                Node.prototype.visit = function(visitor) {
                    lines.push(this.constructor.name);
                    return orig.call(this, visitor);
                };
                try { inst.visit(new LateChecker()); } finally { Node.prototype.visit = orig; }
            }
    } catch (e) {
        status = "err:" + e.constructor.name + ": " + e.message;
    }
    return [status, lines.join(NL)];
}`);
// --layout: LateChecker, then StructLayoutBuilder (StructLayoutBuilder.js) on each function of inline()'s first loop.
// The log: one entry per struct as it is laid out ("struct NAME size=N", then "field NAME offset=N" per field),
// and one per accessor instance ("impl FIELD offset=N structSize=N fieldSize=N"). Returns [status, entries].
const layoutMode = process.argv.includes("--layout");
if (layoutMode) run(`function __layoutHarness(p) {
    const NL = String.fromCharCode(10);
    const entries = [];
    const origStruct = StructLayoutBuilder.prototype.visitStructType;
    const origNative = StructLayoutBuilder.prototype.visitNativeFuncInstance;
    StructLayoutBuilder.prototype.visitStructType = function(node) {
        const laid = node.size != null;
        origStruct.call(this, node);
        if (!laid) {
            let e = "struct " + node.name + " size=" + node.size;
            for (const f of node.fields) e += NL + "field " + f.name + " offset=" + f.offset;
            entries.push(e);
        }
    };
    StructLayoutBuilder.prototype.visitNativeFuncInstance = function(node) {
        origNative.call(this, node);
        const d = node.implementationData;
        if (d) entries.push("impl " + d.fieldName + " offset=" + d.offset + " structSize=" + d.structSize + " fieldSize=" + d.fieldSize);
    };
    let status = "ok";
    try {
        for (const funcList of p.functions.values())
            for (const f of funcList) {
                if (f.typeParameters.length) continue;
                const inst = p.funcInstantiator.getUnique(f, []);
                if (inst.isNative) continue;
                inst.visit(new LateChecker());
                inst.visit(new StructLayoutBuilder());
            }
    } catch (e) {
        status = "err:" + e.constructor.name + ": " + e.message;
    } finally {
        StructLayoutBuilder.prototype.visitStructType = origStruct;
        StructLayoutBuilder.prototype.visitNativeFuncInstance = origNative;
    }
    return [status, entries.join(NL)];
}`);
// --ebuf: LateChecker, StructLayoutBuilder and EBufferBuilder (EBufferBuilder.js, EBuffer.js, EPtr.js) on each function
// of inline()'s first loop. The output: the instance trees, each node with its EPtrs (ePtr, returnEPtr, resultEPtr as
// "B<index>:<offset>"), then "buffers N" and each buffer (Buffer.toString) in creation order. Every EBuffer made is
// recorded by a subclass installed over the global EBuffer binding (__allBuffers); eBufferCount restarts per input.
// The instance trees are printed by dumpRoots (ebufMode adds the EPtr scalars there).
const inlineMode = process.argv.includes("--inline");
// --eval: inline() as --inline, then Evaluator.runFunc on each non-generic function without parameters (not native)
const evalMode = process.argv.includes("--eval");
const ebufMode = process.argv.includes("--ebuf") || inlineMode || evalMode;   // --inline prints the EPtrs too
if (ebufMode) {
    run("var __allBuffers = [];");
    run("(function() { const Orig = EBuffer; EBuffer = class extends Orig { constructor(size) { super(size); __allBuffers.push(this); } }; })()");
}
if (ebufMode) run(`function __ebufHarness(p) {
    const NL = String.fromCharCode(10);
    eBufferCount = 0;
    __allBuffers.length = 0;
    const insts = [];
    for (const funcList of p.functions.values())
        for (const f of funcList) {
            if (f.typeParameters.length) continue;
            const inst = p.funcInstantiator.getUnique(f, []);
            if (inst.isNative) continue;
            inst.visit(new LateChecker());
            inst.visit(new StructLayoutBuilder());
            inst.visit(new EBufferBuilder(p));
            insts.push(inst);
        }
    return insts;
}`);
// --inline: inline() (Inline.js) on the post-check state; the EBuffer registry restarts first.
if (inlineMode || evalMode) run(`function __inlineHarness(p) {
    eBufferCount = 0;
    __allBuffers.length = 0;
    inline(p);
}`);
// --eval: each non-generic function, in the order of inline()'s first loop; the ones without parameters that are not
// native run Evaluator.runFunc on their inlined body. The value is the cell's bits: n<hex> for a number (NaN is nan),
// b0/b1, null, undef, ptr <EPtr>, arr <EArrayRef>; a WTrapError is "trap <message>" and any other error is "err".
if (evalMode) run(`function __evalDescribe(v) {
    if (v === undefined) return "undef";
    if (v === null) return "null";
    if (typeof v === "boolean") return v ? "b1" : "b0";
    if (typeof v === "number") {
        if (v !== v) return "nan";
        const dv = new DataView(new ArrayBuffer(8));
        dv.setFloat64(0, v);
        return "n" + dv.getBigUint64(0).toString(16).padStart(16, "0");
    }
    if (v instanceof EPtr) return "ptr " + v.toString();
    if (v instanceof EArrayRef) return "arr " + v.toString();
    return "other";
}`);
if (evalMode) run(`function __evalHarness(p) {
    const NL = String.fromCharCode(10);
    const lines = [];
    let idx = 0;
    for (const funcList of p.functions.values())
        for (const f of funcList) {
            if (f.typeParameters.length) continue;
            const i = idx++;
            const inst = p.funcInstantiator.getUnique(f, []);
            if (inst.isNative || inst.parameters.length) continue;
            let r;
            try {
                const ptr = new Evaluator(p).runFunc(inst);
                r = ptr === null ? "fell" : __evalDescribe(ptr.loadValue());
            } catch (e) {
                r = e instanceof WTrapError ? "trap " + e.syntaxErrorMessage : "err";
            }
            lines.push("eval " + i + " " + f.name + " " + r);
        }
    return lines.join(NL);
}`);
if (postMode || instantiateMode || visitMode || lateMode || layoutMode || ebufMode) run(`function __postHarness(p) {
    check(p);
    checkLiteralTypes(p);
    resolveProperties(p);
    findHighZombies(p);
    checkLiteralTypes(p);
    checkProgramWrapped(p);
    checkReturns(p);
    checkUnreachableCode(p);
    checkLoops(p);
    checkRecursion(p);
    checkProgramWrapped(p);
    findHighZombies(p);
    return "";
}`);
if (checkMode) run(`function __checkHarness(p) {
    const NL = String.fromCharCode(10);
    const lines = [];
    const orig = Node.prototype.visit;
    Node.prototype.visit = function(visitor) {
        const r = orig.call(this, visitor);
        if (visitor instanceof Checker && this instanceof Expression) {
            lines.push("E " + __cls(this) + " " + (this.origin ? (this.origin.originKind === "native" ? "n" : "u") + this.origin.index : "-") + " " + (r ? r.toString() : "-"));
            if (this instanceof CallExpression) lines.push("C " + this.name + " " + (this.func ? this.func.toDeclString() : "-") + " -> " + (this.resultType ? this.resultType.toString() : "-"));
        }
        return r;
    };
    try { check(p); } finally { Node.prototype.visit = orig; }
    return lines.join(NL);
}`);
ctx.__cls = classOf;

const rows = inputs.map((input, i) => {
    let status = "ok";
    let dump = "";
    let nodes = 0;
    try {
        if (evalMode) {
            if (input.kind !== "doLex") {
                anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)");
                run("__inlineHarness(__p)");
                dump = run("__evalHarness(__p)");
            }
        } else if (inlineMode) {
            if (input.kind !== "doLex") {
                anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)");
                run("__inlineHarness(__p)");
                // the instances in the order of _instances, as instanceDump; then the buffers as --ebuf
                const parts = [];
                let k = 0;
                for (const [orig, list] of run("__p.funcInstantiator._instances"))
                    for (const inst of list) parts.push("instance " + k++ + " " + classOf(orig) + " " + inst.typeArguments.length + "\n" + dumpRoots([inst.func]));
                parts.push("buffers " + run("__allBuffers.length"));
                for (const b of run("__allBuffers")) parts.push(b.toString());
                dump = parts.join("\n");
            }
        } else if (ebufMode) {
            if (input.kind !== "doLex") {
                anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)");
                const insts = run("__ebufHarness(__p)");
                const parts = insts.map(inst => dumpRoots([inst]));
                parts.push("buffers " + run("__allBuffers.length"));
                for (const b of run("__allBuffers")) parts.push(b.toString());
                dump = parts.join("\n");
            }
        } else if (layoutMode) {
            if (input.kind !== "doLex") {
                anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)");
                const [st, text] = run("__layoutHarness(__p)");
                if (st !== "ok") status = st;
                dump = text;
            }
        } else if (lateMode) {
            if (input.kind !== "doLex") {
                anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)");
                const [st, trace] = run("__lateHarness(__p)");
                // the trace is kept up to an error, as the C driver prints it
                if (st !== "ok") status = st;
                dump = trace;
            }
        } else if (visitMode) {
            if (input.kind !== "doLex") { anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)"); dump = run("__visitHarness(__p)"); nodes = dump.split("\n").length; }
        } else if (instantiateMode) {
            if (input.kind !== "doLex") { anonRoles.clear(); resolveInput(input, 6); run("__postHarness(__p)"); dump = instanceDump(ctx.__p); nodes = dump.split("\n").length - 1; }
        } else if (checkMode) {
            if (input.kind !== "doLex") { anonRoles.clear(); resolveInput(input, 6); dump = run(postMode ? "__postHarness(__p)" : "__checkHarness(__p)"); }
        } else if (unifyMode) {
            if (input.kind === "doLex") { dump = ""; }
            else { anonRoles.clear(); resolveInput(input, stage); dump = run(resolveMode ? "__resolveHarness(__p)" : literalMode ? "__literalHarness(__p)" : intrinsicsMode ? "__intrinsicsHarness(__p)" : reasonsMode ? "__reasonsHarness(__p)" : "__unifyHarness(__p)"); }
        } else if (input.kind === "doLex") {
            const toks = lexTokens(input.text);
            dump = toks.join("\n");
            nodes = toks.length;
        } else if (stage >= 1) {
            anonRoles.clear();
            dump = dumpProgram(resolveInput(input, stage));
            nodes = dump.split("\n").length - 1;
        } else {
            anonRoles.clear();
            dump = dumpProgram(parseInput(input));
            nodes = dump.split("\n").length - 1;
        }
    } catch (e) {
        status = "err:" + e.constructor.name + ": " + e.message;
        dump = "";
        nodes = 0;
    }
    return { i, kind: input.kind, textHash: hex8(fnv(input.text)), status, nodes, dump, dumpHash: hex8(fnv(dump)) };
});

if (lateMode || layoutMode || ebufMode || inlineMode) {
    const out = [];
    for (const r of rows) {
        if (r.kind === "doLex") continue;
        out.push("input " + r.i + " " + r.kind + " " + r.status);
        if (r.dump) out.push(r.dump);
    }
    console.log(out.join("\n"));
    process.exit(0);
}
if (checkMode) {
    const out = [];
    for (const r of rows) {
        if (r.kind === "doLex") continue;
        out.push("input " + r.i + " " + r.kind + " " + r.status);
        if (r.dump && !postMode) out.push(r.dump);
    }
    console.log(out.join("\n"));
    process.exit(0);
}
if (unifyMode) {
    const fs = require("fs");
    const bytes = Buffer.from(rows.map(r => r.dump).join(""));
    for (let off = 0; off < bytes.length;) off += fs.writeSync(1, bytes, off, bytes.length - off);
    process.exit(0);
}
for (const r of rows) console.log([r.i, r.kind, r.textHash, r.status, r.nodes, r.dumpHash].join(" "));
const okCount = rows.filter(r => r.status === "ok").length;
const totalNodes = rows.reduce((a, r) => a + r.nodes, 0);
let all = 0x811c9dc5;
for (const r of rows) all = Math.imul(all ^ parseInt(r.dumpHash, 16), 16777619) >>> 0;
console.log("summary inputs " + rows.length + " ok " + okCount + " errors " + (rows.length - okCount) +
    " nodes " + totalNodes + " hash " + hex8(all));
// Timed parse stage (the C side's --bench): every iteration parses all stdlib and doPrep inputs.
if (benchAt > 0) {
    const parseable = inputs.filter((input, i) => i === 0 || input.kind !== "doLex");
    let elapsed = 0, parsed = 0, failed = 0;
    for (let it = 0; it < benchAt; it++) {
        const t0 = performance.now();
        let p = 0, f = 0;
        parseable.forEach((input, k) => {
            try {
                parseInput(input);
                p++;
            } catch (e) {
                f++;
            }
        });
        elapsed += performance.now() - t0;
        parsed = p;
        failed = f;
    }
    console.log("js parse " + benchAt + " iterations " + elapsed.toFixed(3) + " ms");
    console.log("parsed " + parsed + " failed " + failed);
    process.exit(0);
}

// Lexer totals over every input (stage 0 only) (not in --refs mode) (the workload of the lexer analogue wsl.c).
if (stage === 0) {
    let chars = 0, tokens = 0, errors = 0;
    inputs.forEach((input, i) => {
        chars += input.text.length;
        ctx.__text = input.text;
        run("__lexer = new Lexer('/internal/test', '" + (i === 0 || input.kind !== 'doPrep' ? 'native' : 'user') + "', 0, __text)");
        try {
            for (;;) {
                const t = run("__lexer.next()");
                if (!t) break;
                tokens++;
            }
        } catch (e) {
            errors++;
        }
    });
    console.log("lexer inputs " + inputs.length + " chars " + chars + " tokens " + tokens + " errors " + errors);
}
if (dumpAt >= 0) console.log(rows[dumpAt].dump);
