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
// Team of plan-2026-11-19 (rebalance/plan-2026-11-19.md).
const CALL_PROOFS = "Prove call checks once";
const OWNERS = [
  [/^crates\/quench-runtime\/src\/(vm\/dispatch_fast\/wasm|wasm\.rs$)/, "Wasm"],
  [/^crates\/quench-runtime\/src\/vm\/dispatch_fast\/completions\.rs$/, "Calls and frames"],
  [/^crates\/quench-bjit\/.*wasm/, "Wasm"],
  [/^crates\/quench-bjit\//, "b-jit"],
  [/^crates\/quench-runtime\/src\/(wasm\/|vm\/dispatch_wasm|vm\/compiled\/wasm|staged\/wasm|vm\/wasm)/, "Wasm"],
  [/^crates\/quench-runtime\/src\/vm\/compiled\/(lane_region|lane_compile|lane_facts|entry_cell|lane_body_abi|exit_protocol|osr_lane|ahead_entry|frameless)/, "u-jit"],
  [/^crates\/quench-runtime\/src\/vm\/compiled\/(emit_machine|lane_body_run)|^crates\/quench-runtime\/src\/vm\/(jit_backend|tier_policy|native_stencil|native_graph|native_object)/, "Compile pipeline"],
  [/^crates\/quench-runtime\/src\/vm\/compiled/, CALL_PROOFS],
  [/^crates\/quench-runtime\/src\/vm\/dispatch_fast\/(call|body_calls|native_call|return|lane_stack|activation)|^crates\/quench-runtime\/src\/(vm\/activation|register_file|staged\/(call|protocol|frame))/, "Calls and frames"],
  [/^crates\/quench-runtime\/src\/vm\/(dispatch_fast\/(generator|job|promise|await|completions)|generator|promise|job)/, "No-jit"],
  [/^crates\/quench-runtime\/src\/vm\/(dispatch_fast\/(in_place|builtin|native_rows|array_steps)|inplace|array|string|number|json|collections|object_keys|object_builtins|primitives|builtins|native_rows|native_lengths|intact_fuses|math|date|regexp|typed_array)/, "Builtins"],
  [/^crates\/quench-runtime\/src\/(heap|vm\/gc|vm\/environment|vm\/name_scope)/, "Env, GC and heap memory"],
  [/^crates\/quench-runtime\/src\/(vm\/(object|shape|preshaped|method_cache|field)|typed_ir|staged\/object)/, "Fact store"],
  [/^crates\/quench-runtime\/src\/(vm\/dispatch|vm\/lane|staged\/)/, "Derive"],
  [/^crates\/quench-runtime\/src\/(compile|bytecode|parse)/, "Front end"],
  [/^crates\/quench-node\//, "Builtins"], // Node host moved to Builtins
  [/^crates\/quench-runtime\//, "Derive"],
];
// Symbol-level overrides (file, symbol) -> owner, from the owners' own triage.
const SYMBOL_OWNERS = [
  [/vm\/dispatch_fast\/generator_windows\.rs$/, /./, "Derive"],
  // Call protocol v2 frame fields live in LaneStack but belong to the call protocol (Derive triage).
  [/vm\/dispatch_fast\.rs$/, /^dead:plain_frames$/, "Calls and frames"],
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
// Frame types: JIT-side frames fold in F5 with the JIT threads (coordinator, 15:20 UTC 19 Nov);
// LaneFrame is Derive's protocol_row frame; the rest are the Calls thread's one frame model.
function frameOwner(rel, name) {
  if (/^LaneFrame$/.test(name)) return "Derive";
  if (/vm\/compiled(\/|\.rs$)/.test(rel) || /^(NativeActivation|JsNativeActivation)$/.test(name)) return "u-jit";
  return "Calls and frames";
}
// The item an allow(dead_code) sits on, so the key survives line moves: dead:<name>.
function deadName(s, at, line) {
  const m = /^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const\s+|unsafe\s+|extern\s+"C"\s+)*fn|struct|enum|const|static|mod|type|trait|union|impl(?:<[^>]*>)?)\s+([A-Za-z_][A-Za-z0-9_]*)|^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*[:(,{]/.exec(s.slice(at, at + 400));
  return m ? `dead:${m[1] || m[2]}` : `dead@${line}`;
}
function testRanges(s) {
  const r = [];
  const re = /#\[cfg\((?:(?:any|all)\()?test\b[^\]]*\]/g;
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
  /^crates\/quench-runtime\/src\/staged(\/|\.rs$)/, /^crates\/quench-runtime\/src\/compile\/(ast|emit)/,
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
    // extra.readFrom: the owner whose branch holds this item's fix (JIT frames land on the calls branch).
    if (wants(extra.readFrom || owner)) items.push({ cat, owner, file: rel, line, symbol, detail, branch, key: `${cat}|${rel}|${symbol}` });
  };

  // Opcode predicates: `fn is_x(&self)` on the instruction types.
  const preds = new Set();
  for (const rel of files.filter(f => /src\/(bytecode\.rs|bytecode\/|staged\/operation)/.test(f))) {
    const s = fs.readFileSync(path.join(tree, rel), "utf8");
    for (const m of s.matchAll(/\bfn\s+(is_[a-z0-9_]+)\s*\(\s*&?self\s*\)/g)) preds.add(m[1]);
  }
  // Class predicates the catalog generates (op_catalog.rs, op_classes.rs) are its interface, not opcode names.
  for (const rel of files.filter(f => /src\/bytecode\/(op_catalog|op_classes)\.rs$/.test(f))) {
    const s = fs.readFileSync(path.join(tree, rel), "utf8");
    for (const m of s.matchAll(/\b(is_[a-z0-9_]+)\b/g)) preds.delete(m[1]);
  }
  for (const p of ["is_wide_marker", "is_call_with_eval_flags", "is_relocatable_local_access", "is_field_store", "is_wide", "is_module", "is_shared", "is_super_construct", "is_register_field", "is_operand_field", "is_empty", "is_some", "is_none", "is_ok", "is_err", "is_null", "is_nan", "is_finite", "is_ascii", "is_zero", "is_negative", "is_positive"]) preds.delete(p);

  // Files another crate compiles through #[path] (the stencil build reads runtime rows this way):
  // their items look unused to the runtime crate, so #[allow(dead_code)] there is not a dead path.
  const pathIncluded = new Set();
  for (const rel of files) {
    const src = fs.readFileSync(path.join(tree, rel), "utf8");
    for (const m of src.matchAll(/#\[path\s*=\s*"([^"]+)"\]/g)) {
      const target = path.normalize(path.join(path.dirname(rel), m[1]));
      if (target.split("/")[1] !== rel.split("/")[1]) pathIncluded.add(target);
    }
  }

  // Builtin keys from the Native enum.
  let nativeKeys = [];
  const declaredByTier = { interpreter: 0, "b-jit": 0, "u-jit": 0 };
  const fnIndex = []; // for builtin bodies

  for (const rel of files) {
    const testFile = isTestPath(rel);
    const src = fs.readFileSync(path.join(tree, rel), "utf8");
    if (rel.endsWith("heap/cell.rs")) {
      const list = /\nnatives!\s*\{([\s\S]*?)\n\}/.exec(src); // one natives! list generates the enum
      if (list) nativeKeys = [...strip(list[1]).replace(/:\s*[a-z_]+/g, "").matchAll(/\b([A-Z][A-Za-z0-9]+)\b/g)].map(x => x[1]);
      const m = list ? null : /enum Native\s*\{([\s\S]*?)\n\}/.exec(src);
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
    // Methods of `impl <staged trait> for <stencil machine>` are the stage itself, not rows.
    const stageImpls = [];
    for (const m of s.matchAll(/\bimpl\b([^{;]*?)\bfor\b[^{;]+\{/g)) {
      const head = m[1], trait = (head.match(/([A-Z][A-Za-z0-9_]*)\s*(?:<[^>]*>)?\s*$/) || [])[1];
      if (/staged::/.test(head) || (trait && stagedNames.has(trait)) || /(Stage|Machine[A-Za-z]*)\b/.test(trait || ""))
        stageImpls.push([m.index, matchBrace(s, m.index + m[0].length - 1)]);
    }
    for (const f of fns) {
      if (callsStaged.get(f.name)) continue;
      if (isStencilRows && inRanges(stageImpls, f.at)) continue;
      // A stencil row is a fn the stitcher places: const-generic and returning Out, or named *_row.
      // const fn decoders, slot reads/writes and exit helpers are plumbing.
      const sig = s.slice(f.at, f.open);
      const isRowFn = (/<\s*const\b/.test(sig) && /->\s*Out\b/.test(sig)) || /_row$/.test(f.name);
      if (isStencilRows && (!isRowFn || /\bconst\s+(unsafe\s+)?fn\s*$/.test(s.slice(Math.max(0, f.at - 16), f.at + 2)))) continue;
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
      // The row catalog (row_catalog.rs, and codes.rs that only includes it) is the single source.
      const isCatalog = /row_catalog\.rs$/.test(rel) || (/codes\.rs$/.test(rel) && /row_catalog/.test(src));
      if (!isCatalog && lits.length >= 8 && /^crates\/quench-bjit\/|staged\/(layout|frame)|codes|layout/.test(rel))
        add("hand-table", rel, lineOf(starts, lits[0].index), "literal consts", `${lits.length} numeric constants typed in by hand (offsets/codes); generate from the runtime layout/catalog`);
      for (const m of s.matchAll(/\b(numbered_codes|codes|call_rows)!\s*\{/g)) if (!isCatalog) add("hand-table", rel, lineOf(starts, m.index), `${m[1]}!`, `${m[1]}! hand-lists row names`);
    }

    // H. Rules from principles.md and the project instructions.
    const inRuntime = /^crates\/quench-(runtime|bjit)\//.test(rel);
    const tierCode = /^crates\/quench-bjit\/|vm\/compiled|vm\/dispatch_fast|vm\/dispatch\.rs|vm\/tier_policy/.test(rel);
    // P3: no opcode exclusion tables (arrays, sets, maps or masks; a scalar flag such as a refused bit is not a table).
    if (tierCode) for (const m of s.matchAll(/\b(?:const|static)\s+([A-Z_]*(?:EXCLUD|UNSUPPORTED|REJECT|DENY|REFUSE|NOT_SUPPORTED|DECLINE)[A-Z_]*)\s*:\s*&?\s*(?:\[|&|[A-Za-z_:<]*(?:Set|Map|Table|Vec|Mask))/g))
      if (!inRanges(tests, m.index)) add("exclusion-table", rel, lineOf(starts, m.index), m[1], `${m[1]}: an exclusion table in tier code (principle 3: every op has a row, no exclusion tables)`);
    // P3: one frame model.
    if (inRuntime && !/staged\/frame\.rs$/.test(rel)) for (const m of s.matchAll(/\bstruct\s+([A-Za-z0-9_]*(?:Frame|Activation|Window)[A-Za-z0-9_]*)\b/g))
      if (!inRanges(tests, m.index) && !/(Frames|Words|Plan|Kind|Stats|Builder|Ref|Id|Key|Info)$/.test(m[1])) add("frame-model", rel, lineOf(starts, m.index), m[1], `frame type ${m[1]} (principle 3: one frame model; staged/frame.rs is the layout source)`, { owner: frameOwner(rel, m[1]), readFrom: "Calls and frames" });
    // P2/P3: one entry mechanism (a fn-pointer transmute is a hand-made way into compiled code).
    if (inRuntime && /vm\//.test(rel)) for (const m of s.matchAll(/transmute::<[^>]*extern\s+"C"|transmute\s*\([^)]*\)\s*as\s*(?:unsafe\s+)?extern|as\s+(?:unsafe\s+)?extern\s+"C"\s+fn/g))
      if (!inRanges(tests, m.index)) add("entry-mechanism", rel, lineOf(starts, m.index), `entry@${lineOf(starts, m.index)}`, `a hand-made entry into compiled code (fn-pointer cast); principle 3: one trampoline, one EntryDescriptor`, { owner: /wasm/.test(rel) ? "Wasm" : "u-jit" });
    // P7: facts in one lattice.
    if (inRuntime) for (const m of s.matchAll(/\benum\s+(Rep|Repr|Representation|SlotType|ValueKind|Tagged[A-Za-z]*Role)\b/g))
      if (!inRanges(tests, m.index)) add("duplicate-lattice", rel, lineOf(starts, m.index), m[1], `enum ${m[1]}: another value-representation lattice (principle 7: facts in one lattice)`, { owner: "Fact store" });
    // P8: TS syntax refused instead of run.
    if (/compile\//.test(rel)) for (const m of s.matchAll(/outside the supported subset|unsupported TypeScript|TypeScript[^\n]{0,40}not supported/g))
      add("ts-rejected", rel, lineOf(starts, m.index), `ts@${lineOf(starts, m.index)}`, `TypeScript syntax refused here (principle 8: TS is first-class)`, { owner: "Front end" });
    // P10: optimizations are general, never benchmark-specific.
    if (inRuntime) for (const m of src.matchAll(/\b(richards|deltablue|delta_blue|navier|raytrace|earley|boyer|splay|babylon|nbody|n_body|jetstream|octane|sunspider|crypto_aes|validatorjs|acorn_wtb|babel_wtb)\b/gi))
      if (!inRanges(tests, m.index) && !/^\s*\/\//.test(src.slice(src.lastIndexOf("\n", m.index) + 1, m.index))) add("benchmark-specific", rel, lineOf(starts, m.index), `bench@${lineOf(starts, m.index)}`, `names benchmark "${m[1]}" in engine code (principle 10)`);
    // P3: one specializer driver. Public compile entry points in vm/compiled are candidates to fold.
    if (/^crates\/quench-runtime\/src\/vm\/compiled(\.rs|\/)/.test(rel)) for (const f of fns) {
      const head = s.slice(Math.max(0, f.at - 24), f.at);
      if (/pub(\([^)]*\))?\s+$/.test(head) && /^(compile|specialize|lower|stitch|build)_(body|bodies|region|loop|unit|function|program|osr|ahead|lane|module|method|native|image|entry)\b/.test(f.name))
        add("specializer-driver", rel, lineOf(starts, f.at), f.name, `${f.name}: a separate specializer driver (principle 3: one specializer driver for every tier)`, { owner: "u-jit" });
    }
    // P12: delete dead paths.
    for (const m of s.matchAll(/#!?\[(?:allow|expect)\((?:[a-z_]+, *)*dead_code\b[^\]]*\]/g))
      // A reasoned expect on a repr(C) ABI-layout struct: its fields exist for the C layout, and the build fails if they become live.
      if (/^#\[expect\(dead_code,\s*reason/.test(m[0]) && /#\[repr\(C/.test(s.slice(Math.max(0, m.index - 200), m.index + 300))) continue;
      else if (!inRanges(tests, m.index) && !pathIncluded.has(rel)) add("dead-code", rel, lineOf(starts, m.index), deadName(s, m.index + m[0].length, lineOf(starts, m.index)), `#[allow(dead_code)] keeps a dead path (principle 12)`);
    // One policy home: QUENCH_* switches outside vm/switches.rs.
    if (inRuntime && !/vm\/switches\.rs$/.test(rel)) for (const m of src.matchAll(/env::var(?:_os)?\(\s*"(QUENCH_[A-Z0-9_]+)"/g))
      if (!inRanges(tests, m.index)) add("policy-outside-home", rel, lineOf(starts, m.index), m[1], `${m[1]} read outside vm/switches.rs (one policy home)`, { owner: "Compile pipeline" });

    // E. Per-call re-checks in call, return and entry paths.
    if (/vm\/dispatch_fast|vm\/compiled\/(entry|lane_body|exit|osr_lane|frameless)|crates\/quench-bjit\/.*call/.test(rel)) {
      for (const f of fns) {
        if (!/call|return|enter|entry|activation/.test(f.name)) continue;
        const ls = f.body.split("\n");
        const hits = [];
        ls.forEach((l, k) => { const m = RECHECK.exec(l); if (m) hits.push(`${lineOf(starts, f.open) + k}:${m[0].trim()}`); });
        if (hits.length) add("per-call-recheck", rel, lineOf(starts, f.at), f.name, `${f.name} re-checks per call: ${hits.slice(0, 6).join(", ")}${hits.length > 6 ? ", …" : ""} (a link or target should prove these once)`, { owner: CALL_PROOFS });
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
    const keys = [full]; if (tail !== full && (tail.includes("_") || tail.length >= 7)) keys.push(tail);
    const re = new RegExp(`^(?:[a-z0-9]+_)*(?:${keys.join("|")})(?:_[a-z0-9]+)*$`);
    const bodies = fnIndex.filter(f => re.test(f.name) && f.lines >= 4 && !BODY_SKIP_FILE.test(f.rel) && !BODY_SKIP_NAME.test(f.name) && !/^\{\s*[A-Za-z_:.<>]+\([^;]*\)\s*\}$/s.test(f.body.trim()));
    // A plain_* fast prefix that declines into the generic native is one body with it.
    for (let i = bodies.length - 1; i >= 0; i--) if (/^plain_/.test(bodies[i].name) && bodies.some(b => b !== bodies[i])) bodies.splice(i, 1);
    // Another builtin's body (TypedArray.*, Wasm array.*) only shares the method name.
    for (let i = bodies.length - 1; i >= 0; i--) if (/^(construct_)?(typed_array|wasm)_/.test(bodies[i].name) && !/^(typed_array|wasm)/.test(full)) bodies.splice(i, 1);
    // Bodies that run the builtin's staged row are tiers of one semantics, not copies.
    for (let i = bodies.length - 1; i >= 0; i--) if (/\bstaged::/.test(bodies[i].body)) bodies.splice(i, 1);
    // A builtin with a staged row: the row and every body that calls it by name are one semantics.
    const rowNames = bodies.filter(b => /\/staged\//.test(b.rel)).map(b => b.name);
    if (rowNames.length) {
      const callsRow = new RegExp(`\\b(${rowNames.join("|")})\\b`);
      for (let i = bodies.length - 1; i >= 0; i--) if (/\/staged\//.test(bodies[i].rel) || callsRow.test(bodies[i].body)) bodies.splice(i, 1);
    }
    // Stencils and u-jit emitters are tier code: the stencil-not-row and clif-not-row rules judge them.
    for (let i = bodies.length - 1; i >= 0; i--) if (/^crates\/quench-bjit\/|vm\/compiled\//.test(bodies[i].rel)) bodies.splice(i, 1);
    // Helpers and embedding wrappers are layers, not bodies.
    for (let i = bodies.length - 1; i >= 0; i--) if (/_(helper|embedding)$/.test(bodies[i].name)) bodies.splice(i, 1);
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
  const TS_SOURCE = { interpreter: "Fact store", "b-jit": "b-jit", "u-jit": "Fact store" }; // u-jit's declared.rs lives on the fact-store branch
  for (const [t, n] of Object.entries(declaredByTier))
      if (n === 0 && wants(TS_SOURCE[t])) items.push({ cat: "ts-types-unused", owner: "Fact store", file: `(tier ${t})`, line: 0, symbol: t, branch, key: `ts-types-unused|${t}`,
        detail: `${t} reads no declared TS type (no reference to DeclaredTypes/declared facts in its files)` });
  return items;
}

// ---------- gate budgets ----------
// Budgets only shrink: compare a branch's budget files with v2-merged's.
function budgets(baseDir, dir, branch, owner) {
  const items = [];
  const rel = "crates/quench-tier-corpus/";
  for (const f of ["ir_budget.txt", "ir_floor.txt", "tier_inversions.txt"]) {
    let a, b;
    try { a = fs.readFileSync(path.join(baseDir, rel, f), "utf8"); b = fs.readFileSync(path.join(dir, rel, f), "utf8"); } catch { continue; }
    const rows = t => new Map(t.split("\n").filter(l => l && !l.startsWith("#")).map(l => [l.trim().split(/\s+/).filter(x => !/^-?[\d.]+$/.test(x)).join(" "), l.trim()]));
    const A = rows(a), B = rows(b);
    for (const [k, line] of B) {
      const old = A.get(k);
      if (old == null) { if (f === "tier_inversions.txt") items.push({ cat: "budget-grew", owner, file: rel + f, line: 0, symbol: k, branch, key: `budget-grew|${f}|${k}`, detail: `new inversion "${k}" added (shrink-only debt)` }); continue; }
      const na = old.match(/-?\d+(\.\d+)?/g) || [], nb = line.match(/-?\d+(\.\d+)?/g) || [];
      if (na.length === nb.length && nb.some((x, i) => +x > +na[i] + 1e-9)) items.push({ cat: "budget-grew", owner, file: rel + f, line: 0, symbol: k, branch, key: `budget-grew|${f}|${k}`, detail: `${k}: ${old} -> ${line} (budgets only shrink)` });
    }
  }
  return items;
}
function inversions(file) {
  const items = [];
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    const m = /^([a-z0-9_]+)\s+(default|aot)\s*$/.exec(line);
    if (m) items.push({ cat: "tier-inversion", owner: m[2], file: "tier_inversions.txt", line: 0, symbol: `${m[1]} ${m[2]}`, branch: "v2-merged",
      key: `tier-inversion|${m[1]}|${m[2]}`, detail: `${m[1]}: ${m[2]} runs it in more instructions than the mode above it (principle 5: tier order by design)` });
  }
  return items;
}
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
  // Rows whose fix lives on another branch than the gate's named owner (owners' triage).
  const GATE_OWNER = { bi_set_has: "Builtins", bi_map_get: "Builtins", arr_includes: "Builtins", arr_indexof: "Builtins" };
  for (const m of txt.matchAll(/^([a-z0-9_]+): (default|aot) ([\d.]+) Ir\/iter, ceiling ([\d.]+) \(budget ([\d.]+)\) -> (.+)$/gm))
    items.push({ cat: "budget-over-c6", owner: GATE_OWNER[m[1]] || m[2], file: `ir-budget ${tag}`, line: 0, symbol: `${m[1]} ${m[2]}`, branch: "v2-merged", key: `budget-over-c6|${m[1]}|${m[2]}`,
      detail: `${m[1]} ${m[2]}: ${m[3]} Ir/iter vs budget ${m[5]} (${(+m[3] / +m[5]).toFixed(1)}x)` });
  for (const m of txt.matchAll(/^([a-z0-9_]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)x\s+over\s+(.*)$/gm)) {
    const first = (m[7].match(/\d+\s+([A-Za-z_:<>]+)/) || [])[1] || "";
    let o = ownerFor[m[1]];
    if (!o) o = /call|return|native/.test(first) ? "Cut calls and frames to C" : /row<(50|89)|field/.test(first) ? "Unify fact store and object model" : /Vm::|json|string|array|regexp/i.test(first) ? "Cut builtin library code to C" : "Derive every tier from one semantics";
    items.push({ cat: "budget-over-floor", owner: GATE_OWNER[m[1]] || own(o), file: `ir-budget ${tag}`, line: 0, symbol: m[1], branch: "v2-merged", key: `budget-over-floor|${m[1]}`,
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
  "tier-inversion": "Tier inversions (gate debt)", "exclusion-table": "Opcode exclusion tables", "frame-model": "Frame types beside the one frame model",
  "entry-mechanism": "Hand-made entries into compiled code", "duplicate-lattice": "Extra value lattices", "ts-rejected": "TypeScript syntax refused",
  "benchmark-specific": "Benchmark-specific code", "specializer-driver": "Specializer drivers beside the one driver", "budget-grew": "Budgets that grew (must only shrink)", "dead-code": "Dead code kept alive", "policy-outside-home": "Switches outside the policy home",
};
const MEASURED = new Set(["budget-over-c6", "budget-over-floor", "tier-inversion"]);
// Exemptions: tools/vision-ledger-exemptions.txt, one per line: category<TAB>file regex<TAB>symbol regex<TAB>reason.
function loadExemptions() {
  const f = path.join(path.dirname(new URL(import.meta.url).pathname), "vision-ledger-exemptions.txt");
  let txt = ""; try { txt = fs.readFileSync(f, "utf8"); } catch { return []; }
  return txt.split("\n").filter(l => l.trim() && !l.startsWith("#")).map(l => { const [cat, file, sym, reason] = l.split("\t"); return { cat, file: new RegExp(file), sym: new RegExp(sym || "."), reason: reason || "" }; });
}
// An exemption whose reason starts "NEEDS BUILD" keeps the item open but moves it to the stabilization bucket.
const kindOf = it => MEASURED.has(it.cat) || it.needsBuild ? "measured" : "code";
// plan-2026-11-19b (stabilization team): the scanner's path owners -> the thread that owns them now.
const TEAM = {
  "Derive": "Rows and b-jit", "b-jit": "Rows and b-jit",
  "Calls and frames": "Calls", "Prove call checks once": "Calls",
  "Fact store": "Objects", "Builtins": "Library", "Front end": "Startup",
  "Env, GC and heap memory": "Async and memory", "No-jit": "Async and memory",
  "Compile pipeline": "u-jit",
};
function combine(outDir, statePath, inputs) {
  const all = [];
  for (const f of inputs) all.push(...JSON.parse(fs.readFileSync(f, "utf8")));
  const byKey = new Map();
  for (const it of all) if (!byKey.has(it.key)) byKey.set(it.key, it);
  const ex = loadExemptions();
  const exempted = [];
  for (const it of byKey.values()) it.owner = TEAM[it.owner] || it.owner;
  const items = [...byKey.values()].filter(it => {
    const e = ex.find(e => (e.cat === "*" || e.cat === it.cat) && e.file.test(it.file) && e.sym.test(it.symbol));
    if (e && /^NEEDS BUILD/.test(e.reason)) { it.needsBuild = true; it.detail += ` [${e.reason}]`; return true; }
    if (e) { exempted.push({ ...it, reason: e.reason }); return false; }
    return true;
  });
  let prev = { keys: [] };
  try { prev = JSON.parse(fs.readFileSync(statePath, "utf8")); } catch {}
  const prevKeys = new Set(prev.keys), nowKeys = new Set(items.map(i => i.key));
  for (const it of items) it.status = prevKeys.has(it.key) ? "open" : (prev.keys.length ? "new" : "open");
  const closed = prev.keys.filter(k => !nowKeys.has(k));
  // An owner that just closed its last item stays listed (code 0, was N) once, so its count is seen to reach zero.
  const owners = [...new Set([...items.map(i => i.owner), ...Object.keys(prev.code || {}).filter(o => prev.code[o] > 0)])].sort();
  const cats = Object.keys(CAT_TITLE);
  const stamp = new Date().toISOString().replace(/\.\d+Z$/, "Z");
  const weight = it => it.cat === "opcode-named" ? (+(/^(\d+)/.exec(it.detail) || [0, 1])[1]) : 1;

  let md = `# Vision ledger\n\nGenerated ${stamp} by tools/vision-ledger.mjs (reading only, no build). Branches: ${[...new Set(items.map(i => i.branch))].join(", ")}.\n\n`;
  const codeN = items.filter(i => kindOf(i) === "code").length, measN = items.length - codeN;
  md += `**Code gaps: ${codeN}** (closable now by editing; must reach zero). **Measured gaps: ${measN}** (gate rows and items marked NEEDS BUILD; close at stabilization).\n\n`;
  md += `**${items.length} open items** (${items.filter(i => i.status === "new").length} new since the last run, ${closed.length} closed). Opcode names counted individually: ${items.filter(i => i.cat === "opcode-named").reduce((a, i) => a + weight(i), 0)}.\n\n`;
  md += `Each owner's items come from that owner's own pushed branch tip; gate items from the newest v2-merged ir-budget report. "Calls a staged row" is decided by name, so counts are an upper bound: triage before deleting.\n\n`;
  md += `| owner | ${cats.map(c => c.replace(/-/g, " ")).join(" | ")} | total |\n|---|${cats.map(() => "--:").join("|")}|--:|\n`;
  for (const o of owners) {
    const mine = items.filter(i => i.owner === o);
    md += `| ${o} | ${cats.map(c => mine.filter(i => i.cat === c).length || "").join(" | ")} | ${mine.length} |\n`;
  }
  md += `| **all** | ${cats.map(c => items.filter(i => i.cat === c).length || "").join(" | ")} | **${items.length}** |\n\n`;
  for (const o of owners) {
    const mineAll = items.filter(i => i.owner === o);
    md += `## ${o} (code ${mineAll.filter(i => kindOf(i) === "code").length}, measured ${mineAll.filter(i => kindOf(i) === "measured").length})\n\n`;
    for (const c of [...cats.filter(c => !MEASURED.has(c)), ...cats.filter(c => MEASURED.has(c))]) {
      const mine = items.filter(i => i.owner === o && i.cat === c);
      if (!mine.length) continue;
      md += `### ${CAT_TITLE[c]} (${mine.length})\n\n`;
      for (const it of mine) md += `- ${it.status === "new" ? "**NEW** " : ""}\`${it.file}${it.line ? ":" + it.line : ""}\` ${it.detail} _(${it.branch})_\n`;
      md += "\n";
    }
  }
  if (exempted.length) md += `## Exempted (${exempted.length}), with reasons from tools/vision-ledger-exemptions.txt\n\n${exempted.map(it => `- \`${it.file}${it.line ? ":" + it.line : ""}\` ${it.cat} ${it.symbol}: ${it.reason}`).join("\n")}\n\n`;
  if (closed.length) md += `## Closed since the last run (${closed.length})\n\n${closed.map(k => `- ${k}`).join("\n")}\n`;
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "ledger.md"), md);

  const prevCode = prev.code || {};
  const codeBy = {};
  let delta = `Vision ledger ${stamp}: code gaps ${codeN}, measured gaps ${measN}; ${items.filter(i => i.status === "new").length} new, ${closed.length} closed.\n`;
  fs.rmSync(path.join(outDir, "owners"), { recursive: true, force: true });
  fs.mkdirSync(path.join(outDir, "owners"), { recursive: true });
  for (const o of owners) {
    const mine = items.filter(i => i.owner === o);
    const code = mine.filter(i => kindOf(i) === "code"), meas = mine.filter(i => kindOf(i) === "measured");
    codeBy[o] = code.length;
    const nw = mine.filter(i => i.status === "new");
    const stuck = prevCode[o] != null && code.length > 0 && code.length >= prevCode[o];
    delta += `- ${o}: code ${code.length}${prevCode[o] != null ? ` (was ${prevCode[o]})` : ""}${stuck ? " NOT DROPPING" : ""}, measured ${meas.length}${nw.length ? `, ${nw.length} new` : ""} (${cats.filter(c => mine.some(i => i.cat === c)).map(c => `${c} ${mine.filter(i => i.cat === c).length}`).join(", ")})\n`;
    let om = `# ${o}: open vision gaps (${stamp})\n\nCode gaps ${code.length}${prevCode[o] != null ? ` (previous pass ${prevCode[o]})` : ""}: close now by editing; must reach zero. Measured gaps ${meas.length}: gate rows, close at stabilization.\nRead from: ${[...new Set(mine.map(i => i.branch))].join(", ")}. Counts are by name: triage before deleting, and report false positives to the "Track the full vision gap" thread.\n\n`;
    for (const [title, list] of [["Code gaps", code], ["Measured gaps", meas]]) {
      if (!list.length) continue;
      om += `## ${title}\n\n`;
      for (const c of cats) { const l = list.filter(i => i.cat === c); if (!l.length) continue; om += `### ${CAT_TITLE[c]} (${l.length})\n\n` + l.map(it => `- ${it.status === "new" ? "**NEW** " : ""}\`${it.file}${it.line ? ":" + it.line : ""}\` ${it.detail}`).join("\n") + "\n\n"; }
    }
    fs.writeFileSync(path.join(outDir, "owners", o.replace(/[^A-Za-z0-9-]+/g, "_") + ".md"), om);
  }
  fs.writeFileSync(path.join(outDir, "delta.md"), delta);
  fs.writeFileSync(statePath, JSON.stringify({ stamp, keys: [...nowKeys], code: codeBy }));
  process.stdout.write(delta);
  // Gate step: VL_GATE=1 fails (exit 3) when any code gap is open.
  if (process.env.VL_GATE === "1" && codeN > 0) process.exit(3);
}

// ---------- main ----------
const [cmd, ...args] = process.argv.slice(2);
if (cmd === "scan") {
  const [tree, branch, out, ...rest] = args;
  const owners = []; for (let i = 0; i < rest.length; i++) if (rest[i] === "--owner") owners.push(rest[++i]);
  const items = scan(path.resolve(tree), branch, owners);
  fs.writeFileSync(out, JSON.stringify(items));
  process.stdout.write(`${branch}: ${items.length} items\n`);
} else if (cmd === "budgets") {
  const items = budgets(args[0], args[1], args[2], args[3]);
  fs.writeFileSync(args[4], JSON.stringify(items));
  process.stdout.write(`budgets ${args[2]}: ${items.length} items\n`);
} else if (cmd === "inversions") {
  const items = inversions(args[0]);
  fs.writeFileSync(args[1], JSON.stringify(items));
  process.stdout.write(`inversions: ${items.length} items\n`);
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
