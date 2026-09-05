#!/usr/bin/env node
// Aggregates QUENCH_EXEC_TRACE handler/opcode counters across an entire
// corpus of fixtures into one ranked ledger: which check/handler category
// accounts for what share of total retired-instruction-proxy counts,
// across everything measured -- not just one profiled run.
//
// This exists because a single-fixture profile answers "what's hot here,"
// not "what fraction of everything we care about does this represent."
// See docs/v8_v7.md, "How to make it a mechanical tool" / the systematic
// category-reduction process, for the methodology this implements.
//
// Usage:
//   node tools/instruction-category-ledger.mjs \
//     --engine target/exec-trace/release/quench-node \
//     --corpus quench-bench/micros \
//     --corpus quench-bench/deegen-curriculum \
//     [--fixture path/to/one.js ...] \
//     --out target/instruction-category-ledger.json \
//     [--timeout-ms 30000] [--compare target/prior-ledger.json] \
//     [--synthetic-before target/synth-before.json --synthetic-after target/synth-after.json] \
//     [--facts-log target/instruction-facts.jsonl]
//
// The engine MUST be built with --features execution-trace (a diagnostic
// artifact per performance-lanes.md; never the scored one). Each fixture is
// run once; its `QUENCH_EXEC_TRACE {...}` stderr line is parsed and summed
// into a running total keyed by category and by individual opcode/handler
// name within that category. v8-v7 fixtures are not standalone scripts (they
// load through base.js/run.js); pass pre-wrapped copies via repeated
// --fixture flags rather than a --corpus directory, so this tool never has
// to guess at harness wiring it hasn't verified.
//
// The category/name aggregation is fully generic over the snapshot's
// top-level JSON keys (see aggregate() below) -- if execution_trace.rs is
// later extended to key counters by ShapeId/FactSiteId/CodeId (the identity
// types already defined in identity.rs/facts.rs) instead of just opcode
// name, this tool picks up the finer-grained categories automatically, with
// no code change here. Add a mechanism_hint for any new category name to
// CATEGORY_MECHANISM_HINTS below when that lands.
//
// --facts-log appends one line per (fixture, category, name) to a JSONL
// file, in the Fact{benchmark, build, machine, vm_unit, metric, value}
// shape -- an append-only raw record, disposable under target/ like every
// other generated report, that the summary JSON/console output is derived
// from. Keep the summary (the --out JSON, and its top rows copied into
// architecture-evidence.md) as the durable record; the facts log is for
// re-deriving a different view later without re-running fixtures.
//
// --synthetic-before/--synthetic-after operationalize the anti-cheat
// generalization check (docs/v8_v7.md, "Healthy optimization vs. benchmark
// cheating"): run the SAME candidate change on a synthetic clone of the
// motivating fixture's shape, produce a ledger before and after, and pass
// both here alongside --compare (the real corpus's before-ledger). The tool
// flags any category whose real-corpus delta and synthetic-clone delta
// diverge in sign or magnitude -- the mechanical version of "the fix only
// helped the literal fixture" that a human reviewer would otherwise have to
// notice by eye.

