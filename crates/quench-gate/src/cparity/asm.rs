//! The machine code of a dumped region, disassembled by `objdump` into instructions the
//! analysis reads by mnemonic and operand.

use std::{path::Path, process::Command};

use crate::process;

const OBJDUMP: &str = "objdump";
/// Disassemble a raw binary as x86-64 in AT&T syntax, without the instruction bytes.
const OBJDUMP_ARGS: [&str; 7] = ["-D", "-b", "binary", "-mi386:x86-64", "-M", "att", "--no-show-raw-insn"];
const ADDRESS_RADIX: u32 = 16;
const ADDRESS_RADIX_PREFIX: &str = "0x";
const ADDRESS_END: char = ':';
const JUMP_INITIAL: char = 'j';
const UNCONDITIONAL_JUMP: &str = "jmp";
const NOP_PREFIX: &str = "nop";
/// Mnemonics after which execution does not continue with the next instruction.
const NO_FALL_THROUGH: [&str; 4] = [UNCONDITIONAL_JUMP, "ret", "retq", "ud2"];

#[derive(Clone, Debug)]
pub struct Instruction {
    pub address: u64,
    pub text: String,
}

impl Instruction {
    pub fn mnemonic(&self) -> &str {
        self.text.split_whitespace().next().unwrap_or_default()
    }

    pub fn operands(&self) -> &str {
        self.text.split_once(char::is_whitespace).map_or("", |(_, operands)| operands.trim())
    }

    pub fn is_nop(&self) -> bool {
        self.mnemonic().starts_with(NOP_PREFIX)
    }

    pub fn is_conditional_jump(&self) -> bool {
        self.mnemonic().starts_with(JUMP_INITIAL) && self.mnemonic() != UNCONDITIONAL_JUMP
    }

    pub fn falls_through(&self) -> bool {
        !NO_FALL_THROUGH.contains(&self.mnemonic())
    }

    /// The address a direct jump goes to.
    pub fn target(&self) -> Option<u64> {
        if !self.mnemonic().starts_with(JUMP_INITIAL) {
            return None;
        }
        let operand = self.operands().split_whitespace().next()?.strip_prefix(ADDRESS_RADIX_PREFIX)?;
        u64::from_str_radix(operand, ADDRESS_RADIX).ok()
    }
}

fn instruction(line: &str) -> Option<Instruction> {
    let (address, text) = line.trim().split_once(ADDRESS_END)?;
    let address = u64::from_str_radix(address, ADDRESS_RADIX).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| Instruction { address, text: text.to_owned() })
}

/// The instructions of an `objdump` listing, in address order; its headings are not instructions.
pub fn parse(listing: &str) -> Vec<Instruction> {
    listing.lines().filter_map(instruction).collect()
}

pub fn disassemble(file: &Path) -> Result<Vec<Instruction>, String> {
    let listing = process::run(Command::new(OBJDUMP).args(OBJDUMP_ARGS).arg(file))?;
    if listing.success { Ok(parse(&listing.stdout)) } else { Err(listing.excerpt()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\nx.bin:     file format binary\n\nDisassembly of section .data:\n\n0000000000000000 <.data>:\n   0:\tpush   %rbp\n 300:\tjl     0x2c0\n 31a:\tvmovsd (%rcx,%rax,8),%xmm1\n\t...\n";

    #[test]
    fn headings_are_not_instructions() {
        let code = parse(LISTING);
        assert_eq!(code.iter().map(|code| code.address).collect::<Vec<_>>(), [0, 0x300, 0x31a]);
        assert_eq!((code[1].mnemonic(), code[1].operands()), ("jl", "0x2c0"));
    }

    #[test]
    fn a_direct_jump_has_a_target_and_an_indirect_one_does_not() {
        let jump = |text: &str| Instruction { address: 0, text: text.to_owned() };
        assert_eq!(jump("jl     0x2c0").target(), Some(0x2c0));
        assert_eq!(jump("jmp    *%rax").target(), None);
        assert_eq!(jump("mov    %rax,%rbx").target(), None);
        assert!(jump("jne    0x10").is_conditional_jump() && !jump("jmp    0x10").is_conditional_jump());
        assert!(!jump("jmp    0x10").falls_through() && jump("jne    0x10").falls_through());
    }
}
