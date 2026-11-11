//! `Assertion` over an ASCII subject: the tests `assertion_holds` makes, branch by branch.
use super::Emit;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder};
use quench_regexp::backtrack::native::NativeAssertion;

const LINE_FEED: i64 = b'\n' as i64;
const CARRIAGE_RETURN: i64 = b'\r' as i64;
/// Setting bit 5 folds an ASCII upper-case letter to its lower case.
const CASE_BIT: i64 = 0x20;
const LETTERS: i64 = 26;
const DIGITS: i64 = 10;
/// The position after the next one: a `\r\n` pair at the end is two units.
const PAIR_END: i64 = 2;

impl Emit<'_, '_> {
    pub(super) fn assertion(&mut self, pc: u32, assertion: NativeAssertion) {
        match assertion {
            NativeAssertion::Start { multiline } => self.input_start(multiline),
            NativeAssertion::End { multiline } => self.input_end(multiline),
            NativeAssertion::StartBuffer => {
                let position = self.position();
                let holds = self.compare_constant(IntCC::Equal, position, 0);
                self.require(holds);
            }
            NativeAssertion::EndBuffer => {
                let position = self.position();
                let holds = self.compare(IntCC::Equal, position, self.length);
                self.require(holds);
            }
            NativeAssertion::EndBufferOptionalNewline => self.end_optional_newline(),
            NativeAssertion::Boundary => self.boundary(true),
            NativeAssertion::NonBoundary => self.boundary(false),
        }
        self.next(pc);
    }

    /// Whether the unit at `index`, inside the subject, is a line terminator.
    fn terminator_at(&mut self, index: ir::Value) -> ir::Value {
        let byte = self.byte_at(index);
        let line_feed = self.compare_constant(IntCC::Equal, byte, LINE_FEED);
        let carriage_return = self.compare_constant(IntCC::Equal, byte, CARRIAGE_RETURN);
        self.b.ins().bor(line_feed, carriage_return)
    }

    /// Continues where `condition` holds or `already` did; fails otherwise.
    fn pass_or(&mut self, already: ir::Value, check: impl FnOnce(&mut Self) -> ir::Value) {
        let (done, probe) = (self.b.create_block(), self.b.create_block());
        self.branch(already, done, probe);
        self.b.switch_to_block(probe);
        let holds = check(self);
        self.branch(holds, done, self.backtrack);
        self.b.switch_to_block(done);
    }

    fn input_start(&mut self, multiline: bool) {
        let position = self.position();
        let at_start = self.compare_constant(IntCC::Equal, position, 0);
        if !multiline {
            return self.require(at_start);
        }
        self.pass_or(at_start, |emit| {
            let before = emit.b.ins().iadd_imm_s(position, -1);
            emit.terminator_at(before)
        });
    }

    fn input_end(&mut self, multiline: bool) {
        let position = self.position();
        let at_end = self.compare(IntCC::Equal, position, self.length);
        if !multiline {
            return self.require(at_end);
        }
        self.pass_or(at_end, |emit| emit.terminator_at(position));
    }

    /// `$` without the multiline flag in Python's sense: the end, or before one final newline.
    fn end_optional_newline(&mut self) {
        let position = self.position();
        let at_end = self.compare(IntCC::Equal, position, self.length);
        self.pass_or(at_end, |emit| {
            let one = emit.b.ins().iadd_imm_s(position, 1);
            let two = emit.b.ins().iadd_imm_s(position, PAIR_END);
            let last = emit.compare(IntCC::Equal, one, emit.length);
            let before_last = emit.compare(IntCC::Equal, two, emit.length);
            let (single, pair, done) = (emit.b.create_block(), emit.b.create_block(), emit.b.create_block());
            emit.b.append_block_param(done, types::I8);
            emit.branch(last, single, pair);
            emit.b.switch_to_block(single);
            let terminator = emit.terminator_at(position);
            emit.b.ins().jump(done, &[terminator.into()]);
            emit.b.switch_to_block(pair);
            let none = emit.b.ins().iconst(types::I8, 0);
            let crlf = emit.b.create_block();
            emit.b.ins().brif(before_last, crlf, &[], done, &[none.into()]);
            emit.b.switch_to_block(crlf);
            let first = emit.byte_at(position);
            let second = emit.byte_at(one);
            let is_cr = emit.compare_constant(IntCC::Equal, first, CARRIAGE_RETURN);
            let is_lf = emit.compare_constant(IntCC::Equal, second, LINE_FEED);
            let both = emit.b.ins().band(is_cr, is_lf);
            emit.b.ins().jump(done, &[both.into()]);
            emit.b.switch_to_block(done);
            emit.b.block_params(done)[0]
        });
    }

    /// Whether the byte is a word unit, `[A-Za-z0-9_]`.
    fn is_word(&mut self, byte: ir::Value) -> ir::Value {
        let lowered = self.b.ins().bor_imm_u(byte, CASE_BIT);
        let letter = self.b.ins().iadd_imm_s(lowered, -i64::from(b'a'));
        let letter = self.compare_constant(IntCC::UnsignedLessThan, letter, LETTERS);
        let digit = self.b.ins().iadd_imm_s(byte, -i64::from(b'0'));
        let digit = self.compare_constant(IntCC::UnsignedLessThan, digit, DIGITS);
        let under = self.compare_constant(IntCC::Equal, byte, i64::from(b'_'));
        let either = self.b.ins().bor(letter, digit);
        self.b.ins().bor(either, under)
    }

    /// Whether the unit at `index` is a word unit where `inside` holds, else false.
    fn word_where(&mut self, inside: ir::Value, index: ir::Value) -> ir::Value {
        let (load, done) = (self.b.create_block(), self.b.create_block());
        self.b.append_block_param(done, types::I8);
        let none = self.b.ins().iconst(types::I8, 0);
        self.b.ins().brif(inside, load, &[], done, &[none.into()]);
        self.b.switch_to_block(load);
        let byte = self.byte_at(index);
        let word = self.is_word(byte);
        self.b.ins().jump(done, &[word.into()]);
        self.b.switch_to_block(done);
        self.b.block_params(done)[0]
    }

    fn boundary(&mut self, expected: bool) {
        let position = self.position();
        let after_start = self.compare_constant(IntCC::UnsignedGreaterThan, position, 0);
        let before = self.b.ins().iadd_imm_s(position, -1);
        let left = self.word_where(after_start, before);
        let before_end = self.compare(IntCC::UnsignedLessThan, position, self.length);
        let right = self.word_where(before_end, position);
        let differ = self.b.ins().icmp(if expected { IntCC::NotEqual } else { IntCC::Equal }, left, right);
        self.require(differ);
    }
}
