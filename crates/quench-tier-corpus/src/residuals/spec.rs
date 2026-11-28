//! What the C version of each kernel (first line of its fixture) needs in the hot loop, per tier.
//!
//! Conventions, shared by every row:
//! - boxes 0: C stores raw integers and doubles; a tag store is residual.
//! - spills 0: the live loop values (at most eight) fit the sixteen general registers C uses.
//! - calls 0: C inlines small callees and keeps allocation growth in a cold path.
//! - guards: exits other than the loop's own test. Bounds yes (one per element access, the
//!   cost of safe indexing), type and shape no (proved once before the loop).
//! - u-jit instructions, CLIF: loop test `icmp`+`brif` (2), the back edge `jump` (1), the
//!   interrupt poll `atomic_load` (1: JavaScript must stay interruptible), the increment (1).
//!   An indexed element access is bounds `icmp`+`brif` (2), `uextend`, `iadd`, `load` (3).
//! - b-jit instructions, stencil rows: loop test (1, a fused compare-and-branch row), the
//!   back-edge poll row (1), the increment row (1); an element access is one `Index` or
//!   `Store` row (bounds inside the stencil).

use super::{Counts, Kernel};

macro_rules! kernel {
    ($name:literal, u: $u:expr, b: $b:expr $(,)?) => {
        Kernel {
            name: $name,
            source: include_str!(concat!(
                "../../../../tests/fixtures/residuals/",
                $name,
                ".js"
            )),
            expect: [Counts($u), Counts($b)],
        }
    };
}

pub const KERNELS: &[Kernel] = &[
    // u: test 2 + iadd s + iadd i + poll + jump = 6. b: Add, Add, loop test, poll = 4.
    kernel!("int-sum", u: [0, 0, 0, 0, 6], b: [0, 0, 0, 0, 4]),
    // u: test 2 + two element reads 2x5 + fmul + fadd + iadd i + poll + jump = 17. b: 2 Index, Mul, Add, Add, test, poll = 7.
    kernel!("float-dot", u: [0, 2, 0, 0, 17], b: [0, 2, 0, 0, 7]),
    // u: test 2 + read 5 + write 5 (bounds 2, uextend, iadd, store) + iadd i + poll + jump = 15. b: Index, Store, Add, test, poll = 5.
    kernel!("array-copy", u: [0, 2, 0, 0, 15], b: [0, 2, 0, 0, 5]),
    // Shape proved once: no guard. u: test 2 + 2 loads + iadd + store + iadd i + poll + jump = 9. b: 2 Field, Add, store row, Add, test, poll = 7.
    kernel!("field-accumulate", u: [0, 0, 0, 0, 9], b: [0, 0, 0, 0, 7]),
    // One bounds check on ps[i&1]; the two-layout dispatch (tag load, icmp, select) rejoins in the loop.
    // u: test 2 + band + bounds 2 + addr 3 + dispatch 3 + load + iadd s + iadd i + poll + jump = 16. b: And, Index, Field dispatch, Add, Add, test, poll = 7.
    kernel!("poly-field", u: [0, 1, 0, 0, 16], b: [0, 1, 0, 0, 7]),
    // One bounds check on s[i]. u: test 2 + (bounds 2, addr 2, load, uextend = 6) + shl, isub, iadd 3 + iadd i + poll + jump = 14. b: Shl, Sub, CharCodeAt, Add, Add, test, poll = 7.
    kernel!("charcode-hash", u: [0, 1, 0, 0, 14], b: [0, 1, 0, 0, 7]),
    // Inlined callee folds into the int-sum body: same counts.
    kernel!("small-call", u: [0, 0, 0, 0, 6], b: [0, 0, 0, 0, 4]),
    // The captured counter is a register once inlined. u: test 2 + 3 iadd + poll + jump = 7. b: 3 Add, test, poll = 5.
    kernel!("closure-counter", u: [0, 0, 0, 0, 7], b: [0, 0, 0, 0, 5]),
    // One capacity guard. u: test 2 + len, cap loads 2 + icmp, brif 2 + addr 2 + store + iadd len + store len + iadd i + poll + jump = 15. b: Push, Add, test, poll = 4.
    kernel!("array-push", u: [0, 1, 0, 0, 15], b: [0, 1, 0, 0, 4]),
    // Three shift-xor pairs. u: test 2 + 6 ops + iadd i + poll + jump = 11. b: 6 rows + Add + test + poll = 9.
    kernel!("bitwise-mix", u: [0, 0, 0, 0, 11], b: [0, 0, 0, 0, 9]),
    // Innermost k loop, i*n and j hoisted by C: two bounds checks. u: test 2 + a: iadd, bounds 2, addr 2, load = 6 + b: imul, iadd, bounds 2, addr 2, load = 7 + fmul, fadd, iadd k, poll, jump = 20.
    // b: Add, Mul, Add, 2 Index, Mul, Add, Add k, test, poll = 10.
    kernel!("matmul", u: [0, 2, 0, 0, 20], b: [0, 2, 0, 0, 10]),
    // The break is the one extra exit. u: test 2 + imul + icmp + brif + iadd + poll + jump = 8. b: Mul, break row, Add, test, poll = 5.
    kernel!("early-exit", u: [0, 1, 0, 0, 8], b: [0, 1, 0, 0, 5]),
    // The handler is a never-taken branch that rejoins: no exit. u: test 2 + iadd s + icmp + brif + iadd i + poll + jump = 8. b: Add, branch row, Add, test, poll = 5.
    kernel!("try-catch", u: [0, 0, 0, 0, 8], b: [0, 0, 0, 0, 5]),
    // Producer and consumer fuse into the int-sum loop: same counts.
    kernel!("generator-yield", u: [0, 0, 0, 0, 6], b: [0, 0, 0, 0, 4]),
    // The stable prototype method inlines to the field-accumulate body: same counts.
    kernel!("proto-method", u: [0, 0, 0, 0, 9], b: [0, 0, 0, 0, 7]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kernel_runs_hot_through_a_named_function() {
        for kernel in KERNELS {
            assert!(
                kernel.source.contains("function kernel")
                    || kernel.source.contains("function* produce"),
                "{}",
                kernel.name
            );
        }
        assert_eq!(KERNELS.len(), 15);
    }
}
