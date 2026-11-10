//! BigInt arithmetic and NumberToBigInt against Node's output, within one 64-bit digit and
//! across the digit boundaries.

use crate::test_host::printed;

/// `table` prints `a b` and then `a + b`, `a - b`, `a * b`, `a / b` and `a % b`, or the name of
/// the error each throws.
const PRELUDE: &str = r#"
const ops = { "+": (a, b) => a + b, "-": (a, b) => a - b, "*": (a, b) => a * b, "/": (a, b) => a / b, "%": (a, b) => a % b };
const run = (op, a, b) => { try { return String(ops[op](a, b)); } catch (e) { return e.constructor.name; } };
const table = (pairs, names) => { for (const [a, b] of pairs) print(a + " " + b + " " + names.map((op) => run(op, a, b)).join(" ")); };
const m63 = 2n ** 63n, m64 = 2n ** 64n;
"#;

/// Zero, signs and truncation: a quotient rounds toward zero, a remainder takes the dividend's
/// sign, and a zero divisor throws RangeError.
#[test]
fn one_digit_arithmetic_matches_node() {
    let source = r#"
table([[0n, 0n], [7n, 0n], [0n, 5n], [7n, 2n], [-7n, 2n], [7n, -2n], [-7n, -2n], [2n, 7n], [-2n, 7n], [5n, -5n], [-5n, 5n], [1000000007n, 3000000000n], [3000000000n, 1000000007n]], ["+", "-", "*", "/", "%"]);
"#;
    assert_eq!(
        printed(&format!("{PRELUDE}{source}"), "bigint-one-digit.js"),
        [
            "0 0 0 0 0 RangeError RangeError",
            "7 0 7 7 0 RangeError RangeError",
            "0 5 5 -5 0 0 0",
            "7 2 9 5 14 3 1",
            "-7 2 -5 -9 -14 -3 -1",
            "7 -2 5 9 -14 -3 1",
            "-7 -2 -9 -5 14 3 -1",
            "2 7 9 -5 14 0 2",
            "-2 7 5 -9 -14 0 -2",
            "5 -5 0 10 -25 -1 0",
            "-5 5 0 -10 -25 -1 0",
            "1000000007 3000000000 4000000007 -1999999993 3000000021000000000 0 1000000007",
            "3000000000 1000000007 4000000007 1999999993 3000000021000000000 2 999999986",
        ]
    );
}

/// Sums, differences and products that cross 2^63 and 2^64 either way, and operands of one
/// digit against wider ones.
#[test]
fn arithmetic_across_digit_boundaries_matches_node() {
    let source = r#"
table([[m63 - 1n, 1n], [m63, -1n], [-m63, -1n], [-(m63 - 1n), -2n], [m64 - 1n, 1n], [m64, -1n], [-(m64 - 1n), -1n], [-m64, 1n], [m64 - 1n, m64 - 1n], [-(m64 - 1n), m64 - 1n], [m63, 2n], [m63 - 1n, -(m63 - 1n)], [m64 - 1n, 2n], [m64 + 5n, 3n], [3n, m64 + 5n], [-(2n ** 100n), m64 - 1n], [2n ** 100n, -7n]], ["+", "-", "*", "/", "%"]);
"#;
    assert_eq!(
        printed(&format!("{PRELUDE}{source}"), "bigint-boundaries.js"),
        [
            "9223372036854775807 1 9223372036854775808 9223372036854775806 9223372036854775807 9223372036854775807 0",
            "9223372036854775808 -1 9223372036854775807 9223372036854775809 -9223372036854775808 -9223372036854775808 0",
            "-9223372036854775808 -1 -9223372036854775809 -9223372036854775807 9223372036854775808 9223372036854775808 0",
            "-9223372036854775807 -2 -9223372036854775809 -9223372036854775805 18446744073709551614 4611686018427387903 -1",
            "18446744073709551615 1 18446744073709551616 18446744073709551614 18446744073709551615 18446744073709551615 0",
            "18446744073709551616 -1 18446744073709551615 18446744073709551617 -18446744073709551616 -18446744073709551616 0",
            "-18446744073709551615 -1 -18446744073709551616 -18446744073709551614 18446744073709551615 18446744073709551615 0",
            "-18446744073709551616 1 -18446744073709551615 -18446744073709551617 -18446744073709551616 -18446744073709551616 0",
            "18446744073709551615 18446744073709551615 36893488147419103230 0 340282366920938463426481119284349108225 1 0",
            "-18446744073709551615 18446744073709551615 0 -36893488147419103230 -340282366920938463426481119284349108225 -1 0",
            "9223372036854775808 2 9223372036854775810 9223372036854775806 18446744073709551616 4611686018427387904 0",
            "9223372036854775807 -9223372036854775807 0 18446744073709551614 -85070591730234615847396907784232501249 -1 0",
            "18446744073709551615 2 18446744073709551617 18446744073709551613 36893488147419103230 9223372036854775807 1",
            "18446744073709551621 3 18446744073709551624 18446744073709551618 55340232221128654863 6148914691236517207 0",
            "3 18446744073709551621 18446744073709551624 -18446744073709551618 55340232221128654863 0 3",
            "-1267650600228229401496703205376 18446744073709551615 -1267650600209782657422993653761 -1267650600246676145570412756991 -23384026197294446689991306723232298912998217482240 -68719476736 -68719476736",
            "1267650600228229401496703205376 -7 1267650600228229401496703205369 1267650600228229401496703205383 -8873554201597605810476922437632 -181092942889747057356671886482 2",
        ]
    );
}

