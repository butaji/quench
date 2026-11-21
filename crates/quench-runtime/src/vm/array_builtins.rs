use super::*;

const ARRAY_CONSTRUCTOR_LENGTH: f64 = 1.0;
/// `Array.prototype`'s methods and `Array`'s static methods, in installation order: derived
/// from the rows of `native_rows`.
const ARRAY_PROTOTYPE_METHODS: &[(&str, Native)] = super::native_rows::ARRAY_PROTOTYPE_METHODS;
const ARRAY_STATIC_METHODS: &[(&str, Native)] = super::native_rows::ARRAY_STATIC_METHODS;

impl Vm {
    pub(super) fn install_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        self.array_proto = self.heap.alloc_array(crate::heap::ArrayObject::new(
                Self::empty_object(self.object_proto),
                Vec::new(),
            ));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, Native::Array), self.array_proto);
        self.set_builtin_function_name(array, "Array")?;
        for &(name, native) in ARRAY_PROTOTYPE_METHODS {
            self.set_builtin_named(program, self.array_proto, name, native)?;
        }
        self.set_builtin_named(program, self.array_proto, "constructor", Native::Array)?;
        self.set_builtin_value_named(array, "length", Value::number(ARRAY_CONSTRUCTOR_LENGTH))?;
        let length_atom = self.length_atom;
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
        let prototype_atom = self.prototype_atom();
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
        for &(name, native) in ARRAY_STATIC_METHODS {
            self.set_builtin_named(program, array, name, native)?;
        }
        self.global(program, "Array", array)
    }

    pub(super) fn install_array_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        let array = self.native_with_realm(Native::Array, global, global);
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Array), prototype);
        self.set_builtin_function_name(array, "Array")?;
        self.set_builtin_value_named(array, "prototype", prototype)?;
        self.set_builtin_value_named(prototype, "constructor", array)?;
        self.set_builtin_value_named(array, "length", Value::number(ARRAY_CONSTRUCTOR_LENGTH))?;
        let length = self.length_atom;
        self.set_property_attributes(
            array,
            PropertyKey::string(length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let prototype_atom = self.prototype_atom();
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
        for &(name, native) in ARRAY_PROTOTYPE_METHODS {
            self.set_realm_builtin_named(program, prototype, name, native, Some(global))?;
        }
        if let Some(iterator_symbol) = self.well_known_symbols.get(WellKnownSymbol::Iterator) {
            let values_atom = self.intern_atom("values");
            let values = self.get_property(program, prototype, values_atom)?;
            self.set_symbol_property(prototype, iterator_symbol, values)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(iterator_symbol),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        for &(name, native) in ARRAY_STATIC_METHODS {
            self.set_realm_builtin_named(program, array, name, native, Some(global))?;
        }
        self.install_species(array, |vm| vm.native_with_realm(Native::ArraySpecies, global, global))?;
        self.install_array_unscopables(prototype)?;
        self.set_builtin_value_named(global, "Array", array)
    }

    pub(super) fn install_array_species(&mut self) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        self.install_species(array, |vm| vm.native_value(Native::ArraySpecies))
    }

    pub(super) fn install_array_unscopables(&mut self, array_proto: Value) -> Result<(), JsError> {
        let unscopables = self
            .heap
            .alloc_object(Self::empty_object(Value::NULL));
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
        let symbol = self.well_known_symbols.get(WellKnownSymbol::Unscopables).unwrap();
        self.set_symbol_property(array_proto, symbol, unscopables)?;
        self.set_property_attributes(
            array_proto,
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
