use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn profile_regional_binary(
        &mut self,
        frame: usize,
        pc: usize,
        operator: u32,
        left: Value,
        right: Value,
    ) {
        let fast = self.numeric_binary(operator, left, right).is_some();
        self.profile
            .regional_binary(self.frames[frame].function, pc as u32, fast);
    }
}
