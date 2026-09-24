use super::*;

const ARRAY_CONSTRUCTOR_LENGTH: f64 = 1.0;

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
            ("copyWithin", Native::ArrayCopyWithin),
            ("with", Native::ArrayWith),
            ("forEach", Native::ArrayForEach),
            ("map", Native::ArrayMap),
            ("filter", Native::ArrayFilter),
            ("some", Native::ArraySome),
            ("every", Native::ArrayEvery),
            ("find", Native::ArrayFind),
            ("findIndex", Native::ArrayFindIndex),
            ("findLast", Native::ArrayFindLast),
            ("findLastIndex", Native::ArrayFindLastIndex),
            ("group", Native::ArrayGroup),
            ("groupToMap", Native::ArrayGroupToMap),
            ("flatMap", Native::ArrayFlatMap),
            ("reduce", Native::ArrayReduce),
            ("reduceRight", Native::ArrayReduceRight),
            ("toReversed", Native::ArrayToReversed),
            ("toSpliced", Native::ArrayToSpliced),
            ("sort", Native::ArraySort),
            ("toSorted", Native::ArrayToSorted),
            ("toString", Native::ArrayToString),
            ("toLocaleString", Native::ArrayToString),
            ("keys", Native::ArrayKeys),
            ("values", Native::ArrayValues),
            ("entries", Native::ArrayEntries),
        ] {
            self.set_builtin_named(program, self.array_proto, name, native)?;
        }
        self.set_builtin_named(program, self.array_proto, "constructor", Native::Array)?;
        self.set_named(
            program,
            array,
            "length",
            Value::number(ARRAY_CONSTRUCTOR_LENGTH),
        )?;
        self.set_named(program, array, "prototype", self.array_proto)?;
        self.set_builtin_named(program, array, "from", Native::ArrayFrom)?;
        self.set_builtin_named(program, array, "of", Native::ArrayOf)?;
        self.set_builtin_named(program, array, "isArray", Native::ArrayIsArray)?;
        self.global(program, "Array", array)
    }
}
