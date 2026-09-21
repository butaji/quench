use super::*;
#[rustfmt::skip]
const NATIVES: &[Native] = &[
    Native::Print, Native::Object,
    Native::ObjectKeys, Native::ObjectValues, Native::ObjectEntries, Native::ObjectGetOwnPropertyNames,
    Native::ObjectFromEntries, Native::ObjectIs,
    Native::ObjectCreate, Native::ObjectAssign, Native::ObjectGetPrototypeOf,
    Native::ObjectSetPrototypeOf, Native::ObjectHasOwn,
    Native::ObjectPrototypeHasOwnProperty, Native::ObjectPrototypePropertyIsEnumerable, Native::ObjectPrototypeIsPrototypeOf,
    Native::ReflectGet,
    Native::ReflectSet,
    Native::ReflectOwnKeys,
    Native::ReflectGetPrototypeOf,
    Native::ReflectSetPrototypeOf,
    Native::ReflectConstruct,
    Native::JsonParse,
    Native::JsonStringify,
    Native::Array,
    Native::ArrayIsArray,
    Native::ArrayPush,
    Native::ArrayPop,
    Native::ArraySlice,
    Native::ArrayIncludes,
    Native::ArrayJoin,
    Native::ArrayConcat,
    Native::ArrayFlat,
    Native::ArrayReverse,
    Native::ArrayShift,
    Native::ArrayUnshift,
    Native::ArraySplice,
    Native::ArrayFill,
    Native::ArrayAt,
    Native::ArrayLastIndexOf,
    Native::ArrayIndexOf,
    Native::ArrayCopyWithin,
    Native::ArrayWith,
    Native::ArrayForEach,
    Native::ArrayMap,
    Native::ArrayFilter,
    Native::ArraySome,
    Native::ArrayEvery,
    Native::ArrayFind,
    Native::ArrayFindIndex,
    Native::ArrayFindLast,
    Native::ArrayFindLastIndex,
    Native::ArrayGroup,
    Native::ArrayGroupToMap,
    Native::ArrayFlatMap,
    Native::ArrayReduce,
    Native::ArrayReduceRight,
    Native::ArrayToReversed,
    Native::ArrayToSpliced,
    Native::ArraySort,
    Native::ArrayToSorted,
    Native::ArrayToString,
    Native::ArrayKeys,
    Native::ArrayValues,
    Native::ArrayEntries,
    Native::ArrayFrom,
    Native::ArrayOf,
    Native::ArrayBuffer,
    Native::ArrayBufferSlice,
    Native::ArrayBufferTransfer,
    Native::ArrayBufferResize,
    Native::ArrayBufferTransferToFixedLength,
    Native::ArrayBufferIsView,
    Native::SharedArrayBuffer,
    Native::SharedArrayBufferGrow,
    Native::AtomicsLoad,
    Native::AtomicsStore,
    Native::AtomicsAdd,
    Native::AtomicsSub,
    Native::AtomicsAnd,
    Native::AtomicsOr,
    Native::AtomicsXor,
    Native::AtomicsExchange,
    Native::AtomicsCompareExchange,
    Native::AtomicsIsLockFree,
    Native::Uint8Array,
    Native::Uint8ClampedArray,
    Native::Uint16Array,
    Native::Uint32Array,
    Native::Int8Array,
    Native::Int16Array,
    Native::Int32Array,
    Native::Float32Array,
    Native::Float64Array,
    Native::Uint8ArraySet,
    Native::Uint8ArrayReverse,
    Native::Uint8ArrayFill,
    Native::Uint8ArrayCopyWithin,
    Native::Uint8ArraySubarray,
    Native::Uint8ArraySlice,
    Native::Uint8ArrayIncludes,
    Native::Uint8ArrayIndexOf,
    Native::Uint8ArrayJoin,
    Native::Uint8ArrayToString,
    Native::Uint8ArrayKeys,
    Native::Uint8ArrayValues,
    Native::Uint8ArrayEntries,
    Native::DataView,
    Native::DataViewGetUint8,
    Native::DataViewSetUint8,
    Native::DataViewGetInt8,
    Native::DataViewSetInt8,
    Native::DataViewGetUint16,
    Native::DataViewSetUint16,
    Native::DataViewGetInt16,
    Native::DataViewSetInt16,
    Native::DataViewGetUint32,
    Native::DataViewSetUint32,
    Native::DataViewGetInt32,
    Native::DataViewSetInt32,
    Native::DataViewGetFloat32,
    Native::DataViewSetFloat32,
    Native::DataViewGetFloat64,
    Native::DataViewSetFloat64,
    Native::Map,
    Native::MapGet,
    Native::MapSet,
    Native::MapHas,
    Native::MapDelete,
    Native::MapClear,
    Native::MapKeys,
    Native::MapValues,
    Native::MapEntries,
    Native::MapForEach,
    Native::Set,
    Native::SetAdd,
    Native::SetHas,
    Native::SetDelete,
    Native::SetClear,
    Native::SetKeys,
    Native::SetValues,
    Native::SetEntries,
    Native::SetForEach,
    Native::IteratorNext,
    Native::WeakMap,
    Native::WeakMapGet,
    Native::WeakMapSet,
    Native::WeakMapHas,
    Native::WeakMapDelete,
    Native::WeakSet,
    Native::WeakSetAdd,
    Native::WeakSetHas,
    Native::WeakSetDelete,
    Native::WeakRef,
    Native::WeakRefDeref,
    Native::FunctionCall,
    Native::FunctionApply,
    Native::Date, Native::DateNow, Native::DateGetTime, Native::DateValueOf, Native::DateToISOString, Native::DateToJSON, Native::DateParse, Native::DateUTC,
    Native::Error,
    Native::RegExp,
    Native::RegExpExec,
    Native::RegExpTest,
    Native::String,
    Native::Symbol,
    Native::SymbolFor,
    Native::SymbolKeyFor,
    Native::StringCharCodeAt,
    Native::StringCharAt,
    Native::StringSubstring,
    Native::StringSubstr,
    Native::StringIncludes,
    Native::StringStartsWith,
    Native::StringEndsWith,
    Native::StringIndexOf, Native::StringLastIndexOf, Native::StringToString, Native::StringValueOf,
    Native::StringReplace, Native::StringSplit, Native::StringTrim, Native::StringTrimStart,
    Native::StringTrimEnd,
    Native::StringRepeat,
    Native::StringPadStart,
    Native::StringPadEnd,
    Native::StringMatch,
    Native::StringSearch,
    Native::StringReplaceAll,
    Native::StringAt, Native::StringCodePointAt, Native::StringToUpperCase, Native::StringToLowerCase, Native::StringConcat, Native::StringNormalize,
    Native::EncodeUri,
    Native::EncodeUriComponent,
    Native::DecodeUri,
    Native::DecodeUriComponent,
    Native::StringFromCharCode,
    Native::ParseInt,
    Native::MathLog,
    Native::MathPow,
    Native::MathFloor,
    Native::MathMin,
    Native::MathMax,
    Native::MathRandom,
    Native::MathAbs, Native::MathCeil, Native::MathRound, Native::MathTrunc,
    Native::MathSqrt, Native::MathSign,
    Native::NumberString,
    Native::Number,
    Native::NumberIsNaN,
    Native::NumberIsFinite,
    Native::NumberIsInteger,
    Native::NumberIsSafeInteger,
    Native::NumberParseFloat,
    Native::NumberFixed,
    Native::NumberPrecision,
];
impl<H: Host> Vm<H> {
    pub(super) fn install_builtins(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.install_prototypes();
        for native in NATIVES {
            let value = self.native(*native);
            self.natives.push((*native, value));
        }
        self.install_object(program)?;
        self.install_console(program)?;
        self.install_array(program)?;
        self.install_array_buffer(program)?;
        self.install_typed_array(program)?;
        self.install_data_view(program)?;
        self.install_atomics(program)?;
        self.install_collections(program)?;
        self.install_weak_collections(program)?;
        self.install_iterators(program)?;
        self.global(program, "undefined", Value::UNDEFINED)?;
        self.global(program, "NaN", Value::number(f64::NAN))?;
        self.global(program, "Infinity", Value::number(f64::INFINITY))?;
        self.global(program, "print", self.native_value(Native::Print))?;
        self.install_date(program)?;
        self.global(program, "Error", self.native_value(Native::Error))?;
        self.install_regexp(program)?;
        let symbol = self.native_value(Native::Symbol);
        self.set_named(program, symbol, "for", self.native_value(Native::SymbolFor))?;
        self.set_named(
            program,
            symbol,
            "keyFor",
            self.native_value(Native::SymbolKeyFor),
        )?;
        self.global(program, "Symbol", symbol)?;
        let string = self.native_value(Native::String);
        self.set_named(
            program,
            string,
            "fromCharCode",
            self.native_value(Native::StringFromCharCode),
        )?;
        self.global(program, "String", string)?;
        self.global(program, "parseInt", self.native_value(Native::ParseInt))?;
        self.install_number(program)?;
        self.global(program, "encodeURI", self.native_value(Native::EncodeUri))?;
        self.global(
            program,
            "encodeURIComponent",
            self.native_value(Native::EncodeUriComponent),
        )?;
        self.global(program, "decodeURI", self.native_value(Native::DecodeUri))?;
        self.global(
            program,
            "decodeURIComponent",
            self.native_value(Native::DecodeUriComponent),
        )?;
        self.install_json(program)?;
        self.install_reflect(program)?;
        self.install_math(program)
    }
    fn install_prototypes(&mut self) {
        self.object_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.function_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.object_data_mut(self.globals).unwrap().proto = self.object_proto;
    }
    fn install_object(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let object = self.native_value(Native::Object);
        self.set_named(program, object, "prototype", self.object_proto)?;
        self.set_named(
            program,
            self.function_proto,
            "call",
            self.native_value(Native::FunctionCall),
        )?;
        self.set_named(
            program,
            self.function_proto,
            "apply",
            self.native_value(Native::FunctionApply),
        )?;
        self.set_named(
            program,
            object,
            "keys",
            self.native_value(Native::ObjectKeys),
        )?;
        self.install_object_extra(program, object)?;
        self.set_named(
            program,
            object,
            "create",
            self.native_value(Native::ObjectCreate),
        )?;
        self.set_named(
            program,
            object,
            "assign",
            self.native_value(Native::ObjectAssign),
        )?;
        self.set_named(
            program,
            object,
            "getPrototypeOf",
            self.native_value(Native::ObjectGetPrototypeOf),
        )?;
        self.set_named(
            program,
            object,
            "setPrototypeOf",
            self.native_value(Native::ObjectSetPrototypeOf),
        )?;
        self.set_named(
            program,
            object,
            "hasOwn",
            self.native_value(Native::ObjectHasOwn),
        )?;
        self.global(program, "Object", object)
    }
    fn install_console(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let console = self.object();
        self.set_named(program, console, "log", self.native_value(Native::Print))?;
        self.global(program, "console", console)
    }
    fn install_json(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let json = self.object();
        self.set_named(program, json, "parse", self.native_value(Native::JsonParse))?;
        self.set_named(
            program,
            json,
            "stringify",
            self.native_value(Native::JsonStringify),
        )?;
        self.global(program, "JSON", json)
    }
    fn install_reflect(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let reflect = self.object();
        self.set_named(
            program,
            reflect,
            "get",
            self.native_value(Native::ReflectGet),
        )?;
        self.set_named(
            program,
            reflect,
            "set",
            self.native_value(Native::ReflectSet),
        )?;
        self.set_named(
            program,
            reflect,
            "ownKeys",
            self.native_value(Native::ReflectOwnKeys),
        )?;
        self.set_named(
            program,
            reflect,
            "getPrototypeOf",
            self.native_value(Native::ReflectGetPrototypeOf),
        )?;
        self.set_named(
            program,
            reflect,
            "setPrototypeOf",
            self.native_value(Native::ReflectSetPrototypeOf),
        )?;
        self.set_named(
            program,
            reflect,
            "construct",
            self.native_value(Native::ReflectConstruct),
        )?;
        self.global(program, "Reflect", reflect)
    }
    fn install_math(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let math = self.object();
        self.set_named(program, math, "E", Value::number(std::f64::consts::E))?;
        self.set_named(program, math, "LN2", Value::number(std::f64::consts::LN_2))?;
        self.set_named(program, math, "log", self.native_value(Native::MathLog))?;
        self.set_named(program, math, "pow", self.native_value(Native::MathPow))?;
        self.set_named(program, math, "floor", self.native_value(Native::MathFloor))?;
        self.set_named(program, math, "min", self.native_value(Native::MathMin))?;
        self.set_named(program, math, "max", self.native_value(Native::MathMax))?;
        for (name, native) in [
            ("abs", Native::MathAbs),
            ("ceil", Native::MathCeil),
            ("round", Native::MathRound),
            ("trunc", Native::MathTrunc),
            ("sqrt", Native::MathSqrt),
            ("sign", Native::MathSign),
        ] {
            self.set_named(program, math, name, self.native_value(native))?;
        }
        self.set_named(
            program,
            math,
            "random",
            self.native_value(Native::MathRandom),
        )?;
        self.global(program, "Math", math)
    }
    pub(super) fn empty_object(proto: Value) -> Object {
        Object {
            proto,
            properties: ValueVec::new(),
        }
    }
    fn native(&mut self, kind: Native) -> Value {
        self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.function_proto)),
            kind: FunctionKind::Native(kind),
            env: Value::NULL,
        })
    }
    pub(super) fn native_value(&self, kind: Native) -> Value {
        self.natives
            .iter()
            .find(|(item, _)| *item == kind)
            .unwrap()
            .1
    }
    pub(super) fn object(&mut self) -> Value {
        self.heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)))
    }
    pub(super) fn lookup_atom(&self, name: &str) -> Option<Atom> {
        let hash = Self::atom_hash(name);
        let primary = self.atoms.get(&hash).copied()?;
        if self.atom_name(primary) == name {
            return Some(primary);
        }
        self.atom_collisions.get(&hash).and_then(|atoms| {
            atoms
                .iter()
                .copied()
                .find(|atom| self.atom_name(*atom) == name)
        })
    }
    pub(super) fn intern_atom(&mut self, name: &str) -> Atom {
        if let Some(atom) = self.lookup_atom(name) {
            return atom;
        }
        let atom = (self.atom_text.len() + self.dynamic_atoms.len()) as Atom;
        self.dynamic_atoms.push(Rc::from(name));
        self.index_atom(Self::atom_hash(name), atom);
        self.profile.dynamic_atom();
        atom
    }
    pub(super) fn atom_hash(name: &str) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        name.hash(&mut hasher);
        hasher.finish()
    }
    pub(super) fn index_atom(&mut self, hash: u64, atom: Atom) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.atoms.entry(hash) {
            entry.insert(atom);
        } else {
            self.atom_collisions.entry(hash).or_default().push(atom);
        }
    }
    pub(super) fn atom_name(&self, atom: Atom) -> &str {
        let index = atom as usize;
        if index < self.atom_text.len() {
            &self.atom_text[index]
        } else {
            &self.dynamic_atoms[index - self.atom_text.len()]
        }
    }
    pub(super) fn global(
        &mut self,
        _p: &ResidualProgram,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        if let Some(atom) = self.lookup_atom(name) {
            self.set_property(self.globals, atom, value)?;
        }
        Ok(())
    }
    pub(super) fn set_named(
        &mut self,
        _p: &ResidualProgram,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        if let Some(atom) = self.lookup_atom(name) {
            self.set_property(object, atom, value)?;
        }
        Ok(())
    }
}
