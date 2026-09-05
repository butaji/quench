/* Executed unchanged by each engine after the selected registration and configuration. */
(function() {
  "use strict";
  var spec = __microSpec, cfg = __microConfig;
  var emit = typeof print === "function" ? print : function(s) { console.log(s); };
  function chooseClock() {
    if (typeof process !== "undefined" && process.hrtime && process.hrtime.bigint) return { kind: "hrtime", read: function() { return Number(process.hrtime.bigint()); } };
    if (typeof performance !== "undefined" && performance.now) return { kind: "performance", read: function() { return performance.now() * 1000000; } };
    return { kind: "date", read: function() { return Date.now() * 1000000; } };
  }
  var selectedClock = chooseClock(), clock = selectedClock.read, clockKind = selectedClock.kind;
  var state = spec.setup(cfg.n, cfg.seed, cfg.variant), operation = spec.variants[cfg.variant];
  var sink = 0, expected, validations = 0, windows = [], warmup = [], epoch = 0;

  function encode(x) {
    if (x === undefined) return ["undefined"];
    if (typeof x === "number") return ["number", Number.isNaN(x) ? "NaN" : Object.is(x, -0) ? "-0" : String(x)];
    if (typeof x === "bigint") return ["bigint", String(x)];
    if (x === null || typeof x !== "object") return [typeof x, x];
    if (Array.isArray(x)) return ["array", x.map(encode)];
    return ["object", Object.keys(x).map(function(k) { return [k, encode(x[k])]; })];
  }
  function signature(value) { return JSON.stringify(encode(value)); }
  function validate(value) {
    if (spec.check) spec.check(value, state, cfg.variant);
    var actual = signature(value);
    if (actual !== expected) throw new Error("result changed between invocations");
    validations++;
  }
  function consume(value) {
    var part = typeof value === "number" ? value : typeof value === "string" ? value.length : Array.isArray(value) ? value.length : value === undefined ? 0 : 1;
    sink = ((sink * 33) ^ part) | 0;
    return value;
  }
  function invokeSync() { return consume(operation(state)); }
  async function invokeAsync() { return consume(await operation(state)); }
  function windowSync(ms, minimum) {
    var started = clock(), calls = 0, value;
    do { for (var b = 0; b < (ms ? 16 : 1); b++) { value = invokeSync(); calls++; } } while (calls < minimum || (ms && clock() - started < ms * 1000000));
    return { calls: calls, elapsed_ns: clock() - started, value: value };
  }
  async function windowAsync(ms, minimum) {
    var started = clock(), calls = 0, value;
    do { value = await invokeAsync(); calls++; } while (calls < minimum || (ms && clock() - started < ms * 1000000));
    return { calls: calls, elapsed_ns: clock() - started, value: value };
  }
  function keepWindow(w, target) { validate(w.value); target.push({ calls: w.calls, elapsed_ns: w.elapsed_ns }); }
  function phase(name) { emit("MICRO_PHASE " + JSON.stringify({ epoch: epoch, phase: name })); }
  function finish() {
    if (spec.release) spec.release(state);
    emit("MICRO_RESULT " + JSON.stringify({ schema: 1, id: cfg.id, mode: cfg.mode, clock: clockKind,
      result: expected, validations: validations, sink: sink, windows: windows, warmup: warmup,
      epochs: epoch, work_per_call: cfg.n, normalization: "declared input elements; compare only within a scenario" }));
  }
  function fail(error) { emit("MICRO_ERROR " + String(error && error.stack || error)); if (typeof process !== "undefined") process.exitCode = 1; }
  function runSync() {
    expected = signature(operation(state));
    if (cfg.mode === "throughput") {
      for (var w = 0; w < 4; w++) keepWindow(windowSync(cfg.warmupMs / 4, 1), warmup);
      for (var j = 0; j < cfg.windows; j++) keepWindow(windowSync(cfg.windowMs, 1), windows);
    } else if (cfg.mode === "lifecycle") {
      for (epoch = 1; epoch <= cfg.epochs; epoch++) { phase("allocate-exercise"); keepWindow(windowSync(0, cfg.epochCalls), windows); phase("retained"); if (spec.release) spec.release(state); phase("released"); }
      epoch = cfg.epochs;
    } else keepWindow(windowSync(0, cfg.fixedCalls), windows);
    finish();
  }
  async function runAsync() {
    expected = signature(await operation(state));
    if (cfg.mode === "throughput") {
      for (var w = 0; w < 4; w++) keepWindow(await windowAsync(cfg.warmupMs / 4, 1), warmup);
      for (var j = 0; j < cfg.windows; j++) keepWindow(await windowAsync(cfg.windowMs, 1), windows);
    } else keepWindow(await windowAsync(0, cfg.fixedCalls), windows);
    finish();
  }
  try { if (spec.async) runAsync().catch(fail); else runSync(); } catch (error) { fail(error); }
}());
