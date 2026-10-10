# EarleyBoyer constant-branch fusion preflight

The candidate fused `LoadConst → JumpFalse` only when both instructions used
the same register, the constant had a statically known primitive truth value,
and its constant-pool index fit the packed instruction field. The fused
instruction retained the constant register write and branch target. BigInt and
Wasm constants kept the original path.

The focused Node v24.19.0 oracle matched baseline and candidate exactly for
false/true, positive and signed zero, nonzero numbers, empty/nonempty strings,
null, and BigInt conditional branches. The pinned EarleyBoyer profile run
recorded 493,260 executions of the new opcode out of 737,406,514 physical
dispatches (0.067%). Even removing one full dispatch on every fused execution
would cap the direct dispatch reduction at 0.067%, below a credible production
Score signal and with no plausible RSS benefit. I stopped this candidate
before production pair scoring and kept the profile evidence and patch here.

The compiler folds only cases whose truthiness it can prove. This follows
ECMAScript `ToBoolean`: zero and negative zero, NaN, zero BigInt, null,
undefined, and the empty string are false; the oracle confirms the exercised
Quench paths. See the [ECMAScript ToBoolean algorithm](https://tc39.es/ecma262/2022/multipage/abstract-operations.html#sec-toboolean).