import { spawnSync } from 'node:child_process';
import { readdirSync, statSync, readFileSync, writeFileSync, appendFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';

function parseArgs(argv) {
  const args = {
    corpus: [], fixture: [], engine: null,
    out: 'target/instruction-category-ledger.json',
    timeoutMs: 30000, compare: null,
    syntheticBefore: null, syntheticAfter: null,
    factsLog: null,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--engine') args.engine = argv[++i];
    else if (a === '--corpus') args.corpus.push(argv[++i]);
    else if (a === '--fixture') args.fixture.push(argv[++i]);
    else if (a === '--out') args.out = argv[++i];
    else if (a === '--timeout-ms') args.timeoutMs = Number(argv[++i]);
    else if (a === '--compare') args.compare = argv[++i];
    else if (a === '--synthetic-before') args.syntheticBefore = argv[++i];
    else if (a === '--synthetic-after') args.syntheticAfter = argv[++i];
    else if (a === '--facts-log') args.factsLog = argv[++i];
    else throw new Error(`unknown argument: ${a}`);
  }
  if (!args.engine) throw new Error('--engine <path to execution-trace build> is required');
  if ((args.syntheticBefore && !args.syntheticAfter) || (!args.syntheticBefore && args.syntheticAfter)) {
    throw new Error('--synthetic-before and --synthetic-after must be passed together');
  }
  return args;
}

function buildMetadata(engine) {
  const git = spawnSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' });
  const dirty = spawnSync('git', ['status', '--porcelain'], { encoding: 'utf8' });
  return {
    engine_path: engine,
    engine_mtime: statSync(engine).mtime.toISOString(),
    git_commit: git.status === 0 ? git.stdout.trim() : null,
    git_dirty: dirty.status === 0 ? dirty.stdout.trim().length > 0 : null,
    node_version: process.version,
    platform: `${process.platform}/${process.arch}`,
    collected_at: new Date().toISOString(),
  };
}

function collectFixtures(args) {
  const files = [...args.fixture];
  for (const dir of args.corpus) {
    for (const name of readdirSync(dir).sort()) {
      const full = path.join(dir, name);
      if (name.endsWith('.js') && statSync(full).isFile()) files.push(full);
    }
  }
  return files;
}

function runOne(engine, fixture, timeoutMs) {
  const result = spawnSync(engine, [fixture], { timeout: timeoutMs, encoding: 'utf8' });
  const stderr = result.stderr ?? '';
  const line = stderr.split('\n').find((l) => l.startsWith('QUENCH_EXEC_TRACE '));
  if (!line) {
    return { fixture, ok: false, reason: result.error ? String(result.error) : 'no QUENCH_EXEC_TRACE line (build with --features execution-trace?)' };
  }
  try {
    return { fixture, ok: true, snapshot: JSON.parse(line.slice('QUENCH_EXEC_TRACE '.length)) };
  } catch (err) {
    return { fixture, ok: false, reason: `unparseable snapshot: ${err.message}` };
  }
}

// Maps snapshot top-level keys to a stable category label and a candidate
// class of general mechanism -- this is the "what kind of fix reduces this
// whole category" lookup the ledger surfaces per-row, so ranking a category
// highest immediately suggests where to look, without re-deriving it by hand
// every time.
const CATEGORY_MECHANISM_HINTS = {
  compact: 'inline caching / quickening (per-opcode fast path)',
  leaf_compact: 'inline caching / quickening (per-opcode fast path)',
  slow: 'generalize the fast path so fewer ops fall through to this handler',
  binary: 'type-specialized quickening for this binary op',
  constant: 'constant-site caching / bytecode specialization',
  events: 'diagnostic counters -- not a direct instruction-count category',
  // Not emitted by execution_trace.rs today (checked: counters.quickening is
  // keyed by opcode name for IC hits/misses, not by stencil/region -- there
  // is no per-region native-vs-fallback counter yet). Once a "stencil"
  // category is added there (region/leaf id -> {native_hits, fallback_hits,
  // bytes}), this tool picks it up automatically via the generic aggregate()
  // below with zero code changes; these hints just need to already exist.
  stencil: 'region composition / ABI fix (see docs/v8_v7.md RayTrace/NavierStokes/Crypto section)',
  quickening: 'IC hit/miss ratio -- a low hit rate here means the cache is thrashing, not that the op is expensive',
};

// Some categories (quickening today; a future "stencil" category almost
// certainly) carry a {hits, misses}-shaped object per name instead of a bare
// count. Treat hits+misses as the name's weight for ranking purposes, but
// keep the hit rate alongside it -- a region/site with a huge count and a
// LOW hit rate is a different finding (the mechanism is thrashing, not that
// the op itself is expensive) than one with a high hit rate, and collapsing
// both into one number would hide that.
function weighAndAnnotate(value) {
  if (typeof value === 'number') return { count: value, extra: null };
  if (value && typeof value === 'object' && ('hits' in value || 'misses' in value)) {
    const hits = Number(value.hits ?? 0);
    const misses = Number(value.misses ?? 0);
    const total = hits + misses;
    return { count: total, extra: { hits, misses, hit_rate: total > 0 ? hits / total : null } };
  }
  return { count: 0, extra: null };
}

function aggregate(runs) {
  const categories = {};
  let grandTotal = 0;
  for (const run of runs) {
    if (!run.ok) continue;
    for (const [category, obj] of Object.entries(run.snapshot)) {
      if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) continue;
      categories[category] ??= { total: 0, byName: {}, byNameExtra: {} };
      for (const [name, rawValue] of Object.entries(obj)) {
        const { count, extra } = weighAndAnnotate(rawValue);
        categories[category].total += count;
        categories[category].byName[name] = (categories[category].byName[name] ?? 0) + count;
        if (extra) {
          const prior = categories[category].byNameExtra[name] ?? { hits: 0, misses: 0 };
          categories[category].byNameExtra[name] = { hits: prior.hits + extra.hits, misses: prior.misses + extra.misses };
        }
        grandTotal += count;
      }
    }
  }
  const ranked = Object.entries(categories)
    .map(([category, { total, byName, byNameExtra }]) => ({
      category,
      total,
      share: grandTotal > 0 ? total / grandTotal : 0,
      mechanism_hint: CATEGORY_MECHANISM_HINTS[category] ?? 'inspect by name below before assuming a mechanism',
      top_names: Object.entries(byName)
        .sort((a, b) => b[1] - a[1])
        .slice(0, 10)
        .map(([name, count]) => {
          const extra = byNameExtra[name];
          return {
            name,
            count,
            share_of_category: total > 0 ? count / total : 0,
            ...(extra ? { hits: extra.hits, misses: extra.misses, hit_rate: extra.hits + extra.misses > 0 ? extra.hits / (extra.hits + extra.misses) : null } : {}),
          };
        }),
    }))
    .sort((a, b) => b.total - a.total);
  return { grand_total: grandTotal, categories: ranked };
}

