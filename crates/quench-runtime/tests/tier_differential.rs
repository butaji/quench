//! Tier differential: random terminating programs run in the interpreter and under the JIT
//! must end identically. The interpreter is the semantic source, so any difference is a tier
//! bug. Hot loops push each generated function past the tier thresholds, and late argument
//! type flips make compiled code leave through its guards. A `tier-stress` build compiles at
//! the first chance as well. `TIER_FUZZ_SEED` and `TIER_FUZZ_CASES` widen a local campaign.

use quench_runtime::{Engine, ExecutionMode, Runtime, SystemHost};
use std::fmt::Write;

/// Calls of each generated function: past every tier's warm-up in `tier_policy`.
const HOT_CALLS: u32 = 1_500;
const DEFAULT_CASES: u64 = 120;
const FUNCTIONS: usize = 3;
const STATEMENTS: usize = 6;
const EXPRESSION_DEPTH: u32 = 3;

/// Operand leaves: parameters, locals, fields, elements and the literals at JavaScript's
/// numeric and type edges.
const LEAVES: &[&str] = &[
    "a", "b", "t0", "t1", "o.p", "o.q", "arr[0]", "arr[i & 3]", "arr.length", "0", "-0", "1", "-1",
    "2147483647", "-2147483648", "4294967295", "0.5", "-1.5", "1e21", "NaN", "Infinity", "'7'",
    "'x'", "''", "true", "null", "undefined", "c()",
];
const BINARY: &[&str] = &[
    "+", "-", "*", "/", "%", "|", "&", "^", "<<", ">>", ">>>", "<", "<=", ">", "===", "==", "!==",
    "!=", "&&", "||", "??",
];
const UNARY: &[&str] = &["-", "~", "!", "typeof ", "+", "void "];
const CALLS: &[&str] = &["Math.floor", "Math.abs", "Math.imul(3, ", "String", "Number", "Boolean"];
/// Per-call argument rotations: steady numbers first, then other types late in the run.
const ARGUMENTS: &[&str] = &[
    "i", "i * 0.5", "i - 750", "(i * 65537) | 0", "i % 3 === 0 ? -0 : i",
    "i < 1400 ? i : 'k' + i", "i < 1450 ? i : undefined", "i < 1490 ? i | 0 : 1.25",
    "i < 1300 ? i : {valueOf() { return i; }}", "i < 1495 ? i : null",
];
const SHAPE_EVENTS: &[&str] = &[
    "o = {q: i, p: o.p};", "delete o.q;", "o.r = i;", "arr.push(i);", "arr.length = 2;",
    "arr[7] = i;", "Object.defineProperty(o, 'p', {value: i, writable: true, configurable: true});",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

fn expression(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return rng.pick(LEAVES).to_owned();
    }
    let left = expression(rng, depth - 1);
    match rng.below(6) {
        0 => format!("({}{left})", rng.pick(UNARY)),
        1 => format!("({left} ? {} : {})", expression(rng, depth - 1), expression(rng, depth - 1)),
        2 => {
            let call = rng.pick(CALLS);
            if call.ends_with(", ") { format!("{call}{left})") } else { format!("{call}({left})") }
        }
        _ => format!("({left} {} {})", rng.pick(BINARY), expression(rng, depth - 1)),
    }
}

fn statement(rng: &mut Rng, out: &mut String, depth: u32) {
    let value = expression(rng, EXPRESSION_DEPTH);
    let _ = match rng.below(if depth == 0 { 5 } else { 8 }) {
        0 | 1 => write!(out, "t{} = {value};", rng.below(2)),
        2 => write!(out, "o.{} = {value};", ["p", "q"][rng.below(2)]),
        3 => write!(out, "arr[{}] = {value};", ["0", "i & 3", "t0 & 7"][rng.below(3)]),
        4 => write!(out, "if (i % 97 === 5) {{ {} }}", rng.pick(SHAPE_EVENTS)),
        5 => block(rng, out, &format!("if ({value}) "), depth),
        6 => block(rng, out, "for (var j = 0; j < 3; j++) ", depth),
        _ => block(rng, out, "try ", depth).and_then(|()| write!(out, " catch (e) {{ t1 = String(e && e.name); }}")),
    };
}

