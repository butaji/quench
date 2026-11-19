#!/usr/bin/env node
// Vision ledger: one repeatable scan of every place the code departs from the vision
// (one semantics as staged rows, tiers as projections, generated never hand-written,
// C + 6 budgets, TS types as facts in every tier). Reading only: no build.
//
//   node tools/vision-ledger.mjs scan <tree> <branch> <out.json> [--owner <name>...]
//   node tools/vision-ledger.mjs gate <ir-budget.txt> <out.json>
//   node tools/vision-ledger.mjs combine <out-dir> <state.json> <scan.json>...
//
// `scan` reads one checkout and keeps only items in files owned by --owner (all owners
// when none is given). `combine` merges the scans, writes ledger.md and delta.md to
// <out-dir>, and records item keys in <state.json> so the next run can tell new, still
// open and closed items apart. tools/vision-ledger.sh runs the whole pass over every
// owner's pushed branch tip and v2-merged.
//
// Unlike the derivation-debt ratchet, the scan covers the whole tree: b-jit stencils,
// Cranelift helpers and every file that names an opcode count, not a fixed file list.
// "Calls a staged row" is decided by name (staged paths, names imported from staged,
// same-file helpers that do, followed transitively), so hand counts are an upper bound.

import fs from "node:fs";
import path from "node:path";

