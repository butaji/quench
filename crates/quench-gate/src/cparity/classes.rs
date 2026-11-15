//! What an iteration of a hot loop spends beyond what the C program would: its instruction count
//! and, one count per residual class, the instructions that are the tier's own overhead.
//!
//! The classes are read from the machine code by mnemonic and operand. The NaN-box tag layout
//! (`0x7ff..` tag words, shifts that extract a tag) is the runtime's; a layout change that
//! moves it moves what `tag_tests` counts, which the ratchet then shows.

use std::ops::RangeInclusive;

use super::asm::Instruction;

/// The measure's first field: instructions per iteration.
pub const INSTRUCTIONS: &str = "instructions";
pub const CLASS_COUNT: usize = 7;
pub const FIELD_COUNT: usize = CLASS_COUNT + 1;
/// Instructions per iteration, then the count of each class in `CLASSES` order.
pub type Fields = [u32; FIELD_COUNT];

const SHIFT: &str = "shr";
/// The shift amounts, as operand prefixes, that extract a NaN-box tag from a value word.
const TAG_SHIFTS: [&str; 3] = ["$0x20,", "$0x30,", "$0x31,"];
const COMPARE_PREFIX: &str = "cmp";
/// The immediate prefix of the tag words a tag test compares against.
const TAG_WORD: &str = "$0x7ff";
const STACK_OPERANDS: [&str; 2] = ["(%rsp)", "(%rbp)"];
/// Loads of the interrupt flag, which a poll tests through a bare register.
const POLL_LOADS: [&str; 3] = ["movzbq", "movzbl", "cmpb"];
const MEMORY_OPEN: &str = "(%";
const MEMORY_CLOSE: char = ')';
const OPERAND_SEPARATOR: char = ',';
/// Read-modify-write arithmetic whose destination is memory is a counter kept in memory.
const COUNTER_OPS: [&str; 4] = ["add", "sub", "inc", "dec"];
const TRUNCATING_CONVERSION: &str = "cvttsd2si";
const VECTOR_PREFIX: char = 'v';
const OVERFLOW_OPS: [&str; 4] = ["jo", "jno", "seto", "setno"];

/// A residual class: what it is called in the ratchet and which instructions belong to it.
struct Class {
    name: &'static str,
    holds: fn(&Instruction, &RangeInclusive<u64>) -> bool,
}

static CLASSES: [Class; CLASS_COUNT] = [
    Class { name: "tag_tests", holds: tag_test },
    Class { name: "guards", holds: guard },
    Class { name: "spills", holds: spill },
    Class { name: "polls", holds: poll },
    Class { name: "counter_stores", holds: counter_store },
    Class { name: "cvt_round_trips", holds: conversion },
    Class { name: "overflow_checks", holds: overflow_check },
];

/// The name of each field of a measure, in order.
pub fn field_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(INSTRUCTIONS).chain(CLASSES.iter().map(|class| class.name))
}

/// A tag extraction (a shift by the tag position) or a compare against a tag word.
fn tag_test(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    let extraction = instruction.mnemonic() == SHIFT && TAG_SHIFTS.iter().any(|shift| instruction.operands().starts_with(shift));
    extraction || (instruction.mnemonic().starts_with(COMPARE_PREFIX) && instruction.operands().starts_with(TAG_WORD))
}

/// A conditional jump that leaves the loop, the loop's own test excluded.
fn guard(instruction: &Instruction, body: &RangeInclusive<u64>) -> bool {
    instruction.is_conditional_jump() && instruction.target().is_some_and(|target| !body.contains(&target))
}

/// Traffic through the frame: any operand addressed from the stack or frame pointer.
fn spill(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    STACK_OPERANDS.iter().any(|operand| instruction.operands().contains(operand))
}

/// Whether an operand list holds a memory operand with no displacement and no index.
fn bare_memory(operands: &str) -> bool {
    operands.match_indices(MEMORY_OPEN).any(|(at, _)| {
        let register = operands[at + MEMORY_OPEN.len()..].split(MEMORY_CLOSE).next().unwrap_or_default();
        (at == 0 || operands[..at].ends_with(OPERAND_SEPARATOR)) && !register.contains(OPERAND_SEPARATOR)
    })
}

/// A byte test of the interrupt flag through a bare register.
fn poll(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    POLL_LOADS.contains(&instruction.mnemonic()) && bare_memory(instruction.operands())
}

fn counter_store(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    COUNTER_OPS.iter().any(|op| instruction.mnemonic().starts_with(op)) && instruction.operands().ends_with(MEMORY_CLOSE)
}

/// A double-to-integer conversion: the first half of a round trip through binary64.
fn conversion(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    instruction.mnemonic().trim_start_matches(VECTOR_PREFIX) == TRUNCATING_CONVERSION
}

fn overflow_check(instruction: &Instruction, _: &RangeInclusive<u64>) -> bool {
    OVERFLOW_OPS.contains(&instruction.mnemonic())
}

/// The measure of one iteration through `body`, a loop's instructions in address order.
pub fn measure(body: &[Instruction]) -> Fields {
    let mut fields = [0; FIELD_COUNT];
    let Some((first, last)) = body.first().zip(body.last()) else {
        return fields;
    };
    let span = first.address..=last.address;
    let count = |holds: &dyn Fn(&Instruction) -> bool| u32::try_from(body.iter().filter(|&instruction| holds(instruction)).count()).unwrap_or(u32::MAX);
    fields[0] = count(&|instruction| !instruction.is_nop());
    for (field, class) in fields[1..].iter_mut().zip(&CLASSES) {
        *field = count(&|instruction| (class.holds)(instruction, &span));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cparity::asm::parse;

    #[test]
    fn each_class_counts_its_own_instructions() {
        let body = parse(
            "   0:\tshr    $0x20,%rcx\n   4:\tcmp    $0x7ffd0000,%ecx\n   a:\tjne    0x900\n   f:\tmov    0x20(%rsp),%rdx\n  14:\tmovzbq (%rdx),%rax\n  18:\tadd    %rsi,(%rax)\n  1c:\tvcvttsd2si %xmm8,%ecx\n  20:\tjno    0x0\n  24:\tnopw   0x0(%rax,%rax,1)\n  2b:\tjl     0x0\n",
        );
        let fields = measure(&body);
        assert_eq!(fields[0], 9);
        let named: Vec<(&str, u32)> = field_names().zip(fields).skip(1).collect();
        assert_eq!(
            named,
            [("tag_tests", 2), ("guards", 1), ("spills", 1), ("polls", 1), ("counter_stores", 1), ("cvt_round_trips", 1), ("overflow_checks", 1)]
        );
    }

    #[test]
    fn a_memory_operand_with_an_index_or_displacement_is_not_bare() {
        assert!(bare_memory("(%rdx),%rax") && bare_memory("$0x0,(%rax)"));
        assert!(!bare_memory("(%rcx,%rax,1),%rdx") && !bare_memory("0x14(%rdx),%rax"));
    }
}
