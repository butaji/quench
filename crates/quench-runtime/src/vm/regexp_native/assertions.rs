//! `Assertion` over an ASCII subject: the tests `assertion_holds` makes, branch by branch.
use super::Emit;
use crate::staged::machine::{Binary, Lane, MachineFlow, MachineScalar, Relation};
use cranelift_codegen::ir;
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
                let holds = self.compare_constant(Relation::Equal, position, 0);
                self.require(holds);
            }
            NativeAssertion::EndBuffer => {
                let position = self.position();
                let holds = self.compare(Relation::Equal, position, self.length);
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
        let line_feed = self.compare_constant(Relation::Equal, byte, LINE_FEED);
        let carriage_return = self.compare_constant(Relation::Equal, byte, CARRIAGE_RETURN);
        self.b.raw_binary(Binary::Or, line_feed, carriage_return)
    }

    /// Continues where `condition` holds or `already` did; fails otherwise.
    fn pass_or(&mut self, already: ir::Value, check: impl FnOnce(&mut Self) -> ir::Value) {
        let (done, probe) = (self.b.new_block(), self.b.new_block());
        self.branch(already, done, probe);
        self.b.enter_open(probe);
        let holds = check(self);
        self.branch(holds, done, self.backtrack);
        self.b.enter_open(done);
    }

    fn input_start(&mut self, multiline: bool) {
        let position = self.position();
        let at_start = self.compare_constant(Relation::Equal, position, 0);
        if !multiline {
            return self.require(at_start);
        }
        self.pass_or(at_start, |emit| {
            let before = emit.b.raw_binary_imm(Binary::Add, position, -1);
            emit.terminator_at(before)
        });
    }

    fn input_end(&mut self, multiline: bool) {
        let position = self.position();
        let at_end = self.compare(Relation::Equal, position, self.length);
        if !multiline {
            return self.require(at_end);
        }
        self.pass_or(at_end, |emit| emit.terminator_at(position));
    }

    /// `$` without the multiline flag in Python's sense: the end, or before one final newline.
    fn end_optional_newline(&mut self) {
        let position = self.position();
        let at_end = self.compare(Relation::Equal, position, self.length);
        self.pass_or(at_end, |emit| {
            let one = emit.b.raw_binary_imm(Binary::Add, position, 1);
            let two = emit.b.raw_binary_imm(Binary::Add, position, PAIR_END);
            let last = emit.compare(Relation::Equal, one, emit.length);
            let before_last = emit.compare(Relation::Equal, two, emit.length);
            let (single, pair, done) = (emit.b.new_block(), emit.b.new_block(), emit.b.new_block());
            let outcome = emit.b.block_parameter(done, Lane::I8);
            emit.branch(last, single, pair);
            emit.b.enter_open(single);
            let terminator = emit.terminator_at(position);
            emit.b.goto(done, &[terminator]);
            emit.b.enter_open(pair);
            let none = emit.b.raw_const(Lane::I8, 0);
            let crlf = emit.b.new_block();
            emit.b.branch_if(before_last, (crlf, &[]), (done, &[none]));
            emit.b.enter_open(crlf);
            let first = emit.byte_at(position);
            let second = emit.byte_at(one);
            let is_cr = emit.compare_constant(Relation::Equal, first, CARRIAGE_RETURN);
            let is_lf = emit.compare_constant(Relation::Equal, second, LINE_FEED);
            let both = emit.b.raw_binary(Binary::And, is_cr, is_lf);
            emit.b.goto(done, &[both]);
            emit.b.enter_open(done);
            outcome
        });
    }

    /// Whether the byte is a word unit, `[A-Za-z0-9_]`.
    fn is_word(&mut self, byte: ir::Value) -> ir::Value {
        let lowered = self.b.raw_binary_imm(Binary::Or, byte, CASE_BIT);
        let letter = self.b.raw_binary_imm(Binary::Add, lowered, -i64::from(b'a'));
        let letter = self.compare_constant(Relation::UnsignedLess, letter, LETTERS);
        let digit = self.b.raw_binary_imm(Binary::Add, byte, -i64::from(b'0'));
        let digit = self.compare_constant(Relation::UnsignedLess, digit, DIGITS);
        let under = self.compare_constant(Relation::Equal, byte, i64::from(b'_'));
        let either = self.b.raw_binary(Binary::Or, letter, digit);
        self.b.raw_binary(Binary::Or, either, under)
    }

    /// Whether the unit at `index` is a word unit where `inside` holds, else false.
    fn word_where(&mut self, inside: ir::Value, index: ir::Value) -> ir::Value {
        let (load, done) = (self.b.new_block(), self.b.new_block());
        let outcome = self.b.block_parameter(done, Lane::I8);
        let none = self.b.raw_const(Lane::I8, 0);
        self.b.branch_if(inside, (load, &[]), (done, &[none]));
        self.b.enter_open(load);
        let byte = self.byte_at(index);
        let word = self.is_word(byte);
        self.b.goto(done, &[word]);
        self.b.enter_open(done);
        outcome
    }

    fn boundary(&mut self, expected: bool) {
        let position = self.position();
        let after_start = self.compare_constant(Relation::UnsignedGreater, position, 0);
        let before = self.b.raw_binary_imm(Binary::Add, position, -1);
        let left = self.word_where(after_start, before);
        let before_end = self.compare(Relation::UnsignedLess, position, self.length);
        let right = self.word_where(before_end, position);
        let differ = self.compare(if expected { Relation::NotEqual } else { Relation::Equal }, left, right);
        self.require(differ);
    }
}
