//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the staged row (`staged/{environment,elements,entry,value}.rs`)
//! run on the stencil stage and compiled by rustc at build time (`quench-bjit/stencils`), so a row
//! has one definition of its layout reads across the interpreter, the specializers and here.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1; a capture's slot and the slot's byte
//! offset; a site field row packs its object register's frame offset above the destination's in
//! 0, takes the narrow upper-word table in 1 and the cache entry's byte offset in 2). Every failed proof leaves through exit 0 with nothing written, so the interpreter
//! runs the instruction.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::staged::Form;
use quench_bjit::numeric::{library, Place};
use quench_bjit::Stencil;


/// The heap catalog's macro (`quench-bjit/stencils/row_catalog.rs`, exported by the lib) and its heap row names
/// (`row_names.rs`, the lib's copy); `heap_code_of!` reads each row's catalog line.
use quench_bjit::catalog;
use quench_bjit::numeric::row_names;

/// The heap rows' codes as one expression over `$heap` (a `Heap`): each chunk is a `match` whose arms are the catalog's
/// patterns (`Heap::X => row`, a family's offset after `=>`) and whose catch-all is `None`; the chunks are tried in turn.
macro_rules! heap_code_match {
    (@chunks $heap:tt, $acc:expr;) => {
        $acc
    };
    (@chunks $heap:tt, $acc:expr; [$end:ident $table:ident: $($body:tt)*] $($rest:tt)*) => {
        heap_code_match!(@chunks $heap, $acc.or_else(|| heap_code_match!(@go $heap, []; $($body)*)); $($rest)*)
    };
    (@go $heap:tt, [$($arms:tt)*];) => {
        match $heap {
            $($arms)*
            _ => None,
        }
    };
    (@go $heap:tt, [$($arms:tt)*]; reserve $n:expr $(, $($rest:tt)*)?) => {
        heap_code_match!(@go $heap, [$($arms)*]; $($($rest)*)?)
    };
    (@go $heap:tt, [$($arms:tt)*]; $(#[$m:meta])* $name:ident : uses($($u:tt)*) $(x $n:ident)? twin: $kind:ident($what:tt) = $pat:pat $(=> $off:expr)? $(, $($rest:tt)*)?) => {
        heap_code_match!(@go $heap, [$($arms)* $pat => Some(row_names::$name $(+ $off)?),]; $($($rest)*)?)
    };
    (@go $heap:tt, [$($arms:tt)*]; $(#[$m:meta])* $name:ident : uses($($u:tt)*) $(x $n:ident)? twin: $kind:ident($what:tt) $(, $($rest:tt)*)?) => {
        heap_code_match!(@go $heap, [$($arms)*]; $($($rest)*)?)
    };
    ($heap:tt heap { $($chunk:tt)* } number { $($kinds:tt)* }) => {
        heap_code_match!(@chunks $heap, None; $($chunk)*).unwrap_or_else(|| unreachable!("a heap variant without a catalog row"))
    };
}

/// `$heap`'s code: the catalog's heap rows as one expression (`heap_code_match!`).
macro_rules! heap_code_of {
    ($heap:tt) => {
        catalog!(heap_code_match($heap))
    };
}

/// The stencil layout numbers (`staged/layout_offsets.rs`, shared with the stencils by `#[path]`) are the
/// runtime's own: each is a `const` check against the runtime's `offset_of!`, `size_of` or layout const.
/// The stencils are x86-64 only, so the checks are too.
#[cfg(target_arch = "x86_64")]
pub(crate) mod layout_checks;

/// What a heap row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Heap {
    /// Depth-0 capture `slot` of the running activation. It continues at TAKEN (`continues_at_taken`), as a fast arm
    /// of a body's `LoadCapture` whose helper step answers a slot the dense run does not hold (a root binding's shared slot).
    Capture(u16),
    /// `dst = array[index]` of a dense array: the array word at hole 1, the index at hole 2.
    Index,
    /// `array[index] = value` replacing a present element in place: the array word at hole 0, the
    /// value at hole 1, the index at hole 2.
    Store,
    /// `Index` and `Store` of a typed array of the planned `TypedArrayKind` (its discriminant) the loop's entry viewed:
    /// the array's entry words are at the record offset in hole 1 (a read) or hole 0 (a store). The entry proved the
    /// kind, so the row, one per kind, tests none.
    TypedIndex(u8),
    TypedStore(u8),
    /// A typed array's `length`: the count its entry words hold (offset in hole 1), an int32.
    TypedLength,
    /// The field value the loop entry recorded: its byte offset in the record at hole 1.
    Field,
    /// The data field the site's field cache entry names, own or held by the receiver's prototype
    /// (`entry::site_read`), read from the object register whose frame offset sits above the
    /// destination's in hole 0 (hole 2: the entry's byte offset).
    Site,
    /// A chain of two such reads, `object.first.second` (a nested field lookup), whose entry offsets
    /// share hole 2: the first below `CHAIN_SECOND_AT` bits, the second above them.
    SiteChain,
    /// `Site` and `SiteChain` of the activation's receiver, which the payload holds: hole 0 is the
    /// destination alone.
    ThisSite,
    ThisChain,
    /// The `length` of the direct array at hole 1, an int32 (what the array's record counts).
    Length,
    /// `String.prototype.charCodeAt` of the flat string at hole 1, at the int32 index at hole 2 (`CHAR_CODE_AT_ROW`): the call
    /// the method read's native folds into (`base_region::function::native_call`).
    CharCodeAt,
    /// `parseInt` of the flat string at hole 1 in radix 10 (`PARSE_INT_ROW`).
    ParseInt,
    /// Stores the value operand (hole 1) into the own data field the site's cache entry (hole 2) names
    /// of the object register whose frame offset is hole 0, where the store gate admits it.
    SiteStore,
    /// `SiteStore` into the activation's receiver: hole 0 is unused.
    ThisStore,
    /// The inline-cache read (`ic_rows`) of the own slot of the object register (hole 1), whose
    /// exact shape (hole 2) and slot byte offset (hole 3) the code holds from the site's one entry,
    /// into the destination (hole 0): a shape compare and a load, no cache table read (`staged::ic::own_field`).
    IcGet,
    /// The inline-cache store of a Number (hole 1) into that slot of the object register (hole 0),
    /// the atom, shape and slot byte offset in holes 2 to 4, where the slot's observed type holds
    /// Number (`int`: only int32, which takes only int32 words), so no observation widens.
    IcSet { int: bool },
    /// `IcGet` and `IcSet` of a receiver every path proved a direct object (`base_region::proofs`): one compare
    /// of its auxiliary word with the entry's shape proves the shape and flat plain values (holes as theirs).
    IcKnownGet,
    /// `admitted`: the store's run-level test is the region's entry (`Heap::Stores`), which holds with no call since
    /// (`base_region::proofs`), so the row tests nothing of its own.
    IcKnownSet { int: bool, admitted: bool },
    /// `IcGet` of a class-keyed entry (`ic_rows::class_get_row`): the field at hole 3 (a field index) of the live class cell
    /// the receiver (hole 0's upper half) is, whose class key hole 2 holds; a miss leaves as `IcGet`'s.
    IcGetClass,
    /// `IcGetClass` of a held field: the holder cell's granule at hole 4 names the cell the field is read from.
    IcHeldClass,
    /// The store of a Number (hole 1) into the field at hole 3 of the live class cell the object register (hole 0) is, whose
    /// class key is hole 2 (`ic_rows::class_set_value`), where the class space's store gate admits it.
    IcSetClass,
    /// The class-keyed add (`add::class_add`): hole 0 holds the before-key (low) and after-class (high), hole 2 the slot and the
    /// object register's frame offset; the value is hole 1. A hit continues at TAKEN.
    AddClass,
    /// `IcKnownGet` of a receiver whose entry proved its shape and flat form (`ShapeFlat`, `base_region::proofs`): the
    /// arena slot at hole 3 (its byte offset), no compare; hole 2 is zero and unread.
    IcKnownGetFlat,
    /// `IcKnownSet` of a receiver whose entry proved its shape and flat form: the flat slot's byte offset follows the
    /// atom (hole 3) where not `admitted`, else is hole 2; no shape is compared (holes as the stencil reads them).
    IcKnownSetFlat { int: bool, admitted: bool },
    /// Leaves through exit 0 unless the object register at hole 0 is a direct object: what an access's miss
    /// runs after its site row, so the receiver is proved one on every edge out of the access.
    IcProof,
    /// Stores the value operand (hole 0) into depth-0 capture `slot` of an activation whose slots
    /// admit every direct store; the slot is hole 1 and its byte offset hole 2. It continues at TAKEN, as `Capture` does.
    CaptureSet(u16),
    /// The running activation's `this`.
    This,
    /// The activation's local slot `slot`, whose number is hole 1.
    LoadLocal(u16),
    /// The initialized binding `slot` (hole 1, its byte offset hole 2) of the storage of the running activation's
    /// environment window (`JsStepDispatch::environment_storage`) into the register at hole 0, else the row leaves into
    /// the helper step after it. It continues at TAKEN, as `Capture` does.
    EnvLocal(u16),
    /// The register at hole 0 stored to that slot where its binding is initialized, else the row leaves into the helper
    /// step after it; the slot is hole 1 and its byte offset hole 2. It continues at TAKEN.
    EnvStore(u16),
    /// The register at hole 0 defined as that slot's value, over a dead zone or any value but a shared slot (`InitializeTdz`
    /// passes the hole), else the row leaves into the helper step; the slot is hole 1 and its byte offset hole 2.
    /// It continues at TAKEN.
    EnvDefine(u16),
    /// The first operand written to the activation's local slot `slot`, whose number is hole 2. It continues at TAKEN
    /// (`continues_at_taken`): as a fast arm it hops past the helper step, in a loop region it branches to the next
    /// instruction.
    StoreLocal(u16),
    /// The fast arm of a `LoadLocal` of a frame-owned slot (`staged::read_initialized_local`): slot `slot`
    /// (hole 1) into the register at hole 0 where its binding is initialized, else the row leaves into the
    /// helper step after it, which raises.
    LoadChecked(u16),
    /// The fast arm of a `StoreLocal` that does not initialize its binding (`staged::write_initialized_local`):
    /// the first operand to slot `slot` (hole 2) where the binding is initialized.
    StoreChecked(u16),
    /// The register at the frame offset of hole 1 holds exactly the word of hole 2.
    Identity,
    /// The entry test of a declared parameter (`base_region::declared`): the register at hole 0 holds a word whose
    /// tag-decided languages meet the declared mask at hole 1, else the entry leaves with nothing written.
    Declared,
    /// The entry test of a captured declaration a body calls by its identity (`base_region::link::entry_rows`): the
    /// depth-0 capture at the slot of hole 0 (its byte offset in hole 1) holds the word of hole 2, else the entry leaves
    /// with nothing written.
    CaptureIdentity,
    /// The entry test of a class-typed parameter (`base_region::link::entry_rows`): the register at hole 0 holds a direct
    /// object whose exact shape and prototype link are the pair in hole 1 (`entry::shape_link_matches`), else the entry
    /// leaves with nothing written.
    Prototype,
    /// The entry test of a live class cell (`base_region::link::entry_rows`): the register at hole 0 holds a direct object
    /// whose live class cell's class is the one in hole 1 (`guard::class`), else the entry leaves with nothing written.
    Class,
    /// The entry test of a loop-invariant receiver (`base_region::link::entry_rows`): the register at hole 0 holds a direct
    /// object of the exact shape in hole 1 (`guard::shape`), else the entry leaves with nothing written.
    Shape,
    /// `Shape` whose object is also in the flat form (`guard::shape` at `Form::Arena`): its values are the arena run's.
    ShapeFlat,
    /// The entry test that the run stores specialized (`base_region::link::entry_rows`, `guard::stores`): else the entry
    /// leaves with nothing written.
    Stores,
    /// A prototype-held field folded to the value its holder held when the code was compiled (hole 2):
    /// the object register whose frame offset sits above the destination's in hole 0 is a direct object
    /// of the exact shape in hole 1's low half and the prototype link in its high half, else the row
    /// leaves. A Stable fact holds the value (`PrototypeProof::Linked`, `entry::held_constant`).
    Held,
    /// `Held` of the activation's receiver, which the payload holds: hole 0 is the destination alone.
    ThisHeld,
    /// Branches to its taken successor, an entry's failure, unless the active realm's global object (the
    /// word at the byte offset in hole 1 of the VM) is the word of hole 2: the realm a body folded
    /// facts of its callees or its global names in (`Vm::realm_globals_offset`), proved once, at the entry.
    Realm,
    /// Leaves unless the invalidation word at the address in hole 1 is zero (`code_dependencies::Invalidation`):
    /// the check before a row that holds a Stable fact where a call may have changed it.
    Live,
    /// The fast arm of `RequireIteratorResult` (`staged::iteration`): the register at the frame offset
    /// of hole 1 holds a direct object or array record, else the row leaves into the helper step after it.
    Object,
    /// The fast arm of `ToPropertyKey`: the register at hole 1 holds a direct string, which is copied to
    /// the register at hole 0, else the row leaves into the helper step after it.
    Key,
    /// The fast arm of `Unary`'s `typeof` index (`staged::value::immediate_typeof_index`): the register at
    /// hole 1 holds a value whose tag decides its index, an int written to the register at hole 0, else the
    /// row leaves into the helper step after it.
    TypeofIndex,
    /// The fast arm of `Unary`'s `typeof` string (`staged::value::immediate_typeof_string`): the register at hole 1
    /// holds a value whose tag decides its index, the entry of the table of result strings at hole 2 written to
    /// the register at hole 0, else the row leaves into the helper step after it.
    TypeofString,
    /// The fast arm of `Binary`'s `instanceof` of a constructor the code holds (`staged::object::instanceof_by_words`): the
    /// register at hole 0's upper half is walked along its prototype chain for the `prototype` link the code holds (hole 1's
    /// upper half, beside the constructor register's offset), where the constructor register holds exactly the identity of
    /// hole 2, a boolean written to the register at hole 0's lower half, else the row leaves into the helper step after it.
    Instanceof,
    /// The fast arm of `Unary`'s `typeof` test: whether (or, where `negated`, whether not) that index is
    /// `index` (hole 2), as a boolean.
    TypeofIs { index: u8, negated: bool },
    /// The global object's data slot (hole 2: its byte offset in the arena run behind its extras) read into the
    /// destination, the object being the record id of hole 1: a Stable fact holds the object's layout.
    GlobalRead,
    /// The first operand stored to that slot (hole 0: the record id), where the row's `entry` variant takes the
    /// store gate for an operand that may be a heap reference, and a Number operand needs none.
    GlobalStore,
    /// The word of the global lexical binding at the address in hole 1 into the destination where it is initialized
    /// (`staged::lexical_read`), else the row leaves, for the interpreter to raise the dead zone's `ReferenceError`: a Stable
    /// fact holds the address (the global layout).
    LexicalRead,
    /// The first operand stored to the initialized global lexical binding at the address in hole 2, which no code holds the value of.
    LexicalWrite,
    /// The tagged word of hole 1 into the destination: an immutable global, a folded `typeof` name, and
    /// the `undefined` receiver of a name call.
    Word,
    /// An inlined callee's depth-0 capture: the ordinary value of the slot at the address in hole 1 (the
    /// guarded closure's environment is fixed) into the destination, else the row leaves inside the callee.
    CalleeCapture,
    /// The fast arm of `AppendArray` (`elements::append_in_room`): the Number operand at hole 1 is appended to
    /// the unique writable dense array in the frame slot of hole 0 where the backing's capacity word proves
    /// room, else the row leaves into the helper step after it, which grows the backing.
    Append,
    /// The property add of a `DefineField` or an adding `SetField` (`staged::add::site_add`): the value
    /// operand (hole 1) joins the object register (its frame offset in hole 2) or, with `this`, the
    /// activation's receiver, by the transition holes 0 and 2 carry (`numeric::site_bits::ADD_AFTER_AT`,
    /// `ADD_OBJECT_AT`), where the receiver holds the before-shape in `form`'s storage and, for the
    /// row's `entry` variant, the prototype link hole 2 names. A hit continues at TAKEN; a miss leaves
    /// into the helper step after the row, or the leave a guarded row makes.
    Add { form: Form, this: bool },
    /// The fast arm of `ForInNext` (`staged::iteration::for_in_next`): the for-in state in the frame slot of
    /// hole 1 yields its next key to the slot of hole 0 where its memo proves the snapshot current, else the row
    /// leaves into the helper step after it, which takes any other step and the exhausted one's jump.
    ForIn,
    /// The fast arm of `SetFunctionName` (`staged::naming::site_name`): the function in the frame slot
    /// whose offset hole 2 carries (`numeric::site_bits::NAME_FUNCTION_AT`) takes the name word of hole 1 in the slot
    /// hole 2 names, moving between the shapes of hole 0 (`numeric::site_bits::ADD_AFTER_AT`), where it holds the empty
    /// name in `form`'s storage. A hit continues at TAKEN; a miss leaves into the helper step after the row.
    Name { form: Form },
    /// A staged row of the `library::STAGED_FIRST..=STAGED_LAST` run (`staged_rows.rs`), by its code: a fast arm over the
    /// register operands the planner names (`steps::fast_arm`), its destination a frame slot, which a miss leaves for the
    /// helper step after the row. A hit continues at TAKEN.
    Staged(u8),
}

/// The storage forms an add row exists for, in the order of the catalog's add rows (packed, arena, either).
pub(crate) const FORMS: [Form; 3] = [Form::Packed, Form::Arena, Form::Either];

/// A heap row: its operation and where its operands and destination live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HeapRow {
    pub(crate) heap: Heap,
    pub(crate) dst: Place,
    pub(crate) a: Place,
    pub(crate) b: Place,
    /// The row's variant: an array's backing is the entry record's (the offset at the array hole), not
    /// looked up per access (`Index`, `Store`); a site read is a chain of two reads (`SiteChain`).
    pub(crate) entry: bool,
}

