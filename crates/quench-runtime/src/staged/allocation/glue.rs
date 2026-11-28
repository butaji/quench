//! The runtime decoders of the allocation rows: `Allocation::decode` and its site-backed arm,
//! over instruction words. Moved out of `allocation.rs` (runtime-only names: `Op`, `WideInstruction`).

#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, WideInstruction};

/// The element registers of the array literal `make` makes when the `following` instructions
/// begin with its definitions, one per index in order and none of the array itself; `None` for
/// an array of holes, a literal past the window, or one whose definitions do not follow.
fn literal_elements(make: WideInstruction, following: &[WideInstruction]) -> Option<RegisterList> {
    let (array, length) = (make.result_register(), make.array_length());
    let defines = following.get(..length).filter(|defines| !defines.is_empty())?;
    let ordered = defines.iter().enumerate().all(|(index, define)| define.op().is_define_array_element()
        && define.register_b() == array && define.register_a() != array && define.imm() as usize == index);
    if !ordered { return None; }
    RegisterList::collect(length, |index| defines[index].register_a())
}

/// The decoders of the allocation rows over the instruction words, implemented for `Allocation`.
pub trait AllocationAllocationExt: Sized {
    /// The register `instruction` defines and the allocation it makes, given the instructions
    /// `following` it (an array literal's element definitions); `None` for an opcode that makes
    /// no such value.
    fn decode(instruction: WideInstruction, following: &[WideInstruction]) -> Option<(Register, Self)>;
    /// The allocations whose value a table the function carries describes: a regular expression
    /// literal, a constant array and a literal template.
    fn decode_site_backed(instruction: WideInstruction) -> Option<Self>;
}

impl AllocationAllocationExt for Allocation {
    fn decode(instruction: WideInstruction, following: &[WideInstruction]) -> Option<(Register, Self)> {
        let allocation = match instruction.op() {
            Op::MakeObject => Self::Empty,
            Op::MakeArray => match literal_elements(instruction, following) {
                Some(elements) => Self::ArrayOf { elements },
                None => Self::Array { length: u32::try_from(instruction.array_length()).ok()?,
                    numbers: instruction.boolean_field(crate::bytecode::InstructionField::B)? },
            },
            Op::MakeObject2 => Self::ObjectPair { site: u32::try_from(instruction.object_site_index()).ok()?,
                first: instruction.register_b(), second: instruction.register_c() },
            Op::MakeObjectLiteral => Self::ObjectLiteral {
                site: u32::try_from(instruction.object_site_index()).ok()?, window: instruction.register_window() },
            Op::MakeClosure => Self::Closure { function: instruction.closure_function_index() },
            Op::StringConcat => Self::Concat { window: instruction.register_window() },
            _ => Self::decode_site_backed(instruction)?,
        };
        Some((instruction.result_register(), allocation))
    }

    fn decode_site_backed(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::CreateRegExpLiteral => Self::RegExp {
                site: u32::try_from(instruction.regexp_literal_site_index()).ok()?, callee: instruction.optional_register_b() },
            Op::MakeConstArray => Self::ConstArray {
                constant: u32::try_from(instruction.constant_index()).ok()?, count: instruction.element_count() },
            Op::MakeLiteralTemplate => Self::Template {
                template: u32::try_from(instruction.literal_template_index()).ok()? },
            _ => return None,
        })
    }
}