function writeFactsLog(factsLogPath, runs, metadata) {
  const lines = [];
  for (const run of runs) {
    if (!run.ok) continue;
    for (const [category, obj] of Object.entries(run.snapshot)) {
      if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) continue;
      for (const [name, rawValue] of Object.entries(obj)) {
        const { count, extra } = weighAndAnnotate(rawValue);
        lines.push(JSON.stringify({
          benchmark: run.fixture,
          build: metadata.git_commit,
          machine: metadata.platform,
          vm_unit: `${category}.${name}`,
          metric: extra ? 'hits_misses' : 'count',
          value: extra ? { hits: extra.hits, misses: extra.misses } : count,
          collected_at: metadata.collected_at,
        }));
      }
    }
  }
  mkdirSync(path.dirname(factsLogPath), { recursive: true });
  appendFileSync(factsLogPath, lines.map((l) => l + '\n').join(''));
  return lines.length;
}

function compareSyntheticGeneralization(realBefore, realAfter, synthBefore, synthAfter) {
  const deltaByCategory = (before, after) => {
    const beforeByCategory = Object.fromEntries(before.categories.map((c) => [c.category, c.share]));
    return Object.fromEntries(after.categories.map((c) => [c.category, c.share - (beforeByCategory[c.category] ?? 0)]));
  };
  const realDelta = deltaByCategory(realBefore, realAfter);
  const synthDelta = deltaByCategory(synthBefore, synthAfter);
  const allCategories = new Set([...Object.keys(realDelta), ...Object.keys(synthDelta)]);
  const rows = [];
  for (const category of allCategories) {
    const real = realDelta[category] ?? 0;
    const synth = synthDelta[category] ?? 0;
    // Flag a mismatch when the real corpus moved meaningfully (>0.5pp) but
    // the synthetic clone didn't move comparably (opposite sign, or less
    // than a third of the real movement) -- the mechanical form of "this
    // only helped the literal fixture."
    const realMoved = Math.abs(real) > 0.005;
    const sameDirection = Math.sign(real) === Math.sign(synth);
    const comparableMagnitude = Math.abs(synth) >= Math.abs(real) / 3;
    const mismatch = realMoved && (!sameDirection || !comparableMagnitude);
    rows.push({ category, real_delta_pp: real * 100, synthetic_delta_pp: synth * 100, generalization_mismatch: mismatch });
  }
  return rows.sort((a, b) => Math.abs(b.real_delta_pp) - Math.abs(a.real_delta_pp));
}

