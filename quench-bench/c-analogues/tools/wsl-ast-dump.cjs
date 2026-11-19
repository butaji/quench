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
const unifyMode = process.argv.includes("--unify") || resolveMode;
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
if (unifyMode && !resolveMode) run(`function __unifyHarness(p) {
    const NL = String.fromCharCode(10);
    const plain = t => {
        if (t instanceof TypeRef) return t.typeArguments.every(plain);
        if (t instanceof PtrType || t instanceof ArrayRefType) return plain(t.elementType);
        return t instanceof TypeVariable || t instanceof StructType || t instanceof EnumType || t instanceof NativeType;
    };
    const out = [];
    const push = t => { if (out.length < 60 && plain(t)) out.push(t); };
    for (const t of p.types.values()) if (t instanceof StructType) for (const f of t.fields) push(f.type);
    for (const funcs of p.functions.values()) for (const f of funcs) { for (const q of f.parameters) push(q.type); push(f.returnType); }
    for (const pr of p.protocols.values()) for (const s of pr.signatures) { for (const q of s.parameters) push(q.type); push(s.returnType); }
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
    ForLoop: { s: [], k: [K("initialization", n => n._initialization), K("condition", n => n._condition), K("increment", n => n._increment), K("body", n => n._body)] },
    Break: { s: [], k: [] },
    Continue: { s: [], k: [] },
    TrapStatement: { s: [], k: [] },
    CommaExpression: { s: [], k: [K("list", n => n._list)] },
    NullLiteral: { s: [], k: [] },
    BoolLiteral: { s: [S("value", n => n._value)], k: [] },
    VariableRef: { s: [S("name", n => (anonRoles.get(n.variable) ? "anonVar" : n._name)), S("anon", n => anonRoles.get(n.variable) || null)], k: [] },
    CallExpression: { s: [S("name", n => n._name)], k: [K("typeArguments", n => n._typeArguments), K("argumentList", n => n._argumentList)] },
    DotExpression: { s: [S("fieldName", n => n._fieldName)], k: [K("base", n => n.base)] },
    IndexExpression: { s: [], k: [K("base", n => n.base), K("index", n => n._index)] },
    DereferenceExpression: { s: [], k: [K("ptr", n => n._ptr)] },
    MakePtrExpression: { s: [], k: [K("lValue", n => n._lValue)] },
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
const LIT_REFS = { s: [S("preferred", n => TGT(n.type.preferredType.type))], k: [] };
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

function dumpProgram(program) {
    const lines = ["program"];
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
        lines.push(indent + c + (scalars.length ? " " + scalars.join(" ") : ""));
        for (const kid of [...spec.k, ...rs.k]) walk(kid.get(x), depth + 1);
    };
    for (const statement of program._topLevelStatements) walk(statement, 1);
    return lines.join("\n");
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

const rows = inputs.map((input, i) => {
    let status = "ok";
    let dump = "";
    let nodes = 0;
    try {
        if (unifyMode) {
            if (input.kind === "doLex") { dump = ""; }
            else { anonRoles.clear(); resolveInput(input, stage); dump = run(resolveMode ? "__resolveHarness(__p)" : "__unifyHarness(__p)"); }
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