// ---------- ownership ----------
// First match wins. Paths are relative to the repository root.
const OWNERS = [
  [/^crates\/quench-bjit\/.*wasm/, "Wasm"],
  [/^crates\/quench-bjit\//, "b-jit"],
  [/^crates\/quench-runtime\/src\/(wasm\/|vm\/dispatch_wasm|vm\/compiled\/wasm|staged\/wasm|vm\/wasm)/, "Wasm"],
  [/^crates\/quench-runtime\/src\/vm\/compiled\/(lane_region|lane_compile|lane_facts|entry_cell|lane_body_abi|exit_protocol|osr_lane|ahead_entry|frameless)/, "u-jit"],
  [/^crates\/quench-runtime\/src\/vm\/compiled\/(emit_machine|lane_body_run)|^crates\/quench-runtime\/src\/vm\/(jit_backend|tier_policy|native_stencil|native_graph|native_object)/, "Compile pipeline"],
  [/^crates\/quench-runtime\/src\/vm\/compiled/, "Replace"],
  [/^crates\/quench-runtime\/src\/vm\/dispatch_fast\/(call|body_calls|native_call|return|lane_stack|activation)|^crates\/quench-runtime\/src\/(vm\/activation|register_file|staged\/(call|protocol|frame))/, "Calls and frames"],
  [/^crates\/quench-runtime\/src\/vm\/(dispatch_fast\/(generator|job|promise|await|completions)|generator|promise|job)/, "No-jit"],
  [/^crates\/quench-runtime\/src\/vm\/(dispatch_fast\/(in_place|builtin|native_rows|array_steps)|inplace|array|string|number|json|collections|object_keys|object_builtins|primitives|builtins|native_rows|native_lengths|intact_fuses|math|date|regexp|typed_array)/, "Builtins"],
  [/^crates\/quench-runtime\/src\/(heap|vm\/gc|vm\/environment|vm\/name_scope)/, "Env, GC and heap memory"],
  [/^crates\/quench-runtime\/src\/(vm\/(object|shape|preshaped|method_cache|field)|typed_ir|staged\/object)/, "Fact store"],
  [/^crates\/quench-runtime\/src\/(vm\/dispatch|vm\/lane|staged\/)/, "Derive"],
  [/^crates\/quench-runtime\/src\/(compile|bytecode|parse)/, "Front end"],
  [/^crates\/quench-node\//, "Node host"],
  [/^crates\/quench-runtime\//, "Derive"],
];
// Symbol-level overrides (file, symbol) -> owner, from the owners' own triage.
const SYMBOL_OWNERS = [
  [/vm\/dispatch_fast\/completions\.rs$/, /^lane_(complete_|return_|restore_caller|resume_after_call|resume_at|settle_promise|settled_then)/, "Calls and frames"],
  [/vm\/dispatch_fast\/generator_windows\.rs$/, /./, "Derive"],
];
export function ownerOfSymbol(rel, sym) {
  for (const [f, n, o] of SYMBOL_OWNERS) if (f.test(rel) && n.test(sym || "")) return o;
  return ownerOf(rel);
}
export function ownerOf(rel) {
  for (const [re, o] of OWNERS) if (re.test(rel)) return o;
  return "unowned";
}

// ---------- Rust text helpers ----------
// Blank out comments and string/char literal contents, keeping offsets and newlines.
function strip(src) {
  const out = src.split("");
  const n = src.length;
  let i = 0;
  const blank = (a, b) => { for (let k = a; k < b; k++) if (out[k] !== "\n") out[k] = " "; };
  while (i < n) {
    const c = src[i], d = src[i + 1];
    if (c === "/" && d === "/") { let j = src.indexOf("\n", i); if (j < 0) j = n; blank(i, j); i = j; continue; }
    if (c === "/" && d === "*") {
      let depth = 1, j = i + 2;
      while (j < n && depth) { if (src[j] === "/" && src[j + 1] === "*") { depth++; j += 2; } else if (src[j] === "*" && src[j + 1] === "/") { depth--; j += 2; } else j++; }
      blank(i, j); i = j; continue;
    }
    if (c === "r" && (d === '"' || d === "#") && !/[A-Za-z0-9_]/.test(src[i - 1] || "")) {
      let j = i + 1, hashes = 0;
      while (src[j] === "#") { hashes++; j++; }
      if (src[j] === '"') {
        const close = '"' + "#".repeat(hashes);
        let e = src.indexOf(close, j + 1); if (e < 0) e = n;
        blank(j + 1, e); i = e + close.length; continue;
      }
    }
    if (c === '"') {
      let j = i + 1;
      while (j < n && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      blank(i + 1, j); i = j + 1; continue;
    }
    if (c === "'") {
      if (d === "\\") { let j = i + 2; while (j < n && src[j] !== "'") j++; blank(i + 1, j); i = j + 1; continue; }
      if (src[i + 2] === "'") { blank(i + 1, i + 2); i += 3; continue; }
    }
    i++;
  }
  return out.join("");
}

function lineStarts(s) { const a = [0]; for (let i = 0; i < s.length; i++) if (s[i] === "\n") a.push(i + 1); return a; }
function lineOf(starts, off) { let lo = 0, hi = starts.length - 1; while (lo < hi) { const m = (lo + hi + 1) >> 1; if (starts[m] <= off) lo = m; else hi = m - 1; } return lo + 1; }
function matchBrace(s, open) { let depth = 0; for (let i = open; i < s.length; i++) { if (s[i] === "{") depth++; else if (s[i] === "}") { depth--; if (!depth) return i; } } return s.length; }

// Ranges of `#[cfg(test)]` items.
function testRanges(s) {
  const r = [];
  const re = /#\[cfg\((?:any\()?test[^\]]*\]/g;
  let m;
  while ((m = re.exec(s))) {
    const brace = s.indexOf("{", m.index), semi = s.indexOf(";", m.index);
    if (brace < 0 || (semi >= 0 && semi < brace)) continue;
    r.push([m.index, matchBrace(s, brace)]);
  }
  return r;
}
const inRanges = (r, off) => r.some(([a, b]) => off >= a && off <= b);

// Functions: name, start offset, body range.
function functions(s, tests) {
  const fns = [];
  const re = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g;
  let m;
  while ((m = re.exec(s))) {
    if (inRanges(tests, m.index)) continue;
    let i = m.index, depth = 0;
    for (i = re.lastIndex; i < s.length; i++) {
      const c = s[i];
      if (c === "(" || c === "<" || c === "[") depth++;
      else if (c === ")" || c === ">" || c === "]") { if (s[i - 1] !== "-" && s[i - 1] !== "=") depth--; }
      else if (c === ";" && depth <= 0) { i = -1; break; }
      else if (c === "{" && depth <= 0) break;
    }
    if (i < 0 || i >= s.length) continue;
    const end = matchBrace(s, i);
    fns.push({ name: m[1], at: m.index, open: i, end, body: s.slice(i, end + 1) });
  }
  return fns;
}

const isTestPath = rel => /(^|\/)(tests?|benches)(\/|\.rs$)|_tests?\.rs$|\/tests\//.test(rel);

function walk(dir, base, acc) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "target" || e.name === "vendor" || e.name.startsWith(".")) continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, base, acc);
    else if (e.name.endsWith(".rs")) acc.push(path.relative(base, p));
  }
  return acc;
}

