// Self-hosted global functions (ES §18.2).
//
// These are pure-spec algorithms over the `__ops__` bridge — no Rust behind
// them. This is the proof-of-scale for the JS-builtins architecture: the file
// is embedded via `include_str!` and evaluated by `builtins/core/bootstrap.rs`
// during realm init.

// isNaN(number) — ES §18.2.3: ToNumber, then true iff the result is NaN.
function isNaN(number) {
  var n = __ops__.toNumber(number);
  return n !== n;
}

// isFinite(number) — ES §18.2.2: ToNumber, then false iff NaN or ±Infinity.
function isFinite(number) {
  var n = __ops__.toNumber(number);
  return n === n && n !== Infinity && n !== -Infinity;
}