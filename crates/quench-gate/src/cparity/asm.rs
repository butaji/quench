//! The machine code of a dumped region, disassembled by the host's disassembler into
//! instructions the analysis reads by mnemonic and operand.
//!
//! x86-64 reads `objdump -b binary`; AArch64 has no raw-binary mode there, so the bytes go to
//! `llvm-mc --disassemble` as hex, which prints one instruction per input and no addresses:
//! they are the byte offsets of the four-byte instructions this module counts.

use std::path::Path;
#[cfg(not(target_arch = "x86_64"))]
use std::{fmt::Write as _, io::Write as _, process::{Command, Stdio}};
#[cfg(target_arch = "x86_64")]
use std::process::Command;

#[cfg(target_arch = "x86_64")]
use crate::process;

#[cfg(target_arch = "x86_64")]
const OBJDUMP: &str = "objdump";
#[cfg(target_arch = "x86_64")]
/// Disassemble a raw binary as x86-64 in AT&T syntax, without the instruction bytes.
const OBJDUMP_ARGS: [&str; 7] = ["-D", "-b", "binary", "-mi386:x86-64", "-M", "att", "--no-show-raw-insn"];
#[cfg(not(target_arch = "x86_64"))]
const LLVM_MC: [&str; 2] = ["llvm-mc", "/opt/homebrew/opt/llvm/bin/llvm-mc"];
#[cfg(not(target_arch = "x86_64"))]
const LLVM_MC_ARGS: [&str; 2] = ["--disassemble", "--triple=arm64-apple-macos"];
/// The width of every AArch64 instruction, and so the stride between addresses.
#[cfg(not(target_arch = "x86_64"))]
const ARM64_INSTRUCTION_BYTES: u64 = 4;

const ADDRESS_RADIX: u32 = 16;
const ADDRESS_RADIX_PREFIX: &str = "0x";
const ADDRESS_END: char = ':';
/// Unconditional control transfers of either architecture: `jmp`, `b`; and the indirect or
/// returning forms, which no branch target follows.
const NO_FALL_THROUGH: [&str; 10] = ["jmp", "ret", "retq", "ud2", "b", "br", "ret", "brk", "udf", "hlt"];
/// The label of a branch's target in `objdump`'s listing and the prefix of `llvm-mc`'s byte
/// displacement.
const TARGET_PREFIX: char = '#';
/// AArch64's test-and-branch forms, conditional like a conditional `b`.
const ARM64_CONDITIONAL_JUMPS: [&str; 4] = ["cbz", "cbnz", "tbz", "tbnz"];

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
        self.mnemonic().starts_with("nop")
    }

    /// Whether the instruction is a conditional branch in either architecture's text: an x86
    /// `jcc` but `jmp`, an AArch64 `b.cond`, or one of its test-and-branch forms.
    pub fn is_conditional_jump(&self) -> bool {
        let mnemonic = self.mnemonic();
        (mnemonic.starts_with('j') && mnemonic != "jmp")
            || mnemonic.starts_with("b.")
            || ARM64_CONDITIONAL_JUMPS.contains(&mnemonic)
    }

    pub fn falls_through(&self) -> bool {
        !NO_FALL_THROUGH.contains(&self.mnemonic())
    }

    /// The address a direct branch goes to. `objdump` names it absolutely (`jl 0x2c0`);
    /// `llvm-mc` prints the byte displacement from the instruction (`b.eq #4`), which is what
    /// the AArch64 operand of `cbz w1, #16` also ends with.
    pub fn target(&self) -> Option<u64> {
        let mnemonic = self.mnemonic();
        if !(mnemonic.starts_with('j') || mnemonic == "b" || self.is_conditional_jump()) {
            return None;
        }
        let operand = self.operands().split([',', ' ']).filter(|field| !field.is_empty()).next_back()?;
        if let Some(displacement) = operand.strip_prefix(TARGET_PREFIX) {
            let displacement = displacement.strip_prefix("0x")
                .map_or_else(|| displacement.parse::<i64>().ok(), |hex| i64::from_str_radix(hex, ADDRESS_RADIX).ok())?;
            return Some((self.address as i64 + displacement) as u64);
        }
        let absolute = operand.strip_prefix(ADDRESS_RADIX_PREFIX)?;
        u64::from_str_radix(absolute, ADDRESS_RADIX).ok()
    }
}

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
fn instruction(line: &str) -> Option<Instruction> {
    let (address, text) = line.trim().split_once(ADDRESS_END)?;
    let address = u64::from_str_radix(address, ADDRESS_RADIX).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| Instruction { address, text: text.to_owned() })
}

/// The instructions of an `objdump` listing, in address order; its headings are not instructions.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub fn parse(listing: &str) -> Vec<Instruction> {
    listing.lines().filter_map(instruction).collect()
}

/// The instructions of an `llvm-mc --disassemble` listing, addressed by their byte offset: one
/// output line per four bytes of input. A word `llvm-mc` prints nothing for (a data word inside
/// the dump) becomes `.inst`, which is an instruction no branch or nop test matches.
#[cfg(not(target_arch = "x86_64"))]
fn parse_mc(listing: &str, instruction_count: usize) -> Result<Vec<Instruction>, String> {
    let lines: Vec<&str> = listing.lines().map(str::trim_end).filter(|line| !line.trim().is_empty()).collect();
    if lines.len() != instruction_count {
        return Err(format!("llvm-mc printed {} instructions for {instruction_count} encoded", lines.len()));
    }
    Ok(lines.into_iter().enumerate().map(|(index, text)| Instruction {
        address: index as u64 * ARM64_INSTRUCTION_BYTES,
        text: text.trim().to_owned(),
    }).collect())
}