function compareLedgers(prior, current) {
  const priorByCategory = Object.fromEntries(prior.categories.map((c) => [c.category, c]));
  return current.categories.map((c) => {
    const before = priorByCategory[c.category];
    return {
      category: c.category,
      share_before: before?.share ?? null,
      share_after: c.share,
      delta: before ? c.share - before.share : null,
    };
  });
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const fixtures = collectFixtures(args);
  if (fixtures.length === 0) throw new Error('no fixtures found -- pass --corpus <dir> and/or --fixture <file>');

  const metadata = buildMetadata(args.engine);
  const runs = fixtures.map((f) => runOne(args.engine, f, args.timeoutMs));
  const failed = runs.filter((r) => !r.ok);
  const ledger = aggregate(runs);

  const report = {
    schema: 2,
    engine: args.engine,
    metadata,
    fixture_count: fixtures.length,
    failed_fixtures: failed.map((f) => ({ fixture: f.fixture, reason: f.reason })),
    ...ledger,
  };

  if (args.compare) {
    const prior = JSON.parse(readFileSync(args.compare, 'utf8'));
    report.comparison = compareLedgers(prior, ledger);
  }

  if (args.syntheticBefore && args.syntheticAfter) {
    if (!args.compare) throw new Error('--synthetic-before/--synthetic-after require --compare (the real corpus before-ledger) to compare against');
    const realBefore = JSON.parse(readFileSync(args.compare, 'utf8'));
    const synthBefore = JSON.parse(readFileSync(args.syntheticBefore, 'utf8'));
    const synthAfter = JSON.parse(readFileSync(args.syntheticAfter, 'utf8'));
    report.generalization_check = compareSyntheticGeneralization(realBefore, ledger, synthBefore, synthAfter);
  }

  if (args.factsLog) {
    const count = writeFactsLog(args.factsLog, runs, metadata);
    report.facts_appended = count;
  }

  mkdirSync(path.dirname(args.out), { recursive: true });
  writeFileSync(args.out, JSON.stringify(report, null, 2));

  console.log(`Ran ${fixtures.length} fixtures (${failed.length} failed to emit a trace); grand total ${ledger.grand_total} counted events.`);
  console.log('Top categories by share of total:');
  for (const c of ledger.categories.slice(0, 10)) {
    console.log(`  ${c.category}: ${(c.share * 100).toFixed(2)}%  (${c.total} events)  -> ${c.mechanism_hint}`);
    for (const n of c.top_names.slice(0, 3)) {
      console.log(`      ${n.name}: ${(n.share_of_category * 100).toFixed(1)}% of category`);
    }
  }
  if (report.comparison) {
    console.log('\nComparison vs --compare baseline:');
    for (const c of report.comparison) {
      if (c.delta === null) continue;
      console.log(`  ${c.category}: ${(c.share_before * 100).toFixed(2)}% -> ${(c.share_after * 100).toFixed(2)}%  (delta ${(c.delta * 100).toFixed(2)}pp)`);
    }
  }
  if (report.generalization_check) {
    console.log('\nGeneralization check (real corpus vs synthetic clone):');
    let anyMismatch = false;
    for (const row of report.generalization_check) {
      const flag = row.generalization_mismatch ? '  <-- MISMATCH: does not generalize' : '';
      if (row.generalization_mismatch) anyMismatch = true;
      console.log(`  ${row.category}: real ${row.real_delta_pp.toFixed(2)}pp, synthetic ${row.synthetic_delta_pp.toFixed(2)}pp${flag}`);
    }
    if (anyMismatch) {
      console.log('  WARNING: at least one category improved on the real corpus without a comparable synthetic-clone improvement.');
      console.log('  Per docs/v8_v7.md anti-cheat discipline, do not claim this change as general until resolved.');
    }
  }
  if (report.facts_appended != null) {
    console.log(`\nAppended ${report.facts_appended} facts to ${args.factsLog}`);
  }
  console.log(`\nFull report written to ${args.out}`);
}

main();
