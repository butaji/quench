//! Strict equality, `!==` and `switch` of every pair of Symbols, BigInts, strings, records,
//! functions, promises and primitives (two distinct heap handles are equal only as strings or
//! BigInts), against Node's output.
use crate::test_host::printed;

const SOURCE: &str = r#"
const s1 = Symbol('a'), s2 = Symbol('a');
const big = [s1, s2, 1n, 1n, 2n, 'ab', 'a' + 'b', 'x', {}, {}, [], [], function f() {}, null, undefined, 0, NaN, 1.5, Promise.resolve(1), 10n ** 30n, 10n ** 30n, Symbol.iterator, Symbol.iterator];
function matrix(vals) {
  let out = '';
  for (let i = 0; i < vals.length; i++) for (let j = 0; j < vals.length; j++) {
    const a = vals[i], b = vals[j];
    let r = 0;
    if (a === b) r |= 1;
    if (a !== b) r |= 2;
    switch (a) { case b: r |= 4; break; default: r |= 8; }
    out += r.toString(16);
  }
  return out;
}
let last;
for (let k = 0; k < 50; k++) last = matrix(big);
print(last);
"#;

#[test]
fn strict_equality_of_distinct_heap_values_matches_node() {
    assert_eq!(
        printed(SOURCE, "strict-equality.js"),
        ["5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa5aaaaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaaaa55aaaaaaaaaaaaaaaaaaaaa55"]
    );
}