/// `parse_mc`'s listing word by word, for a dump with a word `llvm-mc` prints nothing for: each
/// word is one instruction, an undecodable one named `.inst`.
#[cfg(not(target_arch = "x86_64"))]
fn parse_mc_words(words: &[[u8; ARM64_INSTRUCTION_BYTES as usize]]) -> Result<Vec<Instruction>, String> {
    Ok(words.iter().enumerate().map(|(index, word)| {
        let hex = format!("0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}", word[0], word[1], word[2], word[3]);
        let text = disassemble_words(&hex).ok().and_then(|listing| {
            listing.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
        });
        Instruction { address: index as u64 * ARM64_INSTRUCTION_BYTES, text: text.unwrap_or_else(|| ".inst".to_owned()) }
    }).collect())
}

/// `llvm-mc`'s listing of the hex `words` (four bytes a line).
#[cfg(not(target_arch = "x86_64"))]
fn disassemble_words(words: &str) -> Result<String, String> {
    let mut child = None;
    for candidate in LLVM_MC {
        match Command::new(candidate).args(LLVM_MC_ARGS)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        {
            Ok(spawned) => { child = Some(spawned); break; }
            Err(_) => continue,
        }
    }
    let mut child = child.ok_or("no llvm-mc to disassemble the dump")?;
    let mut stdin = child.stdin.take().ok_or("llvm-mc takes no input")?;
    stdin.write_all(words.as_bytes()).map_err(|error| error.to_string())?;
    drop(stdin);
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("llvm-mc: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn disassemble(file: &Path) -> Result<Vec<Instruction>, String> {
    #[cfg(target_arch = "x86_64")]
    {
        let listing = process::run(Command::new(OBJDUMP).args(OBJDUMP_ARGS).arg(file))?;
        if listing.success { Ok(parse(&listing.stdout)) } else { Err(listing.excerpt()) }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
        let count = bytes.len() / ARM64_INSTRUCTION_BYTES as usize;
        let mut hex = String::with_capacity(count * 20);
        for chunk in bytes.chunks_exact(ARM64_INSTRUCTION_BYTES as usize) {
            let _ = writeln!(hex, "0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}", chunk[0], chunk[1], chunk[2], chunk[3]);
        }
        let listing = disassemble_words(&hex);
        if let Ok(listing) = &listing
            && let Ok(code) = parse_mc(listing, count)
        {
            return Ok(code);
        }
        let words: Vec<[u8; ARM64_INSTRUCTION_BYTES as usize]> = bytes
            .chunks_exact(ARM64_INSTRUCTION_BYTES as usize)
            .map(|chunk| chunk.try_into().expect("four bytes"))
            .collect();
        parse_mc_words(&words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "x86_64")]
    const LISTING: &str = "\nx.bin:     file format binary\n\nDisassembly of section .data:\n\n0000000000000000 <.data>:\n   0:\tpush   %rbp\n 300:\tjl     0x2c0\n 31a:\tvmovsd (%rcx,%rax,8),%xmm1\n\t...\n";

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn headings_are_not_instructions() {
        let code = parse(LISTING);
        assert_eq!(code.iter().map(|code| code.address).collect::<Vec<_>>(), [0, 0x300, 0x31a]);
        assert_eq!((code[1].mnemonic(), code[1].operands()), ("jl", "0x2c0"));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_direct_jump_has_a_target_and_an_indirect_one_does_not() {
        let jump = |text: &str| Instruction { address: 0, text: text.to_owned() };
        assert_eq!(jump("jl     0x2c0").target(), Some(0x2c0));
        assert_eq!(jump("jmp    *%rax").target(), None);
        assert_eq!(jump("mov    %rax,%rbx").target(), None);
        assert!(jump("jne    0x10").is_conditional_jump() && !jump("jmp    0x10").is_conditional_jump());
        assert!(!jump("jmp    0x10").falls_through() && jump("jne    0x10").falls_through());
    }

    /// An AArch64 listing is addressed by byte offset, and a branch's `#` displacement is
    /// relative to the instruction: `b.cond`/`b` reach it, `br`/`ret` do not.
    #[cfg(not(target_arch = "x86_64"))]
    #[test]
    fn aarch64_instructions_take_their_offsets_and_relative_targets() {
        let listing = "\tmov\tw0, #1\n\tb.eq\t#8\n\tbr\tx0\n\tcbz\tw1, #16\n";
        let code = parse_mc(listing, 4).expect("four instructions");
        assert_eq!(code.iter().map(|code| code.address).collect::<Vec<_>>(), [0, 4, 8, 12]);
        assert_eq!(code[0].target(), None);
        assert_eq!(code[1].target(), Some(12), "b.eq at 4 with displacement 8");
        assert_eq!(code[2].target(), None);
        assert_eq!(code[3].target(), Some(28), "cbz at 12 with displacement 16");
        assert!(code[1].is_conditional_jump() && code[3].is_conditional_jump());
        assert!(code[1].falls_through() && !code[2].falls_through());
        assert!(parse_mc(listing, 3).is_err(), "a dropped instruction is not a measurement");
    }
}
