//! The `Frame` trait: the register, local and operand interface every staged operation runs on.

use super::*;

/// A register frame: each register's word and the static fact the stage keeps about it. A
/// stage with no register facts uses `()`.
pub trait Frame<S: Stage> {
    /// The static fact a stage keeps about a register.
    type Fact: Copy;

    /// The fact `register` currently carries.
    fn fact(&self, register: Register) -> Self::Fact;

    /// The semantic type `register` is proved to have; `None` where the stage keeps none, as the
    /// interpreter does not.
    #[inline(always)]
    fn register_type(&self, _register: Register) -> Option<Type> { None }

    /// `register` holds a value of type `ty` along `edge` of the jump the running step ends in:
    /// a frame that keeps type facts narrows them for that edge's successor.
    #[inline(always)]
    fn refine(&mut self, _register: Register, _ty: Type, _edge: Edge) {}

    /// `register`'s word in the representation its fact names; tagged where it is tagged.
    fn read(&mut self, stage: &mut S, register: Register) -> S::Word;

    /// `register`'s tagged word (`Value::to_bits`), boxing an unboxed representation.
    #[inline(always)]
    fn tagged(&mut self, stage: &mut S, register: Register) -> S::Word {
        self.read(stage, register)
    }

    /// Define `register` with `value`, carrying `fact`.
    fn write(&mut self, stage: &mut S, register: Register, value: S::Word, fact: Self::Fact);

    /// The fact a register's *tagged* word carries: a local holds the word `tagged` boxes, not
    /// the source's unboxed view, so a kept store's result is canonical where the stage can say.
    #[inline(always)]
    fn tagged_fact(&self, register: Register) -> Self::Fact { self.fact(register) }

    /// Copy `source` into `destination` with its value and every fact derived from it; the
    /// word copied.
    #[inline(always)]
    fn move_register(&mut self, stage: &mut S, destination: Register, source: Register) -> S::Word {
        let fact = self.fact(source);
        let value = self.read(stage, source);
        self.write(stage, destination, value, fact);
        value
    }

    /// The running activation's `this` word: the bound value, or `Value::DELETED` where the
    /// activation does not hold it.
    fn this(&mut self, stage: &mut S) -> S::Word;

    /// Define `destination` with the activation's `this`, carrying every fact proved of it.
    fn define_this(&mut self, stage: &mut S, destination: Register, this: S::Word);

    /// `register`'s value from `constant`, with the fact the stage keeps of it.
    fn constant(&mut self, stage: &mut S, constant: Constant) -> (S::Word, Self::Fact);

