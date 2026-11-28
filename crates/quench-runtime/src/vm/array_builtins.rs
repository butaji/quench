use super::*;
use super::array_modern::ArrayFromTarget;
use super::native_table::NativeFn;

/// The standalone entry of each `Array` native: the array family's own claim, in the method
/// group of `operations::native_entry`. Each native runs its body through the helper that
/// owns it; the callback, copy, flatten, group and iterator helpers take the native.
pub(super) fn entry(native: Native) -> Option<NativeFn> {
    match native {
        Native::ArrayIsArray => Some(array_is_array),
        Native::ArrayPush => Some(array_push),
        Native::ArrayPop => Some(array_pop),
        Native::ArraySlice => Some(array_slice),
        Native::ArrayIncludes => Some(array_includes),
        Native::ArrayJoin => Some(array_join),
        Native::ArrayConcat => Some(array_concat),
        Native::ArrayFlat | Native::ArrayFlatMap => Some(array_flatten),
        Native::ArrayReverse => Some(array_reverse),
        Native::ArrayShift => Some(array_shift),
        Native::ArrayUnshift => Some(array_unshift),
        Native::ArraySplice => Some(array_splice),
        Native::ArrayFill => Some(array_fill),
        Native::ArrayAt => Some(array_at),
        Native::ArrayLastIndexOf => Some(array_last_index_of),
        Native::ArrayIndexOf => Some(array_index_of),
        Native::ArrayCopyWithin => Some(array_copy_within),
        Native::ArrayWith | Native::ArrayToReversed | Native::ArrayToSpliced => Some(array_copy),
        Native::ArrayForEach
        | Native::ArrayMap
        | Native::ArrayFilter
        | Native::ArraySome
        | Native::ArrayEvery
        | Native::ArrayFind
        | Native::ArrayFindIndex
        | Native::ArrayFindLast
        | Native::ArrayFindLastIndex
        | Native::ArrayReduce
        | Native::ArrayReduceRight => Some(array_callback),
        Native::ArrayGroup | Native::ArrayGroupToMap => Some(array_group),
        Native::ArraySort => Some(array_sort_in_place),
        Native::ArrayToSorted => Some(array_sort_copy),
        Native::ArraySpecies => Some(array_species),
        Native::ArrayToString => Some(array_to_string),
        Native::ArrayToLocaleString => Some(array_to_locale_string),
        Native::ArrayKeys | Native::ArrayValues | Native::ArrayEntries => Some(array_iterator),
        Native::ArrayFrom => Some(array_from),
        Native::ArrayFromAsync => Some(array_from_async),
        Native::ArrayOf => Some(array_of),
        _ => None,
    }
}

fn array_is_array(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    let value = args.first().copied().unwrap_or(Value::UNDEFINED);
    Ok(if vm.is_array(p, value)? { Value::TRUE } else { Value::FALSE })
}

fn array_push(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_push_native(p, this, args)
}

fn array_pop(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_pop_native(p, this)
}

fn array_slice(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_slice_native(p, this, args)
}

fn array_includes(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_includes_native(p, this, args)
}

fn array_join(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_join_native(p, this, args)
}

fn array_concat(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_concat_native(p, this, args)
}

fn array_flatten(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_flatten_native(p, native, this, args)
}

fn array_reverse(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_reverse_native(p, this)
}

fn array_shift(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_shift_native(p, this)
}

fn array_unshift(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_unshift_native(p, this, args)
}

fn array_splice(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_splice_native(p, this, args)
}

fn array_fill(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_fill_native(p, this, args)
}

fn array_at(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_at_native(p, this, args)
}

fn array_last_index_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_last_index_of_native(p, this, args)
}

fn array_index_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_index_of_native(p, this, args)
}

fn array_copy_within(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_copy_within_native(p, this, args)
}

fn array_copy(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_copy_native(p, native, this, args)
}

fn array_callback(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_callback_native(p, native, this, args)
}

fn array_group(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_group_native(p, native, this, args)
}

fn array_sort_in_place(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_sort_native(p, this, args, true)
}

fn array_sort_copy(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_sort_native(p, this, args, false)
}

fn array_species(_vm: &mut Vm, _p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Ok(this)
}

fn array_to_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_to_string_native(p, this)
}

fn array_to_locale_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_to_locale_string_native(p, this, args)
}

fn array_iterator(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.array_iterator_native(p, native, this)
}

fn array_from(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_from_native(p, this, args, ArrayFromTarget::Array)
}

fn array_from_async(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_from_async_native(p, this, args)
}

fn array_of(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_of_native(p, this, args)
}

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
