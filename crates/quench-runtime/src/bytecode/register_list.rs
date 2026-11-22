//! The registers a literal's elements were evaluated into: data a staged allocation row
//! carries, so the elements need not be contiguous.

use crate::bytecode::Register;

/// The most elements a literal evaluates into registers before its array is made, and so the
/// longest list a staged row holds (`RegisterList`).
pub const ARRAY_LITERAL_WINDOW: usize = 8;

/// Up to `ARRAY_LITERAL_WINDOW` registers, in element order, held inline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisterList {
    registers: [Register; ARRAY_LITERAL_WINDOW],
    count: u8,
}

impl RegisterList {
    /// The list of `count` registers `register` names by index; `None` past the window.
    pub(crate) fn collect(
        count: usize,
        mut register: impl FnMut(usize) -> Register,
    ) -> Option<Self> {
        let count = u8::try_from(count)
            .ok()
            .filter(|count| usize::from(*count) <= ARRAY_LITERAL_WINDOW)?;
        let mut registers = [0; ARRAY_LITERAL_WINDOW];
        for (index, slot) in registers.iter_mut().take(usize::from(count)).enumerate() {
            *slot = register(index);
        }
        Some(Self { registers, count })
    }

    /// The registers, in element order.
    pub(crate) fn registers(&self) -> &[Register] {
        &self.registers[..usize::from(self.count)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list keeps its registers in order, empty slots hold nothing, and one past the window
    /// is not a list.
    #[test]
    fn a_list_holds_its_registers_in_order_up_to_the_window() {
        let list = RegisterList::collect(3, |index| [9, 2, 5][index]).expect("three fit");
        assert_eq!(list.registers(), &[9, 2, 5]);
        let empty = RegisterList::collect(0, |_| 1).expect("none fit");
        assert!(empty.registers().is_empty());
        assert!(RegisterList::collect(ARRAY_LITERAL_WINDOW, |_| 0).is_some());
        assert!(RegisterList::collect(ARRAY_LITERAL_WINDOW + 1, |_| 0).is_none());
    }
}