fn block(rng: &mut Rng, out: &mut String, head: &str, depth: u32) -> std::fmt::Result {
    write!(out, "{head}{{ ")?;
    for _ in 0..2 {
        statement(rng, out, depth - 1);
    }
    write!(out, " }}")
}

fn function(rng: &mut Rng, index: usize, out: &mut String) {
    let _ = write!(out, "function f{index}(a, b, o, arr, i, c) {{ var t0 = a, t1 = b; ");
    for _ in 0..STATEMENTS {
        statement(rng, out, 2);
    }
    let _ = writeln!(out, " return {}; }}", expression(rng, EXPRESSION_DEPTH));
}

/// A digest of every result: type, int32 image, sign of zero, NaN, and string length.
const PRELUDE: &str = "var h = 0;
function mix(v) {
  var n = typeof v === 'number' ? ((v * 1024) | 0) ^ (Object.is(v, -0) ? 7 : 0) ^ (v !== v ? 13 : 0)
    : typeof v === 'string' ? v.length * 31 + (v.charCodeAt(0) | 0) : typeof v === 'object' ? (v === null ? 3 : 5) : (typeof v).length;
  h = (Math.imul(h, 31) + (n | 0)) | 0;
}
function counter() { var k = 0; return function () { k = (k + 1) | 0; return k; }; }
";

fn program(seed: u64) -> String {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut out = String::from(PRELUDE);
    for index in 0..FUNCTIONS {
        function(&mut rng, index, &mut out);
    }
    let _ = writeln!(out, "var o = {{p: 1, q: 2}}, arr = [1, 2, 3, 4], c = counter();");
    let _ = write!(out, "for (var i = 0; i < {HOT_CALLS}; i++) {{ ");
    for index in 0..FUNCTIONS {
        let (a, b) = (rng.pick(ARGUMENTS), rng.pick(ARGUMENTS));
        let _ = write!(out, "try {{ mix(f{index}({a}, {b}, o, arr, i, c)); }} catch (e) {{ mix(String(e)); }} ");
    }
    let _ = write!(out, "}}\nthrow h + ' ' + JSON.stringify(o) + ' ' + JSON.stringify(arr);\n");
    out
}

fn run(source: &str, mode: ExecutionMode) -> String {
    let outcome = Engine::specialize(source, "tier-differential.js")
        .map_err(|errors| format!("{errors:?}"))
        .and_then(|program| {
            let mut runtime = Runtime::new(SystemHost);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).map_err(|error| error.to_string())
        });
    format!("{outcome:?}")
}

fn environment(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|value| value.parse().ok()).unwrap_or(default)
}

#[test]
fn compiled_tiers_end_as_the_interpreter_does() {
    let first = environment("TIER_FUZZ_SEED", 1);
    let cases = environment("TIER_FUZZ_CASES", DEFAULT_CASES);
    let divergent: Vec<String> = (first..first + cases)
        .filter_map(|seed| {
            let source = program(seed);
            let interpreted = run(&source, ExecutionMode::Interpreted);
            let compiled = run(&source, ExecutionMode::Compiled);
            (interpreted != compiled).then(|| format!("seed {seed}:\n  interpreter {interpreted}\n  jit {compiled}\n{source}"))
        })
        .collect();
    assert!(divergent.is_empty(), "{} of {cases} programs diverge\n{}", divergent.len(), divergent.join("\n"));
}

/// Regressions the campaigns found, kept as fixed programs.
const FOUND: &[&str] = &[
    // The general tier ran a generator's parameter initialization past its stop.
    "function* g(xs = [1, 2]) { for (let x of xs) yield x; }
     var s = 0; for (var k = 0; k < 40; k++) for (var v of g()) s += v; throw s;",
];

#[test]
fn found_programs_end_as_the_interpreter_does() {
    for source in FOUND {
        assert_eq!(run(source, ExecutionMode::Interpreted), run(source, ExecutionMode::Compiled), "{source}");
    }
}
