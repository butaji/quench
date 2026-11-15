//! Installing the RegExp constructor, its legacy accessors, and its prototype's properties.
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_regexp_intrinsics(
        &mut self,
        realm: Value,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        self.realm
            .intrinsics
            .regexp_intrinsics
            .insert(realm, RegExpIntrinsics::new(constructor, prototype));
        self.set_builtin_function_name(constructor, "RegExp")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            constructor,
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
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        let initial_constructor = self.native_value(Native::RegExp);
        for (name, native) in [
            ("compile", Native::RegExpCompile),
            ("exec", Native::RegExpExec),
            ("test", Native::RegExpTest),
            ("toString", Native::RegExpToString),
        ] {
            let method = if constructor == initial_constructor {
                self.native_value(native)
            } else {
                self.native_with_realm(native, realm, realm)
            };
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        Ok(())
    }

    pub(super) fn install_regexp(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for name in ["source", "flags", "lastIndex", "index", "input"] {
            self.intern_atom(name);
        }
        let constructor = self.native_value(Native::RegExp);
        self.regexp_proto = self.object();
        self.install_regexp_intrinsics(self.realm.globals, constructor, self.regexp_proto)?;
        self.set_builtin_named(program, constructor, "escape", Native::RegExpEscape)?;
        self.install_regexp_symbol_properties(constructor, self.regexp_proto, self.realm.globals)?;
        self.install_regexp_accessors(program, self.regexp_proto, self.realm.globals)?;
        self.install_regexp_legacy_accessors(program, constructor, self.realm.globals)?;
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn install_regexp_legacy_accessors(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        for capture in 1..=REGEXP_LEGACY_CAPTURE_COUNT {
            let name = format!("${capture}");
            self.install_regexp_legacy_accessor(program, constructor, realm, &name, false)?;
        }
        for (names, has_setter) in REGEXP_LEGACY_ACCESSOR_GROUPS {
            for name in *names {
                self.install_regexp_legacy_accessor(
                    program,
                    constructor,
                    realm,
                    name,
                    *has_setter,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn install_regexp_legacy_accessor(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        realm: Value,
        name: &str,
        has_setter: bool,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        let selector = Value::number(atom as f64);
        let getter = self.native_with_realm(Native::RegExpLegacyGetter, selector, realm);
        self.set_builtin_function_name(getter, &format!("get RegExp.{name}"))?;
        let setter =
            has_setter.then(|| self.native_with_realm(Native::RegExpLegacySetter, selector, realm));
        if let Some(setter) = setter {
            self.set_builtin_function_name(setter, &format!("set RegExp.{name}"))?;
        }
        self.set_named(program, constructor, name, getter)?;
        self.set_property_attributes(
            constructor,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter,
            },
        );
        Ok(())
    }

    pub(super) fn install_regexp_accessors(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        // Preserve the existing prototype property order: hasIndices was
        // installed after the older accessors. Flags lookup uses canonical order.
        let (has_indices, older_flags) = REGEXP_FLAG_ACCESSORS
            .split_first()
            .expect("RegExp flag metadata includes hasIndices");
        for (name, native, _) in older_flags.iter().chain(std::iter::once(has_indices)) {
            let getter = self.native_with_realm(*native, realm, realm);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_named(program, prototype, name, getter)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        for (name, native) in [
            ("source", Native::RegExpSource),
            ("flags", Native::RegExpFlags),
        ] {
            let getter = self.native_with_realm(native, realm, realm);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_named(program, prototype, name, getter)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        Ok(())
    }

    pub(super) fn install_regexp_symbol_properties(
        &mut self,
        constructor: Value,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Match) {
            let method = self.native_with_realm(Native::RegExpSymbolMatch, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.match]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Replace) {
            let method = self.native_with_realm(Native::RegExpSymbolReplace, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.replace]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Search) {
            let method = self.native_with_realm(Native::RegExpSymbolSearch, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.search]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Split) {
            let method = self.native_with_realm(Native::RegExpSymbolSplit, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.split]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::MatchAll) {
            let method = self.native_with_realm(Native::RegExpSymbolMatchAll, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.matchAll]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Species) {
            let getter = self.native_with_realm(Native::RegExpSpecies, realm, realm);
            self.set_builtin_function_name(getter, "get [Symbol.species]")?;
            self.set_symbol_property(constructor, symbol, Value::UNDEFINED)?;
            self.set_property_attributes(
                constructor,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        Ok(())
    }
}
