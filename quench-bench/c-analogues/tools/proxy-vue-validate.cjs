#!/usr/bin/env node
// Validation twin for the proxy-vue C analogue (quench-bench/c-analogues/proxy-vue.c).
//
// Runs the JetStream proxy-vue row under node 22 with an instrumented copy of the Vue bundle
// (proxy/vue-bundle.js is read, never modified) and prints, per iteration, the counts of work that
// the C analogue must reproduce, plus the FNV-1a hash of the final store render:
//
//   track    effective track() calls: get/has/iterate reads made while an effect is tracking
//   trigger  trigger() calls that found a dependency map for the target
//   sched    effects scheduled or run by trigger() (Ke in the bundle)
//   run      active effect runs (ReactiveEffect.run)
//   flush    queued jobs run by the scheduler flush
//   cb       watch callbacks called
//   len      length of the final render text
//   hash     FNV-1a 32 of the final render text
//
// usage: node proxy-vue-validate.cjs [jetstream-root] [--date YYYY-MM-DD] [--c]
//   --date  pins the clock: list names are Date.toDateString() of this day. Default: today.
//   --c     print the rows as the C initializer table that proxy-vue.c checks against.
//
// The per-iteration Math.random sequence is JetStream's seeded generator (deterministic=true in the
// row header), reset before each iteration as the composed row does.
"use strict";
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const args = process.argv.slice(2);
let root = "/home/user/quench/quench-bench/jetstream";
let pinnedDate = null;
let emitC = false;
for (let i = 0; i < args.length; i++) {
    if (args[i] === "--date") pinnedDate = args[++i];
    else if (args[i] === "--c") emitC = true;
    else root = args[i];
}
const ITERATIONS = 120;

function instrumentBundle(text) {
    const edits = [
        ["function d(n,e,r){if($e&&Me){", "function d(n,e,r){if($e&&Me){__vc.track++;"],
        ["function We(r,e,t,o){const s=Fe.get(r);if(s){", "function We(r,e,t,o){const s=Fe.get(r);if(s){__vc.trigger++;"],
        ["Ke(e){e===Me&&!e.allowRecurse||(e.scheduler?e.scheduler():e.run())}",
         "Ke(e){e===Me&&!e.allowRecurse||(__vc.sched++,e.scheduler?e.scheduler():e.run())}"],
        ["run(){if(!this.active)return this.fn();let e=Me,t=$e;",
         "run(){if(!this.active)return this.fn();__vc.run++;let e=Me,t=$e;"],
        ["e&&!1!==e.active&&nn(e,null,14)", "e&&!1!==e.active&&(__vc.flush++,nn(e,null,14))"],
        ["re(t,s,3,[e,p===Dn?void 0:a&&p[0]===Dn?[]:p,u])",
         "(__vc.cb++,re(t,s,3,[e,p===Dn?void 0:a&&p[0]===Dn?[]:p,u]))"],
    ];
    let out = text;
    for (const [from, to] of edits) {
        const n = out.split(from).length - 1;
        if (n !== 1) throw new Error(`instrumentation anchor found ${n} times: ${from.slice(0, 50)}`);
        out = out.replace(from, () => to);
    }
    return out;
}

function instrumentBenchmark(text) {
    const from = "await test(store, fn => { fn(); });";
    if (text.split(from).length !== 2) throw new Error("benchmark anchor not unique");
    return text.replace(from, from + " __vc.hash = store.render();");
}

function fnv1a(str) {
    let h = 0x811c9dc5;
    for (let i = 0; i < str.length; i++) {
        h ^= str.charCodeAt(i);
        h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h >>> 0;
}

const proxyDir = path.join(root, "proxy");
const common = fs.readFileSync(path.join(proxyDir, "common.js"), "utf8");
const bundle = instrumentBundle(fs.readFileSync(path.join(proxyDir, "vue-bundle.js"), "utf8"));
const benchmark = instrumentBenchmark(fs.readFileSync(path.join(proxyDir, "vue-benchmark.js"), "utf8"));

// The seeded Math.random of compose_row.py (SEEDED_RANDOM), with the reset the driver calls.
vm.runInThisContext(`(() => {
  const initialSeed = 49734321;
  let seed = initialSeed;
  Math.random = () => {
    seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffff_ffff;
    seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffff_ffff;
    seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffff_ffff;
    seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffff_ffff;
    seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffff_ffff;
    seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffff_ffff;
    return (seed >>> 0) / 0x1_0000_0000;
  };
  Math.random.__resetSeed = () => { seed = initialSeed; };
})();`);

if (pinnedDate) {
    const [y, m, d] = pinnedDate.split("-").map(Number);
    const RealDate = Date;
    const instant = new RealDate(y, m - 1, d, 12, 0, 0).getTime();
    class PinnedDate extends RealDate {
        constructor(...a) { if (a.length === 0) super(instant); else super(...a); }
        static now() { return instant; }
    }
    globalThis.Date = PinnedDate;
}

globalThis.__vc = { track: 0, trigger: 0, sched: 0, run: 0, flush: 0, cb: 0, hash: "" };
vm.runInThisContext(common, { filename: "common.js" });
vm.runInThisContext(bundle, { filename: "vue-bundle.js" });
vm.runInThisContext(benchmark, { filename: "vue-benchmark.js" });

(async () => {
    const rows = [];
    for (let i = 0; i < ITERATIONS; i++) {
        const c = globalThis.__vc;
        c.track = c.trigger = c.sched = c.run = c.flush = c.cb = 0;
        Math.random.__resetSeed();
        await new Benchmark().runIteration();
        const hash = fnv1a(c.hash);
        rows.push([c.track, c.trigger, c.sched, c.run, c.flush, c.cb, c.hash.length, hash]);
    }
    if (emitC) {
        console.log(`static const unsigned int g_expect[${ITERATIONS}][8] = {`);
        for (const r of rows) console.log(`    {${r.join(", ")}},`);
        console.log("};");
    } else {
        console.log("iter track trigger sched run flush cb len hash");
        rows.forEach((r, i) => console.log(i, ...r));
    }
})().catch((e) => { console.error(e); process.exit(1); });