impl Heap {
    /// The row's code in `quench_bjit::numeric::library`: the catalog line of the row names its variant (`row_catalog.rs`).
    pub(crate) fn code(self) -> u8 {
        match self {
            Heap::Staged(code) => code,
            _ => heap_code_of!(self),
        }
    }

    /// Whether a hit continues at the row's TAKEN exit, not its NEXT: the rows of one convention with the fast arms, whose
    /// hit hops past the step laid out after them (`base_region::steps::plan_fast`).
    pub(crate) fn continues_at_taken(self) -> bool {
        matches!(self, Heap::StoreLocal(_) | Heap::Capture(_) | Heap::CaptureSet(_) | Heap::EnvLocal(_) | Heap::EnvStore(_) | Heap::EnvDefine(_))
    }

    /// Whether the row reads a word of its operand's frame slot (a fast arm that tests the word),
    /// which a register held in a pinned register as a Number does not have there.
    pub(crate) fn reads_frame_word(self) -> bool {
        matches!(self, Heap::Object | Heap::Key | Heap::ForIn | Heap::Declared | Heap::Prototype | Heap::Class | Heap::Shape | Heap::ShapeFlat | Heap::TypeofIndex | Heap::TypeofString | Heap::Instanceof | Heap::TypeofIs { .. })
    }

