//! The allocation rows whose operands the instruction alone fixes: an empty object, an object
//! literal over a register window or of two registers, a constant array, a closure, a literal
//! template and a string concatenation. Each makes a
//! heap value, so the frame's heap decides how (`Frame::allocate_value`): the interpreter
//! allocates in place, a specializer calls out to an allocator that leaves for a due
//! collection first.

#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};
use crate::bytecode::{Register, RegisterList, RegisterWindow};

/// What an allocation row makes. Every operand is static: the registers it reads are named by
/// a window, never passed as words, so each tier reads them from its own register homes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Allocation {
    /// An empty ordinary object (`Vm::object`).
    Empty,
    /// An array of `length` holes (`Vm::hole_array`); its elements, defined after it, are
    /// stores of their own. `numbers`: the literal's binding is declared an array of Numbers, so
    /// an empty one starts with a double backing (`Vm::make_array`); a tier that makes it as any
    /// other array is as exact, the backing turning double at its first Number that is no int32.
    Array { length: u32, numbers: bool },
    /// An array holding the values of `elements`, in order (`Vm::literal_array`): the literal
    /// whose `DefineArrayElement` records follow its `MakeArray` back to back, made at once as
    /// their definitions into its holes would leave it. A fresh array needs no write barrier.
    ArrayOf { elements: RegisterList },
    /// A fresh object of regexp literal `site`'s compiled template: the matcher program is the
    /// site's, its state is fresh. A pattern that fails to compile raises at the instruction.
    /// `callee`: the register holding the callee of the `replace`/`match`/`search`/`split` call
    /// the literal is the first argument of (`CreateRegExpLiteral`'s field B), read where the
    /// tier keeps it: while that callee is the realm's String method the site may answer its one
    /// shared object (`Vm::regexp_literal_argument_of`), keyed by the activation that evaluates it.
    RegExp { site: u32, callee: Option<Register> },
    /// The array of the `count` program constants from constant `constant`.
    ConstArray { constant: u32, count: u16 },
    /// An ordinary object of literal `site`'s shape holding registers `first` and `second`.
    ObjectPair { site: u32, first: Register, second: Register },
    /// An ordinary object of literal `site`'s shape holding the registers of `window`.
    ObjectLiteral { site: u32, window: RegisterWindow },
    /// The closure of function `function` over the activation's environment, which the first
    /// closure promotes (`Vm::promote_frame_environment`).
    Closure { function: u32 },
    /// The value literal template `template` builds from its steps.
    Template { template: u32 },
    /// ToString of each register of `window`, concatenated in order.
    Concat { window: RegisterWindow },
}

#[cfg(not(stencil_build))]
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

/// The empty allocations both JIT tiers make inline from a space's cursor and captured template.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlineEmpty {
    /// `Allocation::Empty`: an empty ordinary object.
    Object,
    /// `Allocation::Array { length: 0 }`: a zero-length array, whose elements are the static empty header.
    Array,
}

impl Allocation {
    /// The inline empty allocation this is, or `None`. The one predicate both tiers read: the u-jit's
    /// `allocate_value` and the b-jit planner (`base_region::steps`) decide an inline empty allocation by it.
    pub(crate) fn inline_empty(self) -> Option<InlineEmpty> {
        match self {
            Self::Empty => Some(InlineEmpty::Object),
            Self::Array { length: 0, .. } => Some(InlineEmpty::Array),
            _ => None,
        }
    }

