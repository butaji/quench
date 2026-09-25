use super::*;

const ARRAY_CONSTRUCTOR_LENGTH: f64 = 1.0;

impl<H: Host> Vm<H> {
    pub(super) fn install_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        self.array_proto = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.object_proto),
            elements: Rc::new(Vec::new()),
        });
        self.set_builtin_function_name(array, "Array")?;
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
            ("toLocaleString", Native::ArrayToLocaleString),
            ("keys", Native::ArrayKeys),
            ("values", Native::ArrayValues),
            ("entries", Native::ArrayEntries),
        ] {
            self.set_builtin_named(program, self.array_proto, name, native)?;
        }
        self.set_builtin_named(program, self.array_proto, "constructor", Native::Array)?;
        self.set_builtin_value_named(array, "length", Value::number(ARRAY_CONSTRUCTOR_LENGTH))?;
        let length_atom = self.intern_atom("length");
        self.set_property_attributes(
            array,
            PropertyKey::string(length_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(array, "prototype", self.array_proto)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            array,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_named(program, array, "from", Native::ArrayFrom)?;
        self.set_builtin_named(program, array, "of", Native::ArrayOf)?;
        self.set_builtin_named(program, array, "isArray", Native::ArrayIsArray)?;
        self.global(program, "Array", array)
    }

    pub(super) fn install_array_species(&mut self) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        let species = self.well_known_symbols.get("species").copied().unwrap();
        let getter = self.native_value(Native::ArraySpecies);
        self.set_builtin_function_name(getter, "get [Symbol.species]")?;
        self.set_symbol_property(array, species, Value::UNDEFINED)?;
        self.set_property_attributes(
            array,
            PropertyKey::symbol(species),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn install_array_unscopables(&mut self) -> Result<(), JsError> {
        let unscopables = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        for name in [
            "at",
            "copyWithin",
            "entries",
            "fill",
            "find",
            "findIndex",
            "findLast",
            "findLastIndex",
            "flat",
            "flatMap",
            "includes",
            "keys",
            "values",
            "toReversed",
            "toSorted",
            "toSpliced",
        ] {
            let atom = self.intern_atom(name);
            self.set_property(unscopables, atom, Value::TRUE)?;
        }
        let symbol = self.well_known_symbols.get("unscopables").copied().unwrap();
        self.set_symbol_property(self.array_proto, symbol, unscopables)?;
        self.set_property_attributes(
            self.array_proto,
            PropertyKey::symbol(symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }
}