    /// Whether the row's result is a Number a double holds (a typed element), whatever the site's feedback.
    pub(crate) fn yields_double(self) -> bool {
        matches!(self, Heap::TypedIndex(_))
    }

    /// Whether the row's result is a tagged word whatever the registers around it hold: a value
    /// the activation holds, not a Number the region proved.
    pub(crate) fn yields_tagged(self) -> bool {
        matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::Word | Heap::CharCodeAt | Heap::ParseInt)
    }
}

impl HeapRow {
    /// The row's canonical code in `quench_bjit::numeric::library`.
    pub(crate) fn code(self) -> library::HeapCode {
        library::heap_canonical((self.heap.code(), self.entry, self.dst.code(), self.a.code(), self.b.code()))
    }
}

/// The stencil of `row`, built at compile time by rustc; a row the library lacks declines the region.
pub(crate) fn stencil(row: HeapRow) -> Result<&'static Stencil, String> {
    static TABLE: quench_bjit::rustc::Memo<library::HeapCode> = std::sync::OnceLock::new();
    let code = row.code();
    quench_bjit::rustc::memoized(&TABLE, code, || quench_bjit::rustc::stencil(&library::heap_name(code)).ok_or_else(|| format!("no stencil row {row:?}")))
}

/// The runtime's heap codes are the catalog's rows (`quench-bjit/stencils/row_catalog.rs`): a `Heap` variant
/// whose code the catalog did not list would plan a row no stencil is built for. `every_variant` names each
/// variant over every payload its arms tell apart, so a variant whose code is not a catalog row fails here.
#[cfg(test)]
mod catalog_tests {
    use super::{FORMS, Heap, library};

