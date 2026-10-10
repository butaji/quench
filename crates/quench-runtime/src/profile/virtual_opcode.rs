use super::Profile;

impl Profile {
    #[inline(always)]
    pub(crate) fn virtual_opcode(&mut self, program: u32, opcode: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            super::increment_program_counter(&mut self.opcodes, program, opcode);
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (program, opcode);
    }
}