    #[cfg(not(stencil_build))]
    /// The register `instruction` defines and the allocation it makes, given the instructions
    /// `following` it (an array literal's element definitions); `None` for an opcode that makes
    /// no such value.
    pub(crate) fn decode(instruction: WideInstruction, following: &[WideInstruction]) -> Option<(Register, Self)> {
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

    #[cfg(not(stencil_build))]
    /// The allocations whose value a table the function carries describes: a regular expression
    /// literal, a constant array and a literal template.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each allocation opcode decodes to its row with the operands the catalog fixes, and
    /// no other opcode decodes.
    #[test]
    fn allocation_opcodes_decode_to_their_rows() {
        let literal = WideInstruction::new(Op::MakeObjectLiteral, 4, 1, 3, 9);
        assert_eq!(Allocation::decode(literal, &[]), Some((4, Allocation::ObjectLiteral {
            site: 9, window: RegisterWindow { base: 1, count: 3 } })));
        let closure = WideInstruction::new(Op::MakeClosure, 2, 0, 0, 7);
        assert_eq!(Allocation::decode(closure, &[]), Some((2, Allocation::Closure { function: 7 })));
        let template = WideInstruction::new(Op::MakeLiteralTemplate, 5, 0, 0, 3);
        assert_eq!(Allocation::decode(template, &[]), Some((5, Allocation::Template { template: 3 })));
        let concat = WideInstruction::new(Op::StringConcat, 6, 2, 2, 0);
        assert_eq!(Allocation::decode(concat, &[]), Some((6, Allocation::Concat {
            window: RegisterWindow { base: 2, count: 2 } })));
        assert_eq!(Allocation::decode(WideInstruction::new(Op::Move, 1, 2, 0, 0), &[]), None);
    }

    #[test]
    fn the_empty_object_pair_and_constant_array_decode_to_rows() {
        let make = |op, b, c, imm| Allocation::decode(WideInstruction::new(op, 3, b, c, imm), &[]);
        assert_eq!(make(Op::MakeObject, 0, 0, 0), Some((3, Allocation::Empty)));
        assert_eq!(make(Op::MakeObject2, 1, 2, 5), Some((3, Allocation::ObjectPair { site: 5, first: 1, second: 2 })));
        assert_eq!(make(Op::MakeConstArray, 2, 0, 5), Some((3, Allocation::ConstArray { constant: 5, count: 2 })));
        assert_eq!(make(Op::MakeArray, 0, 0, 4), Some((3, Allocation::Array { length: 4, numbers: false })));
        assert_eq!(make(Op::CreateRegExpLiteral, 0, 0, 6), Some((3, Allocation::RegExp { site: 6, callee: None })));
        let argument = make(Op::CreateRegExpLiteral, 5 + crate::bytecode::OPTIONAL_REGISTER_BIAS, 0, 6);
        assert_eq!(argument, Some((3, Allocation::RegExp { site: 6, callee: Some(5) })), "field B names the call's callee");
        assert!(matches!(make(Op::StringConcat, 1, 1, 0), Some((3, Allocation::Concat { .. }))));
        assert!(make(Op::Jump, 0, 0, 0).is_none());
    }

    /// A `MakeArray` whose definitions follow it in order is the `ArrayOf` row over their
    /// registers, which need not be contiguous; a missing, misordered or self-referencing
    /// definition leaves the array of holes, and a literal past the window does too.
    #[test]
    fn an_array_literal_decodes_to_its_element_registers() {
        let make = |length| WideInstruction::new(Op::MakeArray, 4, 0, 0, length);
        let define = |value, index| WideInstruction::new(Op::DefineArrayElement, value, 4, 0, index);
        let rows = [define(9, 0), define(2, 1), define(7, 2)];
        let Some((4, Allocation::ArrayOf { elements })) = Allocation::decode(make(3), &rows) else { panic!("fused") };
        assert_eq!(elements.registers(), &[9, 2, 7]);
        let holes = Some((4, Allocation::Array { length: 3, numbers: false }));
        assert_eq!(Allocation::decode(make(3), &rows[..2]), holes, "a definition is missing");
        assert_eq!(Allocation::decode(make(3), &[rows[1], rows[0], rows[2]]), holes, "out of order");
        assert_eq!(Allocation::decode(make(3), &[rows[0], define(4, 1), rows[2]]), holes, "the array is its own element");
        assert_eq!(Allocation::decode(make(3), &[rows[0], WideInstruction::new(Op::DefineArrayElement, 2, 5, 0, 1), rows[2]]), holes,
            "another array's definition");
        assert_eq!(Allocation::decode(make(0), &rows), Some((4, Allocation::Array { length: 0, numbers: false })));
        let long: Vec<_> = (0..=crate::bytecode::ARRAY_LITERAL_WINDOW as u32).map(|index| define(index as u16 + 8, index)).collect();
        assert_eq!(Allocation::decode(make(long.len() as u32), &long),
            Some((4, Allocation::Array { length: long.len() as u32, numbers: false })), "past the window");
    }
}