    /// Every variant, with each payload value its arms distinguish: every typed kind, every staged code, every flag and form.
    fn every_variant() -> Vec<Heap> {
        let mut all = vec![
            Heap::Capture(0), Heap::Index, Heap::Store, Heap::TypedLength, Heap::Field, Heap::Site, Heap::SiteChain,
            Heap::ThisSite, Heap::ThisChain, Heap::Length, Heap::CharCodeAt, Heap::ParseInt, Heap::SiteStore, Heap::ThisStore,
            Heap::IcGet, Heap::IcGetClass, Heap::IcHeldClass, Heap::IcSetClass, Heap::AddClass, Heap::IcKnownGet, Heap::IcKnownGetFlat, Heap::IcProof, Heap::CaptureSet(0), Heap::This, Heap::LoadLocal(0), Heap::EnvLocal(0),
            Heap::EnvStore(0), Heap::EnvDefine(0), Heap::StoreLocal(0), Heap::LoadChecked(0), Heap::StoreChecked(0), Heap::Identity,
            Heap::Declared, Heap::CaptureIdentity, Heap::Prototype, Heap::Class, Heap::Shape, Heap::ShapeFlat, Heap::Stores, Heap::Held, Heap::ThisHeld, Heap::Realm,
            Heap::Live, Heap::Object, Heap::Key, Heap::TypeofIndex, Heap::TypeofString, Heap::Instanceof, Heap::GlobalRead,
            Heap::GlobalStore, Heap::LexicalRead, Heap::LexicalWrite, Heap::Word, Heap::CalleeCapture, Heap::Append, Heap::ForIn,
        ];
        all.extend((0..library::TYPED_KINDS).flat_map(|kind| [Heap::TypedIndex(kind), Heap::TypedStore(kind)]));
        all.extend((quench_bjit::numeric::row_names::STAGED_FIRST..=quench_bjit::numeric::row_names::STAGED_LAST).map(Heap::Staged));
        for flag in [false, true] {
            all.push(Heap::IcSet { int: flag });
            all.push(Heap::TypeofIs { index: 0, negated: flag });
            for admitted in [false, true] {
                all.push(Heap::IcKnownSet { int: flag, admitted });
                all.push(Heap::IcKnownSetFlat { int: flag, admitted });
            }
        }
        for form in FORMS {
            all.push(Heap::Name { form });
            for this in [false, true] {
                all.push(Heap::Add { form, this });
            }
        }
        all
    }

    #[test]
    fn heap_codes_are_catalog_rows() {
        let catalog = library::heap_row_codes();
        for heap in every_variant() {
            let code = heap.code();
            assert!(catalog.contains(&code), "{heap:?} has code {code}, which the catalog does not list");
        }
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
pub(crate) mod tests;
