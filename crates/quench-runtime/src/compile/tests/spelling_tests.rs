//! Bun's anti-cheat invariance: the code a program compiles to depends on its meaning, never
//! on how its text spells that meaning. A property read written `x.a`, `x["a"]` or
//! `` x[`a`] ``, and any whitespace or comment between tokens, compile to the same
//! instructions, so no tier can key a fast path on a benchmark's exact text.

use super::*;

/// One program, written several ways that mean the same thing.
const SPELLINGS: &[&[&str]] = &[
    &[
        "function f(x) { return x.a + x.b; } f({a: 1, b: 2});",
        "function f(x) { return x['a'] + x[\"b\"]; } f({a: 1, b: 2});",
        "function f(x) { return x[`a`] + x.b; } f({a: 1, b: 2});",
    ],
    &[
        "function f(x) { x.count = x.count + 1; return x; } f({count: 0});",
        "function f(x) { x['count'] = x['count'] + 1; return x; } f({count: 0});",
    ],
    &[
        "function g(n) { let s = 0; for (let i = 0; i < n; i++) s += i; return s; } g(10);",
        "function g(n) {\n  // sum\n  let s = 0;\n  for (let i = 0; i < n; i++) { s += i; } /* done */\n  return s;\n}\n g(10);",
        "function   g( n ){let s=0;for(let i=0;i<n;i++)s+=i;return s}g(10)",
    ],
    &[
        "class P { constructor(x) { this.x = x; } get(){ return this.x; } } new P(1).get();",
        "class P {\n  constructor(x) {\n    this['x'] = x;\n  }\n  get() { return this.x; }\n}\nnew P(1).get();",
    ],
    &[
        "function h(o) { return o.run(1) + o.n; } h({run(v) { return v; }, n: 2});",
        "function h(o) { return o['run'](1) + o['n']; } h({run(v) { return v; }, n: 2});",
        "function h(o) { return o['r' + 'un'](1) + o['' + 'n']; } h({run(v) { return v; }, n: 2});",
    ],
    &[
        "function c(o) { o.total += 2; o.items[0] = o.total; return o; } c({total: 1, items: [0]});",
        "function c(o) { o['total'] += 2; o[`items`][0] = o['total']; return o; } c({total: 1, items: [0]});",
    ],
];

/// The instructions of every function of `source`, in order.
fn instructions(source: &str) -> Vec<Vec<(Op, u16, u16, u16, u32)>> {
    let program = Engine::specialize(source, "spelling.js").unwrap();
    (0..program.function_count())
        .map(|id| {
            let function = program.function(id);
            function.code.iter().map(|packed| {
                let instruction = packed.as_wide();
                (instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm())
            }).collect()
        })
        .collect()
}

#[test]
fn code_depends_on_meaning_not_spelling() {
    for spellings in SPELLINGS {
        let first = instructions(spellings[0]);
        for spelling in &spellings[1..] {
            assert_eq!(instructions(spelling), first, "{spelling:?} compiles differently from {:?}", spellings[0]);
        }
    }
}
