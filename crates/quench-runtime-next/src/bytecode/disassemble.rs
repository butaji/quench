use super::ResidualProgram;
use std::fmt::Write;

impl ResidualProgram {
    pub fn disassemble(&self) -> String {
        let mut output = String::new();
        for (id, function) in self.functions.iter().enumerate() {
            let name = function
                .name
                .map(|atom| self.atoms[atom as usize].as_ref())
                .unwrap_or("<anonymous>");
            let _ = writeln!(
                output,
                "function {id} {name} params={} locals={} registers={} dispatch={:?}",
                function.params, function.locals, function.registers, function.dispatch
            );
            for (pc, instruction) in function.code.iter().enumerate() {
                let _ = writeln!(output, "  {pc:04} {instruction:?}");
            }
        }
        for (id, site) in self.superinstructions.iter().enumerate() {
            let _ = writeln!(output, "superinstruction {id} {:?}", site.code);
        }
        output
    }
}
