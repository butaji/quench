//! Composite call row of the iteration call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! An iteration step is a `Call` whose result feeds `RequireIteratorResult`: a generator's or an
//! iterator's `next` (`Vm::iteration_step`). What the step's callee does (resume a generator over
//! its record, step an array or a collection) is heap work no row makes; what the call protocol
//! makes of it is a row:
//!
//! - admission of a resumed generator (`admit_resume`): the step's result register is no
//!   returning one (`resumes_at_site`, a site operand) and the activations hold one more
//!   (`call::admit_room`, the one compare every admission is made of);
//! - the step's reads (`reads_step`): the records after the step read its `done` and its
//!   `value`, so the step may hand both to the registers those reads write;
//! - the step's delivery (`step_yielded`, `step_finished`): the `done` word, and the value of a
//!   step that yielded one, into those registers, with no result object.
//!
//! The interpreter runs them on `Concrete` (`dispatch_fast::iterator_step_reads_miss`,
//! `iterator_step_binding_miss`, `generator_windows::lane_resume_generator`); every miss is the
//! handler's own general step (`iterator_step_miss`, `generator_step_miss`, the native call).

use super::call::admit_room;
use super::region::Region;
use super::{word_bytes, Stage};
use crate::bytecode::{Register, RETURN_REGISTER};
use crate::Value;

/// The atoms the records after a step read of its result: `done` and `value`
/// (`Vm::runtime_atoms`), as raw words a tier compares the reads' operands against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepAtoms {
    pub(crate) done: u64,
    pub(crate) value: u64,
}

/// The registers a delivered step writes: the one its `done` read writes, the one its `value`
/// read writes, and for a destructuring step (`lane_binding_step`) the pattern's done flag the
/// record after the `done` read moves it to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepReads {
    pub(crate) done: Register,
    pub(crate) value: Register,
    pub(crate) flag: Option<Register>,
}

/// Whether a step whose result goes to register `result` (the call record's A operand) may
/// resume its generator as an activation: a returning call completes its caller, which a
/// generator's step does not. A site operand, so a tier decides it at decode, no row.
#[inline(always)]
pub const fn resumes_at_site(result: Register) -> bool {
    result & RETURN_REGISTER == 0
}

/// Admits a resumed generator's activation at index `depth` (the activations' count) within
/// `limit` (the lane stack's activation limit): `depth + 1` fits within `limit`, the one
/// compare of `call::admit_room`. Yields `depth`, the new activation's index; leaves at `miss`
/// where the activations are full. Reads and writes no memory.
#[inline(always)]
pub(crate) fn admit_resume<S: Stage>(stage: &mut S, depth: S::Word, limit: S::Word, miss: S::Miss) -> Option<S::Word> {
    let needed = stage.add_imm(depth, 1u64);
    admit_room(stage, limit, needed, miss)?;
    Some(depth)
}

/// `admit_resume` on machine words, for the interpreter's own resume of a generator.
#[inline(always)]
pub fn concrete_admit_resume(depth: usize, limit: usize) -> Option<usize> {
    admit_resume(&mut super::Concrete::new(std::ptr::null()), depth as u64, limit as u64, ()).map(|depth| depth as usize)
}

/// Whether the reads after a step, whose operands are the atoms `done` and `value`, read the
/// step's `done` and `value` (`atoms`): only then may the step skip its result object. A
/// condition, not a guard: a generator's step resumes whatever its reads are. Reads no memory.
#[inline(always)]
pub(crate) fn reads_step<S: Stage>(stage: &mut S, done: S::Word, value: S::Word, atoms: StepAtoms) -> S::Cond {
    let done = stage.eq_imm(done, atoms.done);
    let value = stage.eq_imm(value, atoms.value);
    stage.all(done, value)
}

/// `reads_step` on machine words: the reads' atom operands against the realm's atoms.
#[inline(always)]
pub fn concrete_reads_step(done: u32, value: u32, atoms: StepAtoms) -> bool {
    reads_step(&mut super::Concrete::new(std::ptr::null()), u64::from(done), u64::from(value), atoms)
}

/// Writes the done word `done` to the step's done register and, for a destructuring step, its
/// flag, in the window at `registers` (`Region::Frame`).
#[inline(always)]
fn write_done<S: Stage>(stage: &mut S, registers: S::Word, reads: StepReads, done: S::Word) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, registers, word_bytes(reads.done) as i32, done);
    if let Some(flag) = reads.flag {
        stage.store_u64(at, registers, word_bytes(flag) as i32, done);
    }
}

/// A step that yielded `value`: writes `false` to its done register (and flag) and `value` to
/// its value register in the window at `registers`. No result object is made: only those reads
/// would have observed it. The tier continues past the step's last read.
#[inline(always)]
pub(crate) fn step_yielded<S: Stage>(stage: &mut S, registers: S::Word, reads: StepReads, value: S::Word) {
    let done = stage.constant(Value::FALSE.to_bits());
    write_done(stage, registers, reads, done);
    stage.store_u64(Region::Frame.mutable(), registers, word_bytes(reads.value) as i32, value);
}

/// A finished step: writes `true` to its done register (and flag) in the window at
/// `registers`; the value register keeps its word. Yields the done word. The tier continues at
/// the step's exit, the record that tests the `done` just written.
#[inline(always)]
pub(crate) fn step_finished<S: Stage>(stage: &mut S, registers: S::Word, reads: StepReads) -> S::Word {
    let done = stage.constant(Value::TRUE.to_bits());
    write_done(stage, registers, reads, done);
    done
}

/// `step_yielded` on the interpreter's window `registers` (register 0's address).
#[inline(always)]
pub fn concrete_step_yielded(registers: *mut Value, reads: StepReads, value: Value) {
    step_yielded(&mut super::Concrete::new(std::ptr::null()), registers.expose_provenance() as u64, reads, value.to_bits());
}

/// `step_finished` on the interpreter's window `registers` (register 0's address).
#[inline(always)]
pub fn concrete_step_finished(registers: *mut Value, reads: StepReads) {
    step_finished(&mut super::Concrete::new(std::ptr::null()), registers.expose_provenance() as u64, reads);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATOMS: StepAtoms = StepAtoms { done: 3, value: 4 };

    #[test]
    fn a_resume_is_admitted_below_the_limit_and_not_for_a_returning_call() {
        assert_eq!(concrete_admit_resume(0, 1), Some(0));
        assert_eq!(concrete_admit_resume(1, 1), None);
        assert!(resumes_at_site(5) && !resumes_at_site(5 | RETURN_REGISTER));
    }

    #[test]
    fn the_reads_must_be_done_then_value() {
        assert!(concrete_reads_step(3, 4, ATOMS));
        assert!(!concrete_reads_step(4, 3, ATOMS));
    }

    #[test]
    fn a_delivery_writes_done_and_flag_and_the_value_only_once_yielded() {
        let marker = Value::integer(77);
        let mut window = [marker; 4];
        let reads = StepReads { done: 0, value: 2, flag: Some(1) };
        concrete_step_yielded(window.as_mut_ptr(), reads, Value::integer(9));
        assert_eq!(window, [Value::FALSE, Value::FALSE, Value::integer(9), marker]);
        let mut window = [marker; 4];
        concrete_step_finished(window.as_mut_ptr(), StepReads { flag: None, ..reads });
        assert_eq!(window, [Value::TRUE, marker, marker, marker], "a finished step leaves the value register");
    }
}
