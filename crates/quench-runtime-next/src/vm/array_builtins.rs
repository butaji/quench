use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        self.array_proto = self.object();
        for (name, native) in [
            ("push", Native::ArrayPush),
            ("pop", Native::ArrayPop),
            ("slice", Native::ArraySlice),
            ("includes", Native::ArrayIncludes),
            ("join", Native::ArrayJoin),
            ("concat", Native::ArrayConcat),
            ("flat", Native::ArrayFlat),
            ("reverse", Native::ArrayReverse),
            ("shift", Native::ArrayShift),
            ("unshift", Native::ArrayUnshift),
            ("splice", Native::ArraySplice),
            ("fill", Native::ArrayFill),
            ("at", Native::ArrayAt),
            ("lastIndexOf", Native::ArrayLastIndexOf),
            ("indexOf", Native::ArrayIndexOf),
        ] {
            self.set_named(program, self.array_proto, name, self.native_value(native))?;
        }
        self.set_named(program, array, "prototype", self.array_proto)?;
        self.set_named(
            program,
            array,
            "isArray",
            self.native_value(Native::ArrayIsArray),
        )?;
        self.global(program, "Array", array)
    }
}