// ---------- scan ----------
const RUNTIME = "crates/quench-runtime/src/";
const CATALOG = [ // files allowed to name opcodes: the definition, its binary form, staged decode, emission
  /^crates\/quench-runtime\/src\/bytecode\.rs$/, /^crates\/quench-runtime\/src\/bytecode\/(binary|op_catalog|instruction|opcodes)/,
  /^crates\/quench-runtime\/src\/staged\//, /^crates\/quench-runtime\/src\/compile\/(ast|emit)/,
];
const SEMANTIC_NAME = /(box|unbox|tag|truthy|truthiness|int32|to_int|uint32|compare|arith|number|double|f64_op|operation|to_bool|negate|bitwise|shift)/;
const RECHECK = /\.generation\b|generation\s*!=|\.entry\s*!=|script_calls|activation_limit|activation_count\(|tier_hook|record_js_jit_lane_entry|entry_published|note_site_callee|tier_may_run/;
const DECLARED = /\bdeclared_(parameters|field|fields|layout)\b|\bdeclared_types?\b|\bDeclaredTypes?\b|\bdeclared::|\bDeclaredKind\b|\bdeclared_fact/;
const TIERS = {
  interpreter: /^crates\/quench-runtime\/src\/vm\/(?!compiled)/,
  "b-jit": /^crates\/quench-bjit\/|^crates\/quench-runtime\/src\/vm\/compiled\/(base_region|base_body|stencil|bjit|wasm_stitch)/,
  "u-jit": /^crates\/quench-runtime\/src\/vm\/compiled\/(lane_region|osr_loop|lane_unit|lane_compile|object_slots|scalar)/,
};

function scan(tree, branch, wantOwners) {
  const files = walk(path.join(tree, "crates"), tree, []).filter(f => /^crates\/quench-(runtime|bjit|node)\//.test(f));
  const items = [];
  const wants = o => !wantOwners.length || wantOwners.includes(o);
  const add = (cat, rel, line, symbol, detail, extra = {}) => {
    const owner = extra.owner || ownerOfSymbol(rel, symbol);
    if (wants(owner)) items.push({ cat, owner, file: rel, line, symbol, detail, branch, key: `${cat}|${rel}|${symbol}` });
  };

  // Opcode predicates: `fn is_x(&self)` on the instruction types.
  const preds = new Set();
  for (const rel of files.filter(f => /src\/(bytecode\.rs|bytecode\/|staged\/operation)/.test(f))) {
    const s = fs.readFileSync(path.join(tree, rel), "utf8");
    for (const m of s.matchAll(/\bfn\s+(is_[a-z0-9_]+)\s*\(\s*&?self\s*\)/g)) preds.add(m[1]);
  }
  for (const p of ["is_wide", "is_module", "is_operand_field", "is_empty", "is_some", "is_none", "is_ok", "is_err", "is_null", "is_nan", "is_finite", "is_ascii", "is_zero", "is_negative", "is_positive"]) preds.delete(p);

  // Builtin keys from the Native enum.
  let nativeKeys = [];
  const declaredByTier = { interpreter: 0, "b-jit": 0, "u-jit": 0 };
  const fnIndex = []; // for builtin bodies

  for (const rel of files) {
    const testFile = isTestPath(rel);
    const src = fs.readFileSync(path.join(tree, rel), "utf8");
    if (rel.endsWith("heap/cell.rs")) {
      const m = /enum Native\s*\{([\s\S]*?)\n\}/.exec(src);
      if (m) {
        nativeKeys = [...strip(m[1]).matchAll(/\b([A-Z][A-Za-z0-9]+)\b/g)].map(x => x[1]);
        add("hand-table", rel, lineOf(lineStarts(src), m.index), "enum Native", `Native enum hand-lists ${nativeKeys.length} builtins; other registries should derive from one table`, { owner: "Builtins" });
      }
    }
    if (testFile) continue;
    const s = strip(src);
    const starts = lineStarts(s);
    const tests = testRanges(s);
    const fns = functions(s, tests);
    for (const t of Object.keys(TIERS)) if (TIERS[t].test(rel)) declaredByTier[t] += (s.match(new RegExp(DECLARED.source, "g")) || []).length;
    const ownerHere = ownerOf(rel);
    for (const f of fns) if (!/^(fmt|new|default|drop|clone|from|eq|hash)$/.test(f.name)) fnIndex.push({ rel, name: f.name, line: lineOf(starts, f.at), lines: lineOf(starts, f.end) - lineOf(starts, f.at), body: f.body });

    // A. Opcodes named outside the catalog: one item per file.
    // Only files that see the runtime's own opcode types: an import of bytecode / op_catalog
    // (or quench_runtime's from another crate), or a glob import inside the runtime with no
    // local Op/Class enum. quench-node's own Class/Op enums and oxc's is_module don't count.
    const localEnum = /\benum\s+(Op|Class)\b/.test(s);
    const seesOpcodes = !localEnum && (rel.startsWith("crates/quench-node/")
      ? /quench_runtime::(bytecode|op_catalog)/.test(src)
      : /\b(bytecode|op_catalog|opc)\b/.test(src) || (/^crates\/quench-runtime\//.test(rel) && /use\s+(super|crate::[a-z_:]+)::\*\s*;/.test(src)));
    if (seesOpcodes && !CATALOG.some(re => re.test(rel))) {
      const hits = [];
      const tables = fns.filter(f => /_handler$|handler_table|^handlers?$/.test(f.name)).map(f => [f.at, f.end]);
      for (const m of s.matchAll(/\b(?:Op|Class|opc)::([A-Z][A-Za-z0-9]*)/g))
        if (!inRanges(tests, m.index) && !inRanges(tables, m.index) && !/^[A-Z0-9_]+$/.test(m[1])) hits.push([m.index, m[0]]);
      for (const m of s.matchAll(/\.(is_[a-z0-9_]+)\(\)/g)) if (preds.has(m[1]) && !inRanges(tests, m.index) && !inRanges(tables, m.index)) hits.push([m.index, m[1] + "()"]);
      if (hits.length) {
        const names = {};
        for (const [, n] of hits) names[n] = (names[n] || 0) + 1;
        const top = Object.entries(names).sort((a, b) => b[1] - a[1]).slice(0, 6).map(([n, c]) => `${n} ${c}`).join(", ");
        const lines = [...new Set(hits.map(([o]) => lineOf(starts, o)))].sort((a, b) => a - b);
        add("opcode-named", rel, lines[0], "file", `${hits.length} opcode names outside the catalog (${top}); lines ${lines.slice(0, 12).join(",")}${lines.length > 12 ? ",…" : ""}`, { count: hits.length });
      }
    }

    // Names that count as "calls a staged row" in this file.
    const stagedNames = new Set();
    for (const m of s.matchAll(/use\s+[^;]*staged[^;]*;/g)) {
      const brace = /\{([\s\S]*)\}/.exec(m[0]);
      const list = brace ? brace[1] : m[0].split("::").pop();
      for (const nm of list.split(/[,{}\s]+/)) { const id = nm.split(/\s+as\s+/).pop().replace(/[^A-Za-z0-9_]/g, ""); if (id && id !== "self" && id !== "super") stagedNames.add(id); }
    }
    const callsStaged = new Map();
    const stagedRes = [...stagedNames].map(n => new RegExp(/^[a-z]/.test(n) ? `\\b${n}\\s*(::|\\(|!)` : `\\b${n}\\s*(::|\\{|\\(|<)`));
    const direct = f => /\bstaged::|\brow_step!?\b|\blane_row\b|\bstep_row(_of)?\b|\blane_suspension\b|\bsuspension_step\b|\bstaged_suspend\b/.test(f.body) || stagedRes.some(re => re.test(f.body));
    for (const f of fns) callsStaged.set(f.name, direct(f));
    for (let changed = true; changed;) { // same-file helpers that reach staged
      changed = false;
      for (const f of fns) if (!callsStaged.get(f.name) && fns.some(g => g !== f && callsStaged.get(g.name) && new RegExp(`\\b${g.name}\\s*[(:<]`).test(f.body))) { callsStaged.set(f.name, true); changed = true; }
    }

    // B. Handlers, stencils and Cranelift helpers that do not run a staged row.
    const isLaneFile = /^crates\/quench-runtime\/src\/vm\/dispatch_fast(\.rs|\/)/.test(rel);
    const isStencilRows = /^crates\/quench-bjit\/.*stencils\/(.*_)?rows[^/]*\.rs$/.test(rel);
    const isClif = /^crates\/quench-runtime\/src\/vm\/compiled(\.rs|\/)/.test(rel);
    for (const f of fns) {
      if (callsStaged.get(f.name)) continue;
      const line = lineOf(starts, f.at), len = lineOf(starts, f.end) - line;
      if (/_handler$|handler_table/.test(f.name)) continue; // dispatch tables
      if (isLaneFile && f.name.startsWith("lane_")) add("handler-not-row", rel, line, f.name, `interpreter fn ${f.name} (${len} lines) runs no staged row`);
      else if (isLaneFile && SEMANTIC_NAME.test(f.name) && len > 3) add("value-semantics-copy", rel, line, f.name, `interpreter helper ${f.name} (${len} lines) computes values without a staged row`);
      else if (isStencilRows && len > 1) add("stencil-not-row", rel, line, f.name, `b-jit stencil fn ${f.name} (${len} lines) calls no staged row`);
      else if (isClif && /\.ins\(\)/.test(f.body) && SEMANTIC_NAME.test(f.name)) add("clif-not-row", rel, line, f.name, `Cranelift helper ${f.name} (${len} lines) emits value semantics by hand`);
    }

    // D. Hand-written tables in tier code.
    if (/^crates\/quench-bjit\/|vm\/compiled|vm\/dispatch_fast|vm\/(builtins|native_rows|native_lengths|intact_fuses)|staged\/(layout|frame)|heap\/cell/.test(rel) && !/include!\(concat!\(env!\("OUT_DIR"/.test(src)) {
      for (const m of s.matchAll(/(?:pub(?:\([a-z]+\))?\s+)?(?:const|static)\s+([A-Z][A-Z0-9_]+)\s*:\s*&?(?:'static\s+)?\[[^=]*=\s*&?\[/g)) {
        if (inRanges(tests, m.index)) continue;
        const open = m.index + m[0].length - 1;
        let depth = 0, commas = 0, j = open;
        for (; j < s.length; j++) { const c = s[j]; if ("[({".includes(c)) depth++; else if ("])}".includes(c)) { depth--; if (!depth) break; } else if (c === "," && depth === 1) commas++; }
        if (commas >= 8) add("hand-table", rel, lineOf(starts, m.index), m[1], `${m[1]}: ${commas + 1} hand-listed entries`);
      }
      const lits = [...s.matchAll(/\bconst\s+([A-Z][A-Z0-9_]+)\s*:\s*(?:usize|isize|u8|u16|u32|u64|i8|i16|i32|i64)\s*=\s*(0x[0-9a-fA-F_]+|\d[\d_]*)\s*;/g)].filter(m => !inRanges(tests, m.index));
      if (lits.length >= 8 && /^crates\/quench-bjit\/|staged\/(layout|frame)|codes|layout/.test(rel))
        add("hand-table", rel, lineOf(starts, lits[0].index), "literal consts", `${lits.length} numeric constants typed in by hand (offsets/codes); generate from the runtime layout/catalog`);
      for (const m of s.matchAll(/\b(numbered_codes|codes|call_rows)!\s*\{/g)) add("hand-table", rel, lineOf(starts, m.index), `${m[1]}!`, `${m[1]}! hand-lists row names`);
    }

    // E. Per-call re-checks in call, return and entry paths.
    if (/vm\/dispatch_fast|vm\/compiled\/(entry|lane_body|exit|osr_lane|frameless)|crates\/quench-bjit\/.*call/.test(rel)) {
      for (const f of fns) {
        if (!/call|return|enter|entry|activation/.test(f.name)) continue;
        const ls = f.body.split("\n");
        const hits = [];
        ls.forEach((l, k) => { const m = RECHECK.exec(l); if (m) hits.push(`${lineOf(starts, f.open) + k}:${m[0].trim()}`); });
        if (hits.length) add("per-call-recheck", rel, lineOf(starts, f.at), f.name, `${f.name} re-checks per call: ${hits.slice(0, 6).join(", ")}${hits.length > 6 ? ", …" : ""} (a link or target should prove these once)`);
      }
    }
  }

  // C. Builtins with more than one body.
  const snake = k => k.replace(/([a-z0-9])([A-Z])/g, "$1_$2").replace(/([A-Z])([A-Z][a-z])/g, "$1_$2").toLowerCase();
  const prefixes = /^(object_prototype|array_prototype|string_prototype|number_prototype|map_prototype|set_prototype|object|array|string|number|math|json|reflect|map|set|promise|date|regexp|function|typed_array)_/;
  const seen = new Set();
  // Host, embedding and installer code is not a builtin body.
  const BODY_SKIP_FILE = /^crates\/quench-node\/|^crates\/quench-runtime\/src\/(compile|subset_tests|bytecode)|\/(intl_[a-z_]+|call_printer\/[a-z_]+|agent|atomics|disposal|value_serializer)\.rs$|\/(api_[a-z_]+|embedding|test_host|host_function|shadow_realm|error)\.rs$|\/napi\//;
  const BODY_SKIP_NAME = /^(is|has|ensure|call|alloc|throw|new|box|install|initialize|resolve|queue|advance|evaluate|settle|validate|print|napi|embedding|env|register|create_realm|realm|host)_/;
  for (const k of nativeKeys) {
    const full = snake(k);
    const tail = full.replace(prefixes, "");
    const keys = [full]; if (tail !== full && (tail.includes("_") || tail.length >= 6)) keys.push(tail);
    const re = new RegExp(`^(?:[a-z0-9]+_)*(?:${keys.join("|")})(?:_[a-z0-9]+)*$`);
    const bodies = fnIndex.filter(f => re.test(f.name) && f.lines >= 4 && !BODY_SKIP_FILE.test(f.rel) && !BODY_SKIP_NAME.test(f.name) && !/^\{\s*[A-Za-z_:.<>]+\([^;]*\)\s*\}$/s.test(f.body.trim()));
    const filesSet = new Set(bodies.map(b => b.rel));
    if (bodies.length >= 2 && filesSet.size >= 2 && bodies.length <= 12 && !seen.has(tail)) {
      seen.add(tail);
      const first = bodies[0];
      if (wants("Builtins"))
        items.push({ cat: "builtin-bodies", owner: "Builtins", file: first.rel, line: first.line, symbol: k, branch,
          key: `builtin-bodies|${k}`, detail: `${k}: ${bodies.length} bodies — ${bodies.map(b => `${b.rel.replace(RUNTIME, "")}:${b.line} ${b.name}`).join("; ")}` });
    }
  }

  // G. Declared TS types per tier.
  // Each tier's TS item is read from the branch that holds that tier's code.
  const TS_SOURCE = { interpreter: "Fact store", "b-jit": "b-jit", "u-jit": "u-jit" };
  for (const [t, n] of Object.entries(declaredByTier))
      if (n === 0 && wants(TS_SOURCE[t])) items.push({ cat: "ts-types-unused", owner: "Fact store", file: `(tier ${t})`, line: 0, symbol: t, branch, key: `ts-types-unused|${t}`,
        detail: `${t} reads no declared TS type (no reference to DeclaredTypes/declared facts in its files)` });
  return items;
}

// ---------- gate budgets ----------
function gate(file) {
  const txt = fs.readFileSync(file, "utf8");
  const items = [];
  const tag = path.basename(file).replace(/-ir-budget\.txt$/, "");
  const ownerFor = {};
  for (const m of txt.matchAll(/^([a-z0-9_]+):.*->\s*(.+)$/gm)) ownerFor[m[1]] = ownerFor[m[1]] || m[2].trim();
  const map = { "Cut calls and frames to C": "Calls and frames", "Cut builtin library code to C": "Builtins", "Unify fact store and object model": "Fact store",
    "Derive every tier from one semantics": "Derive", "Specialize u-jit on facts": "u-jit", "Cut environments, GC and heap memory": "Env, GC and heap memory",
    "Build b-jit as the interpreter's PE": "b-jit", "Replace hand-written tier code with generated": "Replace", "Compile pipeline: Cranelift and tier policy": "Compile pipeline" };
  const own = o => map[o] || o || "unowned";
  for (const m of txt.matchAll(/^([a-z0-9_]+): --(b-jit|u-jit) ([\d.]+) Ir\/iter, ceiling ([\d.]+) \(budget C \+ 6 = ([\d.]+)\) -> (.+)$/gm))
    items.push({ cat: "budget-over-c6", owner: own(m[6].trim()), file: `ir-budget ${tag}`, line: 0, symbol: `${m[1]} ${m[2]}`, branch: "v2-merged", key: `budget-over-c6|${m[1]}|${m[2]}`,
      detail: `${m[1]} ${m[2]}: ${m[3]} Ir/iter vs budget C + 6 = ${m[5]} (${(+m[3] / +m[5]).toFixed(1)}x)` });
  for (const m of txt.matchAll(/^([a-z0-9_]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)x\s+over\s+(.*)$/gm)) {
    const first = (m[7].match(/\d+\s+([A-Za-z_:<>]+)/) || [])[1] || "";
    let o = ownerFor[m[1]];
    if (!o) o = /call|return|native/.test(first) ? "Cut calls and frames to C" : /row<(50|89)|field/.test(first) ? "Unify fact store and object model" : /Vm::|json|string|array|regexp/i.test(first) ? "Cut builtin library code to C" : "Derive every tier from one semantics";
    items.push({ cat: "budget-over-floor", owner: own(o), file: `ir-budget ${tag}`, line: 0, symbol: m[1], branch: "v2-merged", key: `budget-over-floor|${m[1]}`,
      detail: `${m[1]} interpreter: ${m[2]} Ir/iter vs floor ${m[3]} (C ${m[5]}, ${m[6]}x C); top: ${m[7].slice(0, 110)}` });
  }
  return items;
}

// ---------- combine ----------
const CAT_TITLE = {
  "opcode-named": "Opcodes named outside the catalog (files)", "handler-not-row": "Interpreter fns that run no staged row",
  "value-semantics-copy": "Interpreter value-semantics copies", "stencil-not-row": "b-jit stencils that call no staged row",
  "clif-not-row": "Cranelift helpers with hand value semantics", "builtin-bodies": "Builtins with more than one body",
  "hand-table": "Hand-written tables", "per-call-recheck": "Per-call re-checks", "budget-over-c6": "JIT tiers over C + 6 (gate)",
  "budget-over-floor": "Interpreter over its C floor (gate)", "ts-types-unused": "TS types that reach no tier",
};
function combine(outDir, statePath, inputs) {
  const all = [];
  for (const f of inputs) all.push(...JSON.parse(fs.readFileSync(f, "utf8")));
  const byKey = new Map();
  for (const it of all) if (!byKey.has(it.key)) byKey.set(it.key, it);
  const items = [...byKey.values()];
  let prev = { keys: [] };
  try { prev = JSON.parse(fs.readFileSync(statePath, "utf8")); } catch {}
  const prevKeys = new Set(prev.keys), nowKeys = new Set(items.map(i => i.key));
  for (const it of items) it.status = prevKeys.has(it.key) ? "open" : (prev.keys.length ? "new" : "open");
  const closed = prev.keys.filter(k => !nowKeys.has(k));
  const owners = [...new Set(items.map(i => i.owner))].sort();
  const cats = Object.keys(CAT_TITLE);
  const stamp = new Date().toISOString().replace(/\.\d+Z$/, "Z");
  const weight = it => it.cat === "opcode-named" ? (+(/^(\d+)/.exec(it.detail) || [0, 1])[1]) : 1;

  let md = `# Vision ledger\n\nGenerated ${stamp} by tools/vision-ledger.mjs (reading only, no build). Branches: ${[...new Set(items.map(i => i.branch))].join(", ")}.\n\n`;
  md += `**${items.length} open items** (${items.filter(i => i.status === "new").length} new since the last run, ${closed.length} closed). Opcode names counted individually: ${items.filter(i => i.cat === "opcode-named").reduce((a, i) => a + weight(i), 0)}.\n\n`;
  md += `Each owner's items come from that owner's own pushed branch tip; gate items from the newest v2-merged ir-budget report. "Calls a staged row" is decided by name, so counts are an upper bound: triage before deleting.\n\n`;
  md += `| owner | ${cats.map(c => c.replace(/-/g, " ")).join(" | ")} | total |\n|---|${cats.map(() => "--:").join("|")}|--:|\n`;
  for (const o of owners) {
    const mine = items.filter(i => i.owner === o);
    md += `| ${o} | ${cats.map(c => mine.filter(i => i.cat === c).length || "").join(" | ")} | ${mine.length} |\n`;
  }
  md += `| **all** | ${cats.map(c => items.filter(i => i.cat === c).length || "").join(" | ")} | **${items.length}** |\n\n`;
  for (const o of owners) {
    md += `## ${o}\n\n`;
    for (const c of cats) {
      const mine = items.filter(i => i.owner === o && i.cat === c);
      if (!mine.length) continue;
      md += `### ${CAT_TITLE[c]} (${mine.length})\n\n`;
      for (const it of mine) md += `- ${it.status === "new" ? "**NEW** " : ""}\`${it.file}${it.line ? ":" + it.line : ""}\` ${it.detail} _(${it.branch})_\n`;
      md += "\n";
    }
  }
  if (closed.length) md += `## Closed since the last run (${closed.length})\n\n${closed.map(k => `- ${k}`).join("\n")}\n`;
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "ledger.md"), md);

  let delta = `Vision ledger ${stamp}: ${items.length} open, ${items.filter(i => i.status === "new").length} new, ${closed.length} closed.\n`;
  for (const o of owners) {
    const mine = items.filter(i => i.owner === o);
    const nw = mine.filter(i => i.status === "new");
    delta += `- ${o}: ${mine.length} open${nw.length ? `, ${nw.length} new` : ""} (${cats.filter(c => mine.some(i => i.cat === c)).map(c => `${c} ${mine.filter(i => i.cat === c).length}`).join(", ")})\n`;
  }
  fs.writeFileSync(path.join(outDir, "delta.md"), delta);
  fs.writeFileSync(statePath, JSON.stringify({ stamp, keys: [...nowKeys] }));
  process.stdout.write(delta);
}

// ---------- main ----------
const [cmd, ...args] = process.argv.slice(2);
if (cmd === "scan") {
  const [tree, branch, out, ...rest] = args;
  const owners = []; for (let i = 0; i < rest.length; i++) if (rest[i] === "--owner") owners.push(rest[++i]);
  const items = scan(path.resolve(tree), branch, owners);
  fs.writeFileSync(out, JSON.stringify(items));
  process.stdout.write(`${branch}: ${items.length} items\n`);
} else if (cmd === "gate") {
  const items = gate(args[0]);
  fs.writeFileSync(args[1], JSON.stringify(items));
  process.stdout.write(`gate: ${items.length} items\n`);
} else if (cmd === "combine") {
  combine(args[0], args[1], args.slice(2));
} else {
  process.stderr.write("usage: vision-ledger.mjs scan <tree> <branch> <out.json> [--owner X]... | gate <ir-budget.txt> <out.json> | combine <out-dir> <state.json> <scan.json>...\n");
  process.exit(2);
}
