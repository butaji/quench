use super::*;

const NATIVES: &[Native] = &[
    Native::Print,
    Native::Object,
    Native::ObjectKeys,
    Native::ObjectCreate,
    Native::ObjectAssign,
    Native::ObjectGetPrototypeOf,
    Native::ObjectSetPrototypeOf,
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
    Native::Map,
    Native::MapGet,
    Native::MapSet,
    Native::MapHas,
    Native::MapDelete,
    Native::MapClear,
    Native::MapKeys,
    Native::MapValues,
    Native::MapEntries,
    Native::Set,
    Native::SetAdd,
    Native::SetHas,
    Native::SetDelete,
    Native::SetClear,
    Native::SetKeys,
    Native::SetValues,
    Native::SetEntries,
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
    Native::Date,
    Native::DateNow,
    Native::Error,
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
    Native::NumberString,
    Native::Number,
    Native::NumberIsNaN,
    Native::NumberIsFinite,
    Native::NumberIsInteger,
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
        let array_buffer = self.native_value(Native::ArrayBuffer);
        self.array_buffer_proto = self.object();
        self.set_named(program, array_buffer, "prototype", self.array_buffer_proto)?;
        self.set_named(
            program,
            self.array_buffer_proto,
            "slice",
            self.native_value(Native::ArrayBufferSlice),
        )?;
        self.global(program, "ArrayBuffer", array_buffer)?;
        self.install_collections(program)?;
        self.install_weak_collections(program)?;
        self.install_iterators(program)?;
        self.global(program, "undefined", Value::UNDEFINED)?;
        self.global(program, "NaN", Value::number(f64::NAN))?;
        self.global(program, "Infinity", Value::number(f64::INFINITY))?;
        self.global(program, "print", self.native_value(Native::Print))?;
        self.global(program, "Date", self.native_value(Native::Date))?;
        self.set_named(
            program,
            self.native_value(Native::Date),
            "now",
            self.native_value(Native::DateNow),
        )?;
        self.global(program, "Error", self.native_value(Native::Error))?;
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
        let number = self.native_value(Native::Number);
        self.set_named(
            program,
            number,
            "isNaN",
            self.native_value(Native::NumberIsNaN),
        )?;
        self.set_named(
            program,
            number,
            "isFinite",
            self.native_value(Native::NumberIsFinite),
        )?;
        self.set_named(
            program,
            number,
            "isInteger",
            self.native_value(Native::NumberIsInteger),
        )?;
        self.global(program, "Number", number)?;
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
        if self.atoms.contains_key(&hash) {
            self.atom_collisions.entry(hash).or_default().push(atom);
        } else {
            self.atoms.insert(hash, atom);
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

    pub(super) fn closure(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.object();
        let function = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.function_proto)),
            kind: match p.functions[id as usize].dispatch {
                DispatchClass::General => FunctionKind::User(id),
                DispatchClass::Numeric => FunctionKind::NumericUser(id),
            },
            env,
        });
        if let Some(atom) = self.lookup_atom("prototype") {
            self.set_property(function, atom, prototype)?;
        }
        Ok(function)
    }
}
