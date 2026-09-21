use super::*;

const NATIVES: &[Native] = &[
    Native::Print,
    Native::Object,
    Native::ObjectKeys,
    Native::Array,
    Native::ArrayIsArray,
    Native::ArrayPush,
    Native::ArrayPop,
    Native::FunctionCall,
    Native::Date,
    Native::Error,
    Native::String,
    Native::Symbol,
    Native::SymbolFor,
    Native::SymbolKeyFor,
    Native::StringCharCodeAt,
    Native::StringCharAt,
    Native::StringSubstring,
    Native::StringSubstr,
    Native::StringFromCharCode,
    Native::ParseInt,
    Native::MathLog,
    Native::MathPow,
    Native::MathFloor,
    Native::MathMin,
    Native::MathMax,
    Native::MathRandom,
    Native::NumberString,
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
        self.global(program, "undefined", Value::UNDEFINED)?;
        self.global(program, "print", self.native_value(Native::Print))?;
        self.global(program, "Date", self.native_value(Native::Date))?;
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
            object,
            "keys",
            self.native_value(Native::ObjectKeys),
        )?;
        self.global(program, "Object", object)
    }

    fn install_console(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let console = self.object();
        self.set_named(program, console, "log", self.native_value(Native::Print))?;
        self.global(program, "console", console)
    }

    fn install_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let array = self.native_value(Native::Array);
        self.array_proto = self.object();
        self.set_named(
            program,
            self.array_proto,
            "push",
            self.native_value(Native::ArrayPush),
        )?;
        self.set_named(
            program,
            self.array_proto,
            "pop",
            self.native_value(Native::ArrayPop),
        )?;
        self.set_named(program, array, "prototype", self.array_proto)?;
        self.set_named(
            program,
            array,
            "isArray",
            self.native_value(Native::ArrayIsArray),
        )?;
        self.global(program, "Array", array)
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

    fn global(&mut self, _p: &ResidualProgram, name: &str, value: Value) -> Result<(), JsError> {
        if let Some(atom) = self.lookup_atom(name) {
            self.set_property(self.globals, atom, value)?;
        }
        Ok(())
    }

    fn set_named(
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