    /// Store `value` at the dense array element `index`, under the proof.
    #[inline(always)]
    fn index_write(&mut self, _stage: &mut S, _index: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// Define element `index` of the literal array `array` as `value` (`Operation::DefineElement`):
    /// the VM's definition into a fresh hole. `None` where the frame has no definer or the
    /// definition is the general path's.
    #[inline(always)]
    fn define_element(&mut self, _stage: &mut S, _array: S::Word, _index: u32, _value: S::Word) -> Option<()> { None }

    /// Whether the object `word` is an object of any kind the VM's `is_object_like` admits, for an
    /// operation that accepts an object but no primitive (`RequireIteratorResult`) and a word the
    /// direct record test does not prove. `None` where the frame keeps no such answer, the operation's
    /// miss.
    #[inline(always)]
    fn object_like(&mut self, _stage: &mut S, _word: S::Word) -> Option<()> { None }

    /// `RequireObjectCoercible`'s miss on a `null` or `undefined` word: the frame raises the TypeError the
    /// VM's `require_object_coercible` raises, where it has a host. The default raises nothing, and the
    /// row's miss is the caller's.
    #[inline(always)]
    fn object_coercion_failed(&mut self, _stage: &mut S, _word: S::Word) {}

    /// The heap value `allocation` makes, defined into `destination`; `None` where the frame
    /// has no allocator for it or the value is the general path's (a due collection).
    #[inline(always)]
    fn allocate_value(&mut self, _stage: &mut S, _destination: Register, _allocation: Allocation, _proof: S::Proof) -> Option<S::Word> { None }

    /// `Throw`: raises the value in `source` as the frame's completion; `None` where the frame cannot, which
    /// the general step does.
    #[inline(always)]
    fn raise(&mut self, _stage: &mut S, _source: Register) -> Option<Self::Control> { None }

    /// `StringCaseIndex`: the index of the value in `source` among the `count` program constants from
    /// `start`, as the frame's case table answers it; `None` where the frame has none (the general step).
    #[inline(always)]
    fn case_index(&mut self, _stage: &mut S, _source: Register, _start: u32, _count: u16) -> Option<(S::Word, Self::Fact)> { None }

    /// `IteratorCleanupPush`: pushes the iterator in register `iterator` with its `done` register on the
    /// running activation's cleanup stack, closed if the loop exits abruptly; `None` where the frame keeps
    /// no such stack (the general step pushes it).
    #[inline(always)]
    fn iterator_push(&mut self, _stage: &mut S, _iterator: Register, _done: Register) -> Option<()> { None }

    /// `IteratorCleanupPop`: pops the running activation's top cleanup entry; `None` where the frame
    /// has none to pop (the general step raises it).
    #[inline(always)]
    fn iterator_pop(&mut self, _stage: &mut S) -> Option<()> { None }

    /// Continue where no interrupt is requested; `None` leaves at the frame's miss edge.
    #[inline(always)]
    fn safepoint(&mut self, _stage: &mut S, _proof: S::Proof) -> Option<()> { Some(()) }

    /// The code's invalidation word (`code_dependencies::Invalidation`), zero while the facts it holds
    /// still hold (`guard::live`); `None` where the frame has none to read.
    fn invalidation(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The active realm's global object word the code folded facts of (`guard::realm`); `None` where the
    /// frame does not hold the realm.
    fn realm_globals(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// Counts a taken back edge in the loop's counter (`guard::back_edge`); `None` where the frame keeps
    /// no counter.
    fn count_edge(&mut self, _stage: &mut S) -> Option<()> { None }

    /// The heap's array record table, where the frame reads live array records; `None` where it does
    /// not, leaving a row that needs it to its call-out.
    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The element store's host helper; `None` where the frame has no helper table.
    #[inline(always)]
    fn index_set_helper(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// The VM's field-store hit (`LaneHelper::FieldSet`).
    #[inline(always)]
    fn field_store_miss(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// Leave the function with the Number register `register`, in the representation its fact
    /// names; `None` where the frame has no numeric completion.
    #[inline(always)]
    fn ret(&mut self, _stage: &mut S, _register: Register) -> Option<Self::Control> { None }

    /// Whether the running record returns a raw word (`staged::raw_representation`, a `WasmReturn64`):
    /// its `Return` completes through `return_raw`, not `ret`. `false` where the frame has no raw records.
    #[inline(always)]
    fn returns_raw(&self) -> bool { false }

    /// Leave the function with the raw word of `register` (a raw return, `returns_raw`): the word is the
    /// returned value, unboxed and untagged; `None` where the frame cannot complete it, which leaves the
    /// row to its miss.
    #[inline(always)]
    fn return_raw(&mut self, _stage: &mut S, _register: Register) -> Option<Self::Control> { None }

    /// `register`'s Wasm scalar word in the representation `rep` names (zero-extended integer or
    /// IEEE bits); `None` where the frame does not hold it so, which leaves the row to its miss.
    #[inline(always)]
    fn wasm_read(&mut self, _stage: &mut S, _register: Register, _rep: WasmRep) -> Option<S::Word> { None }

    /// Define `register` with the Wasm scalar `word` in the representation `rep` names, returning
    /// its tagged word; `None` where the frame cannot make that value in place.
    #[inline(always)]
    fn wasm_write(&mut self, _stage: &mut S, _register: Register, _word: S::Word, _rep: WasmRep) -> Option<S::Word> { None }

    /// `register`'s machine word as it is, with no tag check: a raw register (`WasmRep::RawBits64`,
    /// `staged::raw_representation`) holds its i64 or f64 bits. `None` where the frame does not hold
    /// registers so.
    #[inline(always)]
    fn raw_read(&mut self, _stage: &mut S, _register: Register) -> Option<S::Word> { None }

    /// Define `register` with the raw machine word `word`, unboxed and untagged, returning the word;
    /// `None` where the frame cannot define it so.
    #[inline(always)]
    fn raw_write(&mut self, _stage: &mut S, _register: Register, _word: S::Word) -> Option<S::Word> { None }

    /// The Wasm global bound in `binding` (the register encoding of its binding), as the register
    /// encoding its slot holds; `None` where the frame cannot load it, which leaves the row to its miss.
    #[inline(always)]
    fn wasm_global_load(&mut self, _stage: &mut S, _binding: S::Word) -> Option<S::Word> { None }

    /// Store `value` (a register encoding) to the mutable Wasm global bound in `binding`; `None`
    /// where the frame cannot, which leaves the row to its miss.
    #[inline(always)]
    fn wasm_global_store(&mut self, _stage: &mut S, _binding: S::Word, _value: S::Word) -> Option<()> { None }

    /// The running instance's slot `slot` as its register encoding (`WasmInstanceBinding`); `None`
    /// where the frame cannot read it, which leaves the row to its miss.
    #[inline(always)]
    fn wasm_instance_slot(&mut self, _stage: &mut S, _slot: u32) -> Option<S::Word> { None }

    /// The 16 bytes of the vector `register` holds; `None` where it holds no vector.
    #[inline(always)]
    fn vector(&mut self, _stage: &mut S, _register: Register) -> Option<[u8; VECTOR_BYTES]> { None }

    /// The 16 bytes of the vector constant `constant` names; `None` where it names none.
    #[inline(always)]
    fn vector_constant(&mut self, _stage: &mut S, _constant: Constant) -> Option<[u8; VECTOR_BYTES]> { None }

    /// Define `register` with the vector `bytes` in a fresh cell, returning its tagged word; `None`
    /// where the frame cannot make it, which leaves the row to its miss.
    #[inline(always)]
    fn set_vector(&mut self, _stage: &mut S, _register: Register, _bytes: [u8; VECTOR_BYTES]) -> Option<S::Word> { None }

    /// The bytes of the linear memory a direct access addresses (the function's memory, which the
    /// frame resolved); `None` where the frame holds none, which leaves the row to its miss.
    #[inline(always)]
    fn direct_memory(&mut self) -> Option<&mut [u8]> { None }

    /// The bytes of the linear memory the Wasm memory value in `register` names, and whether its
    /// index is an i64 (memory64); `None` where the frame does not resolve it, which leaves the row
    /// to its miss.
    #[inline(always)]
    fn wasm_memory(&mut self, _stage: &mut S, _register: Register) -> Option<(&mut [u8], bool)> { None }

    /// The log2 of the page size of the Wasm memory value in `register` (16 for the 64 KiB page),
    /// which `memory.size` counts in; `None` where the frame does not resolve it, which leaves the
    /// row to its miss.
    #[inline(always)]
    fn wasm_memory_page_bits(&mut self, _stage: &mut S, _register: Register) -> Option<u32> { None }

    /// The activation's environment storage word and the fact its slot reads carry
    /// (`environment::environment_slot`); `None` where the frame holds no environment.
    #[inline(always)]
    fn environment(&mut self, _stage: &mut S) -> Option<(S::Word, Self::Fact)> { None }

    /// Store `value` to environment slot `slot`, or with `initialize` define it as the first value
    /// of its binding (`environment::environment_slot_define`) rather than store to an initialized
    /// one (`environment_slot_store`); `None` where the frame cannot, leaving the binding rules (an
    /// uninitialized or constant binding, a `with` scope) to the host.
    #[inline(always)]
    fn environment_write(&mut self, _stage: &mut S, _slot: u16, _value: S::Word, _initialize: bool) -> Option<()> { None }

    /// The count of the arguments binding in local `slot`, an int32 word; `None` where the
    /// binding is not one only kept values answer.
    #[inline(always)]
    fn arguments_length(&mut self, _stage: &mut S, _slot: u16) -> Option<(S::Word, Self::Fact)> { None }
    /// The element `index` (a word) of the arguments binding in local `slot`, where the frame holds
    /// its kept values (a keeping activation's); `None` leaves it to the row's miss.
    #[inline(always)]
    fn argument(&mut self, _stage: &mut S, _slot: u16, _index: S::Word) -> Option<(S::Word, Self::Fact)> { None }

    /// The word of the `arguments` binding in local `slot` where the frame keeps the activation's
    /// arguments unmaterialized (`KeptArguments`): the marker a forwarding read passes on, or the object
    /// the binding makes at its last read; a frame's binding is read as the general path reads it. The
    /// staged local read asks it before the slot's own word (`read_arguments_binding`). `None` where the
    /// frame has no such binding, leaving the read to the slot and the row's miss.
    #[inline(always)]
    fn arguments_binding(&mut self, _stage: &mut S, _slot: u16) -> Option<(S::Word, Self::Fact)> { None }

    /// The by-name read of `atom` through the running window activation's scope chain (its `with`
    /// objects, then its dynamic bindings), defining `destination` and, for a call, `receiver`; `None`
    /// where the frame does not decide it, which leaves the name to the row's miss.
    #[inline(always)]
    fn scoped_name(&mut self, _stage: &mut S, _destination: Register, _receiver: Option<Register>, _atom: u32) -> Option<()> { None }

    /// Store `value` to `atom` through the running window activation's scope chain: the `with` object
    /// that has the name takes it. `None` where no object does, or the frame cannot decide it.
    #[inline(always)]
    fn scoped_store(&mut self, _stage: &mut S, _atom: u32, _value: S::Word) -> Option<()> { None }

    /// Store `value` to local `slot` through the running window activation's scope chain (the `with`
    /// object that has the local's name takes it). `None` where no object does, or the frame cannot decide it.
    #[inline(always)]
    fn scoped_local_store(&mut self, _stage: &mut S, _slot: u16, _value: S::Word) -> Option<()> { None }

    /// The running activation's bound `this`, as a word and the fact the frame keeps of it; `None`
    /// where the frame holds none (an unbound `this`, or a frame that does not hold it), which leaves
    /// the row to its miss edge.
    #[inline(always)]
    fn this_value(&mut self, _stage: &mut S) -> Option<(S::Word, Self::Fact)> { None }

    /// The function the running activation runs (its self binding's value), as a word and the fact
    /// the frame keeps of it; `None` where the frame does not hold it, leaving the read to the
    /// interpreter.
    #[inline(always)]
    fn callee_value(&mut self, _stage: &mut S) -> Option<(S::Word, Self::Fact)> { None }

    /// A rest view's read (`Protocol::RestView`): `rest.length` (`get` false) or `rest[key]`
    /// of the values its activation kept past its first `fixed` parameters; `None` where the
    /// frame keeps none of its own or `key` is no index they hold. The row answers the value and
    /// its runner defines the destination.
    #[inline(always)]
    fn rest_view(&mut self, _stage: &mut S, _get: bool, _fixed: usize, _key: Option<Register>) -> Option<(S::Word, Self::Fact)> { None }

    /// The VM's own operation `operation` (`CallOut`), run in place where a row's words do not
    /// decide it, defining `result` where the operation has one: the word it defined with the
    /// fact the frame keeps of it, or a placeholder word where it defines none. `None` where the
    /// frame has no host, or the operation raised, leaving the instruction to the interpreter.
    #[inline(always)]
    fn vm_step(&mut self, _stage: &mut S, _operation: CallOut, _result: Option<Register>)
        -> Option<(S::Word, Self::Fact)> { None }

    /// The `typeof` kind of the heap value `word`, which its cell decides (a string, a BigInt, a
    /// symbol, an object or a function), where the frame proves it; `None` leaves the row at the
    /// frame's miss edge, where the interpreter names it.
    #[inline(always)]
    fn type_of(&mut self, _stage: &mut S, _word: S::Word) -> Option<TypeofKind> { None }

    /// The interned `typeof` result string of the name `name` (an index in `bytecode::TYPEOF_NAMES`,
    /// as a word), with the fact the frame keeps of it; `None` where the frame holds no such table.
    #[inline(always)]
    fn typeof_string(&mut self, _stage: &mut S, _name: S::Word) -> Option<(S::Word, Self::Fact)> { None }

    /// SetFunctionName: name the function `function` by the program atom `name` through cache
    /// `site`, as the VM does (`Vm::set_function_name`). `None` where the frame has no host, which
    /// leaves the row to the interpreter.
    #[inline(always)]
    fn set_function_name(&mut self, _stage: &mut S, _function: S::Word, _name: u32, _site: u16) -> Option<()> { None }

    /// Read the capture `slot`, `depth` scopes out (a static operand: the walk's length), with the
    /// fact the frame keeps of it; `root` reads the root environment's projected binding (a
    /// script's global var or lexical) the walk ends at instead of a function's slots. `None`
    /// where the frame cannot (an uninitialized or shared binding, no captures), leaving the
    /// read to the interpreter.
    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut S, _depth: u16, _slot: u16, _root: bool) -> Option<(S::Word, Self::Fact)> { None }

    /// Store `value` to the capture `slot`, `depth` scopes out; `None` where the binding does
    /// not take the direct store (a `const`, an uninitialized `let`).
    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut S, _depth: u16, _slot: u16, _value: S::Word) -> Option<()> { None }

    /// Whether `value instanceof constructor` holds, decided by the frame's proof of the constructor's
    /// chain (the VM's published watch, or an ordinary walk that runs no guest code) or by the words
    /// alone (`object::instanceof_by_words` over the frame's record table and the constructor's proven
    /// prototype); `None` where the frame decides neither, leaving the test to the stage's walk and the VM's callout.
    #[inline(always)]
    fn instance_of(&mut self, _stage: &mut S, _value: S::Word, _constructor: S::Word) -> Option<S::Cond> { None }

    /// Continues where register `source` holds a direct array; `None` where the frame does not
    /// decide it, leaving the row to its miss edge.
    #[inline(always)]
    fn direct_array(&mut self, _stage: &mut S, _source: Register) -> Option<()> { None }

    /// The length of the direct array in register `source` as an int32 and its fact; `None` leaves
    /// the read to the VM.
    #[inline(always)]
    fn array_length(&mut self, _stage: &mut S, _source: Register) -> Option<(S::Word, Self::Fact)> { None }

    /// The length of the typed array in register `source` as an int32 and its fact; `None` leaves
    /// the read to the VM.
    #[inline(always)]
    fn typed_length(&mut self, _stage: &mut S, _source: Register) -> Option<(S::Word, Self::Fact)> { None }

    /// Appends `value` to the array in register `array` where its backing is writable and has room;
    /// `None` leaves the append to the VM, which grows it.
    #[inline(always)]
    fn array_append(&mut self, _stage: &mut S, _array: Register, _value: S::Word) -> Option<()> { None }

    /// Where control goes: the edge the interpreter returns, or the branch a specializer
    /// emits.
    type Control;

    /// ToBoolean of `register` from the frame's facts; `None` where a value's heap cell
    /// decides it, leaving the test to the interpreter's fallback.
    fn truthiness(&mut self, stage: &mut S, register: Register) -> Option<S::Cond>;

    /// Continue at the jump target when `condition` holds, else after this instruction.
    fn branch(&mut self, stage: &mut S, condition: S::Cond) -> Self::Control;

    /// Continue at the jump target.
    fn jump(&mut self, stage: &mut S) -> Self::Control;

    /// Where a branch table's selector `index` (an int32 word) goes among `entries` entries: the entry
    /// it picks, the last one for any selector past the cases (`Edge::Entry`). `None` where this
    /// frame cannot decide the entry, leaving the table to the tier's own lowering.
    #[inline(always)]
    fn branch_table(&mut self, _stage: &mut S, _index: S::Word, _entries: u32) -> Option<Self::Control> { None }

    /// The canonical boolean `condition`, with the fact this stage keeps of it.
    fn boolean(&mut self, stage: &mut S, condition: S::Cond) -> (S::Word, Self::Fact);

    /// Read local slot `slot` with the fact this stage keeps of it; `None` where this
    /// frame cannot read the activation's locals (the tier then leaves at its exact PC).
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut S, _slot: u16, _miss: Option<S::Miss>)
        -> Option<(S::Word, Self::Fact)> { None }

    /// Write `value` to local slot `slot`; `false` where this frame cannot write them.
    #[inline(always)]
    fn store_local(&mut self, _stage: &mut S, _slot: u16, _value: S::Word, _miss: Option<S::Miss>)
        -> bool { false }
}
