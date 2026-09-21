use super::Profile;

impl Profile {
    #[inline(always)]
    pub(crate) fn virtual_opcode(&mut self, opcode: usize) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.opcodes[opcode] = self.opcodes[opcode].saturating_add(1);
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = opcode;
    }
}