/// NumberToBigInt of safe integers, wider integers and non-integers (RangeError), string and
/// boolean conversions, `-0n`, mixed equality and relations, and loops that mix the paths.
#[test]
fn conversions_and_comparisons_match_node() {
    let source = r#"
const conv = (f) => { try { const v = f(); return typeof v + ":" + String(v); } catch (e) { return e.constructor.name + ":" + e.message; } };
print([() => BigInt(2 ** 53 - 1), () => BigInt(-(2 ** 53 - 1)), () => BigInt(2 ** 53), () => BigInt(-(2 ** 53)), () => BigInt(2 ** 64), () => BigInt(1e21), () => BigInt(-0), () => BigInt(0), () => BigInt(-1), () => BigInt(4294967296), () => BigInt(1.5), () => BigInt(-0.5), () => BigInt(NaN), () => BigInt(Infinity), () => BigInt(-Infinity), () => BigInt("12"), () => BigInt(" -0x10 "), () => BigInt(true), () => BigInt(false), () => BigInt(Number.MAX_VALUE) % 1000n, () => BigInt(5e-324)].map(conv).join(" | "));
print([-0n === 0n, Object.is(-0n, 0n), String(-0n), String(0n * -1n), String(-5n % 5n), String(-1n / 2n), Object.is(-1n / 2n, 0n), 1n == 1, 2n > 1, 1n === 1, typeof 1n, typeof (1n + 2n), 2n ** 64n == 18446744073709551616, BigInt(2 ** 53) == 2 ** 53, -3n < -2n, (m64 - 1n) > (m63), -(m64) < -(m64 - 1n)].join(" "));
let s = 0n; for (let i = 0; i < 3000; i++) s = (s * 3n + BigInt(i) - 1n) % 1000000007n; print(String(s));
let t = 1n; for (let i = 0; i < 3000; i++) t = t * -3n % (m64 + 13n); print(String(t));
"#;
    assert_eq!(
        printed(&format!("{PRELUDE}{source}"), "bigint-conversions.js"),
        [
            "bigint:9007199254740991 | bigint:-9007199254740991 | bigint:9007199254740992 | bigint:-9007199254740992 | bigint:18446744073709551616 | bigint:1000000000000000000000 | bigint:0 | bigint:0 | bigint:-1 | bigint:4294967296 | RangeError:The number 1.5 cannot be converted to a BigInt because it is not an integer | RangeError:The number -0.5 cannot be converted to a BigInt because it is not an integer | RangeError:The number NaN cannot be converted to a BigInt because it is not an integer | RangeError:The number Infinity cannot be converted to a BigInt because it is not an integer | RangeError:The number -Infinity cannot be converted to a BigInt because it is not an integer | bigint:12 | SyntaxError:Cannot convert  -0x10  to a BigInt | bigint:1 | bigint:0 | bigint:368 | RangeError:The number 5e-324 cannot be converted to a BigInt because it is not an integer",
            "true true 0 0 0 0 true true true false bigint bigint true true true true true",
            "-645418774",
            "3785700625493088643",
        ]
    );
}
