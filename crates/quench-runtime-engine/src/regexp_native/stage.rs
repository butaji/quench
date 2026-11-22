//! `MatchStage` on the emitter: the matcher's operations of `quench_regexp::backtrack::matcher`
//! as Cranelift IR over an ASCII subject. A position or unit is an `i64` value, a flag an `i8`
//! one, and a guarded operation a pair of blocks that join on the flag.
use super::{Emit, RegexpMachine};
use quench_runtime::staged::machine::{Binary, Extend, Lane, Relation};
use quench_regexp::backtrack::matcher::MatchStage;
use quench_regexp::backtrack::native::{NativeTest, NATIVE_ASCII_UNITS};

/// The bits of a mask word, and the mask of a shift within one.
const WORD_BITS: i64 = 64;
const WORD_SHIFT_MASK: i64 = WORD_BITS - 1;
/// The words of a mask of the ASCII units.
const MASK_WORDS: usize = (NATIVE_ASCII_UNITS as i64 / WORD_BITS) as usize;

impl<M: RegexpMachine> Emit<'_, M> {
    /// Bit `byte` of the 128-bit `mask`.
    fn in_mask(&mut self, mask: [u64; MASK_WORDS], byte: M::Word) -> M::Word {
        let low = self.constant(mask[0] as i64);
        let high = self.constant(mask[1] as i64);
        let below = self.compare_constant(Relation::UnsignedLess, byte, WORD_BITS);
        let word = self.b.raw_select(below, low, high);
        let shift = self.b.raw_binary_imm(Binary::And, byte, WORD_SHIFT_MASK);
        let bit = self.b.raw_binary(Binary::ShiftRightUnsigned, word, shift);
        let bit = self.b.raw_binary_imm(Binary::And, bit, 1);
        self.compare_constant(Relation::NotEqual, bit, 0)
    }

    /// Whether `byte` lies in `first..=last`.
    fn in_range(&mut self, byte: M::Word, (first, last): (u32, u32)) -> M::Word {
        if first == last {
            return self.compare_constant(Relation::Equal, byte, i64::from(first));
        }
        let offset = self.b.raw_binary_imm(Binary::Add, byte, -i64::from(first));
        self.compare_constant(Relation::UnsignedLess, offset, i64::from(last - first) + 1)
    }
}

impl<M: RegexpMachine> MatchStage for Emit<'_, M> {
    type Pos = M::Word;
    type Unit = M::Word;
    type Flag = M::Word;
    type Test = NativeTest;

    fn length(&mut self) -> M::Word {
        self.length
    }

    fn at_start(&mut self, position: M::Word) -> M::Word {
        self.compare_constant(Relation::Equal, position, 0)
    }

    fn equal(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.compare(Relation::Equal, left, right)
    }

    fn before(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.compare(Relation::UnsignedLess, left, right)
    }

    fn offset(&mut self, position: M::Word, delta: isize) -> M::Word {
        self.b.raw_binary_imm(Binary::Add, position, delta as i64)
    }

    fn unit_at(&mut self, index: M::Word) -> M::Word {
        let address = self.b.raw_binary(Binary::Add, self.bytes, index);
        self.b.raw_load_extended(Lane::I8, Extend::Zero, Lane::I64, self.flags, address, 0)
    }

    fn passes(&mut self, unit: M::Word, test: NativeTest) -> M::Word {
        match test {
            NativeTest::Never => self.b.raw_const(Lane::I8, 0),
            NativeTest::Exact(expected) => self.compare_constant(Relation::Equal, unit, i64::from(expected)),
            NativeTest::Mask(mask) => self.in_mask(mask, unit),
        }
    }

    /// The ranges' ASCII part: an ASCII subject holds no other unit.
    fn in_ranges(&mut self, unit: M::Word, ranges: &[(u32, u32)]) -> M::Word {
        // Two or more ranges (`\w`, which `\b` reads twice) are one test of their mask.
        if ranges.len() > 1 {
            let mut mask = [0u64; MASK_WORDS];
            for &(first, last) in ranges.iter().filter(|(first, _)| *first < NATIVE_ASCII_UNITS) {
                for unit in first..=last.min(NATIVE_ASCII_UNITS - 1) {
                    mask[(unit as i64 / WORD_BITS) as usize] |= 1 << (unit as i64 & WORD_SHIFT_MASK);
                }
            }
            return self.in_mask(mask, unit);
        }
        let mut found = self.b.raw_const(Lane::I8, 0);
        for &(first, last) in ranges.iter().filter(|(first, _)| *first < NATIVE_ASCII_UNITS) {
            let hit = self.in_range(unit, (first, last.min(NATIVE_ASCII_UNITS - 1)));
            found = self.b.raw_binary(Binary::Or, found, hit);
        }
        found
    }

    /// The complement of a comparison is the opposite comparison, which a branch fuses with.
    fn not(&mut self, flag: M::Word) -> M::Word {
        self.b.compare_complement(flag).unwrap_or_else(|| self.b.raw_binary_imm(Binary::Xor, flag, 1))
    }

    fn differ(&mut self, left: M::Word, right: M::Word) -> M::Word {
        self.compare(Relation::NotEqual, left, right)
    }

    fn guarded(&mut self, condition: M::Word, then: impl FnOnce(&mut Self) -> M::Word) -> M::Word {
        let (run, done) = (self.b.new_block(), self.b.new_block());
        let outcome = self.b.block_parameter(done, Lane::I8);
        let none = self.b.raw_const(Lane::I8, 0);
        self.b.branch_if(condition, (run, &[]), (done, &[none]));
        self.b.enter_open(run);
        let held = then(self);
        self.b.goto(done, &[held]);
        self.b.enter_open(done);
        outcome
    }

    fn or_else(&mut self, first: M::Word, otherwise: impl FnOnce(&mut Self) -> M::Word) -> M::Word {
        let (probe, done) = (self.b.new_block(), self.b.new_block());
        let outcome = self.b.block_parameter(done, Lane::I8);
        self.b.branch_if(first, (done, &[first]), (probe, &[]));
        self.b.enter_open(probe);
        let held = otherwise(self);
        self.b.goto(done, &[held]);
        self.b.enter_open(done);
        outcome
    }
}
