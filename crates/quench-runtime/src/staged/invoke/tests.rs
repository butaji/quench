use super::*;
use crate::staged::{Bound, Concrete, Edge, NoPlans};

pub(super) const RETURNED: u32 = 3;
pub(super) const IDENTITY: u64 = 0x1000;
pub(super) const GLOBAL: u64 = 0x2000;

/// A frame over plain memory: a register file, one stack of words, the activation's `this`, the
/// call records of a method site, and a callee that returns what the call entered it with.
pub(super) struct Probe {
    pub(super) file: [u64; 8],
    pub(super) stack: [u64; 8],
    pub(super) entry: CallEntry,
    pub(super) this: u64,
    pub(super) records: Option<Vec<Register>>,
    /// The method the receiver's property rows load, and what they were asked for.
    pub(super) method: Option<u64>,
    pub(super) loaded: Option<(u64, u32, u32)>,
    /// The word a `this` field path reads.
    pub(super) field: Option<u64>,
    /// The words of a plan's constant list, and what a native was entered with.
    pub(super) constants: Vec<u64>,
    pub(super) natives: Vec<(NativeId, u64, Vec<u64>)>,
    pub(super) native_status: u32,
    pub(super) linked: bool,
    pub(super) entered: Vec<u64>,
    pub(super) helper: Option<u64>,
    pub(super) settled: Option<u64>,
    pub(super) returned: Option<u64>,
}

impl Probe {
    pub(super) fn new(entry: CallEntry) -> Self {
        Self { file: [0; 8], stack: [0; 8], entry, this: Value::DELETED.to_bits(), records: None, method: None, loaded: None,
            field: None, constants: Vec::new(), natives: Vec::new(), native_status: RETURNED, linked: false,
            entered: Vec::new(), helper: None, settled: None, returned: None }
    }
}

impl Frame<Concrete> for Probe {
    type Fact = ();
    type Control = Edge;
    fn fact(&self, _register: Register) {}
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.file[usize::from(register)] }
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.file[usize::from(register)] = value;
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 { self.this }
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {}
    fn constant(&mut self, _stage: &mut Concrete, constant: crate::staged::Constant) -> (u64, ()) {
        (constant.value.to_bits(), ())
    }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge { Edge::Following }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
    fn ret(&mut self, stage: &mut Concrete, register: Register) -> Option<Edge> {
        self.returned = Some(self.read(stage, register));
        Some(Edge::Target)
    }
}

impl PropertyFrame<Concrete> for Probe {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
    fn hoisted(&mut self, _stage: &mut Concrete, _lookup: FieldLookup) -> Option<u64> { self.field }
}

impl CallFrame<Concrete> for Probe {
    fn call_entry(&self, _site: &Invocation) -> CallEntry { self.entry }
    fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> { Some(self.file.as_ptr() as u64) }
    fn call_list(&self, list: RegisterList) -> Option<&[Register]> { self.records.as_deref().map(|records| list.registers(records)) }
    fn call_constants(&self, list: WordList) -> Option<&[u64]> { Some(list.words(&self.constants)) }
    fn call_link(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<()> {
        self.linked = true;
        Some(())
    }
    fn call_native(&mut self, _stage: &mut Concrete, native: NativeId, this: u64, (arguments, count): (u64, u16), _miss: ())
        -> Option<Entered<u64, ()>> {
        // SAFETY: the arguments are `count` words of the probe's own file or stack.
        let passed = unsafe { std::slice::from_raw_parts(arguments as *const u64, usize::from(count)) }.to_vec();
        self.natives.push((native, this, passed));
        Some(Entered { status: u64::from(self.native_status), word: Value::integer(55).to_bits(), unfinished: () })
    }
    fn call_result(&mut self, _stage: &mut Concrete, _site: &Invocation, value: u64) -> Option<()> {
        self.settled = Some(value);
        Some(())
    }
    fn call_method(&mut self, _stage: &mut Concrete, receiver: u64, atom: u32, site: u32, _miss: ()) -> Option<u64> {
        self.loaded = Some((receiver, atom, site));
        self.method
    }
    fn call_receiver(&mut self, _stage: &mut Concrete, _constructor: u64) -> Option<u64> { Some(Value::integer(77).to_bits()) }
    fn call_room(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<Room<u64>> {
        let top = self.stack.as_mut_ptr() as u64;
        Some(Room { cell: 0, top, next: top, shadow: 0 })
    }
    fn call_enter(&mut self, _stage: &mut Concrete, this: u64, _room: Room<u64>) -> Option<Entered<u64, ()>> {
        self.entered = self.stack[..3].to_vec();
        Some(Entered { status: u64::from(RETURNED), word: this, unfinished: () })
    }
    fn call_settle(&mut self, _stage: &mut Concrete, _site: &Invocation, _entered: Entered<u64, ()>, _room: Room<u64>,
        value: u64) -> Option<()> {
        self.settled = Some(value);
        Some(())
    }
    fn call_helper(&mut self, _stage: &mut Concrete, _site: &Invocation, direct: Option<u64>) -> Option<u64> {
        direct.or_else(|| { self.helper = Some(1); Some(0) })
    }
}

pub(super) fn plan(kind: CalleeKind, sloppy: Option<u64>) -> CallEntry {
    CallEntry::Direct(DirectPlan { kind, identity: Identity::Guarded(IDENTITY), sloppy, registers: 3, returned: RETURNED, bound: None })
}

pub(super) fn site(receiver: Receiver) -> Invocation {
    Invocation { callee: Callee::Register(0), receiver, arguments: Arguments::Window { first: 2, count: 2 }, result: 7,
        returns: false, forwards: false }
}

pub(super) fn call(probe: &mut Probe, site: Invocation) -> Option<u64> {
    match invoke(&mut Concrete::pure(), probe, (), site) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

#[test]
fn a_frame_without_a_direct_entry_takes_the_vm_call() {
    let mut probe = Probe::new(CallEntry::Helper);
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(0));
    assert_eq!((probe.helper, probe.settled, probe.entered.len()), (Some(1), None, 0));
}

#[test]
fn a_direct_call_admits_binds_fills_enters_and_settles_in_order() {
    let mut probe = Probe::new(plan(CalleeKind::Script, Some(GLOBAL)));
    let receiver = Value::UNDEFINED.to_bits();
    probe.file[..5].copy_from_slice(&[IDENTITY, receiver, 10, 20, 0]);
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(GLOBAL), "a sloppy callee binds the global for undefined");
    assert_eq!(probe.entered, [10, 20, Value::UNDEFINED.to_bits()], "the window takes the arguments then undefined");
    assert_eq!((probe.settled, probe.helper), (Some(GLOBAL), None));
}

#[test]
fn a_construction_yields_the_receiver_its_plan_allocated() {
    let mut probe = Probe::new(plan(CalleeKind::Class, None));
    probe.file[..4].copy_from_slice(&[IDENTITY, 0, 5, 6]);
    assert_eq!(call(&mut probe, site(Receiver::Allocated)), Some(Value::integer(77).to_bits()));
    assert_eq!(probe.entered[..2], [5, 6]);
}

#[test]
fn a_callee_that_is_not_the_bound_one_takes_the_vm_call() {
    let mut probe = Probe::new(plan(CalleeKind::Script, None));
    probe.file[0] = IDENTITY + 1;
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(0));
    assert_eq!((probe.helper, probe.entered.len()), (Some(1), 0), "nothing entered");
}

#[test]
fn a_sloppy_callee_leaves_a_primitive_receiver_to_the_host() {
    let mut probe = Probe::new(plan(CalleeKind::Script, Some(GLOBAL)));
    probe.file[..2].copy_from_slice(&[IDENTITY, Value::integer(5).to_bits()]);
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(0));
    assert_eq!(probe.helper, Some(1));
}

/// A forwarding call and a returning one differ from a call in what their tier does with the
/// result, not in the rows: the same window is entered and the same value completes.
#[test]
fn a_forwarding_or_returning_call_runs_the_rows_of_a_call() {
    for form in [Invocation { forwards: true, ..site(Receiver::Register(1)) }, Invocation { returns: true, ..site(Receiver::Register(1)) }] {
        let mut probe = Probe::new(plan(CalleeKind::Script, None));
        probe.file[..5].copy_from_slice(&[IDENTITY, Value::integer(1).to_bits(), 10, 20, 0]);
        assert_eq!(call(&mut probe, form), Some(Value::integer(1).to_bits()), "{form:?}");
        assert_eq!(probe.entered[..2], [10, 20]);
        assert_eq!(probe.settled, Some(Value::integer(1).to_bits()));
    }
}

#[test]
fn a_return_completes_with_the_register_value() {
    let mut probe = Probe::new(CallEntry::Helper);
    probe.file[3] = 42;
    let step = ret(&mut Concrete::pure(), &mut probe, 3);
    assert!(matches!(step, Step::Control(Edge::Target)));
    assert_eq!(probe.returned, Some(42));
}

fn folded(sloppy: Option<u64>) -> CallEntry {
    CallEntry::Direct(DirectPlan { kind: CalleeKind::Script, identity: Identity::Folded(IDENTITY), sloppy, registers: 3,
        returned: RETURNED, bound: None })
}

#[test]
fn a_known_call_reads_and_checks_no_callee_and_binds_undefined() {
    let mut probe = Probe::new(folded(Some(GLOBAL)));
    probe.file[..4].copy_from_slice(&[0xdead, 0, 10, 20]);
    let known = Invocation { callee: Callee::Known(3), receiver: Receiver::Undefined, ..site(Receiver::Undefined) };
    assert_eq!(call(&mut probe, known), Some(GLOBAL), "a sloppy callee binds the global for the known call's undefined");
    assert_eq!(probe.entered[..2], [10, 20]);
    assert_eq!(probe.helper, None, "no identity guard, so none can fail");
}

#[test]
fn a_callee_folded_under_a_dependency_is_not_read_per_call() {
    let mut probe = Probe::new(folded(None));
    probe.file[..4].copy_from_slice(&[IDENTITY + 1, Value::integer(1).to_bits(), 10, 20]);
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(Value::integer(1).to_bits()));
    assert_eq!((probe.entered[..2].to_vec(), probe.helper), (vec![10, 20], None));
}

pub fn listed(receiver: Receiver) -> Invocation {
    Invocation { callee: Callee::Method { atom: 8, site: 2 }, receiver,
        arguments: Arguments::List(RegisterList { start: 1, count: 2 }), ..site(receiver) }
}

#[test]
fn a_method_call_loads_its_method_from_the_receiver_and_passes_its_listed_registers() {
    let mut probe = Probe::new(plan(CalleeKind::Script, None));
    probe.method = Some(IDENTITY);
    probe.records = Some(vec![9, 6, 3]);
    probe.file[..7].copy_from_slice(&[0, Value::integer(1).to_bits(), 0, 0, 0, 0, 30]);
    probe.file[3] = 20;
    assert_eq!(call(&mut probe, listed(Receiver::Register(1))), Some(Value::integer(1).to_bits()), "the receiver is this");
    assert_eq!(probe.loaded, Some((Value::integer(1).to_bits(), 8, 2)));
    assert_eq!(probe.entered, [30, 20, Value::UNDEFINED.to_bits()], "registers 6 and 3, not consecutive");
}

#[test]
fn a_method_that_is_not_the_planned_one_or_a_list_the_frame_lacks_takes_the_vm_call() {
    let mut probe = Probe::new(plan(CalleeKind::Script, None));
    probe.method = Some(IDENTITY + 1);
    probe.records = Some(vec![9, 6, 3]);
    assert_eq!(call(&mut probe, listed(Receiver::Register(1))), Some(0));
    assert_eq!((probe.helper, probe.entered.len()), (Some(1), 0));
    let mut bare = Probe::new(plan(CalleeKind::Script, None));
    bare.method = Some(IDENTITY);
    assert_eq!(call(&mut bare, listed(Receiver::Register(1))), Some(0), "no call records");
    assert_eq!(bare.helper, Some(1));
}

#[test]
fn a_this_method_call_binds_this_or_the_field_its_path_reads() {
    let this = Value::integer(5).to_bits();
    let mut probe = Probe::new(plan(CalleeKind::Script, None));
    (probe.method, probe.records, probe.this, probe.field) = (Some(IDENTITY), Some(vec![0, 6, 3]), this, Some(77));
    assert_eq!(call(&mut probe, listed(Receiver::This { path: None })), Some(this));
    assert_eq!(probe.loaded.map(|loaded| loaded.0), Some(this));
    assert_eq!(call(&mut probe, listed(Receiver::This { path: Some((11, 6)) })), Some(77), "the field of this is the receiver");
    assert_eq!(probe.loaded.map(|loaded| loaded.0), Some(77));
    probe.this = Value::DELETED.to_bits();
    probe.helper = None;
    assert_eq!(call(&mut probe, listed(Receiver::This { path: None })), Some(0), "an unbound this leaves");
    assert_eq!(probe.helper, Some(1));
}

const NATIVE: NativeId = NativeId { index: 5, beside: false };

fn native(beside: bool, bound: Option<Bound>) -> CallEntry {
    CallEntry::Direct(DirectPlan { kind: CalleeKind::Native(NativeId { beside, ..NATIVE }), identity: Identity::Guarded(IDENTITY),
        sloppy: None, registers: 0, returned: RETURNED, bound })
}

#[test]
fn a_native_is_entered_with_the_callers_window_after_the_lane_links_unless_it_runs_beside() {
    for beside in [false, true] {
        let mut probe = Probe::new(native(beside, None));
        probe.file[..4].copy_from_slice(&[IDENTITY, Value::integer(1).to_bits(), 10, 20]);
        assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(Value::integer(55).to_bits()), "the native's value");
        assert_eq!(probe.natives, [(NativeId { beside, ..NATIVE }, Value::integer(1).to_bits(), vec![10, 20])], "the window as it lies");
        assert_eq!((probe.linked, probe.settled, probe.helper), (!beside, Some(Value::integer(55).to_bits()), None));
    }
}

#[test]
fn a_native_that_left_or_a_construction_of_one_takes_the_vm_call() {
    let mut probe = Probe::new(native(false, None));
    probe.file[..2].copy_from_slice(&[IDENTITY, Value::integer(1).to_bits()]);
    probe.native_status = RETURNED + 1;
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(0));
    assert_eq!((probe.settled, probe.helper), (None, Some(1)), "a status other than returned defines no result");
    let mut construct = Probe::new(native(false, None));
    construct.file[0] = IDENTITY;
    assert_eq!(call(&mut construct, site(Receiver::Allocated)), Some(0));
    assert_eq!((construct.natives.len(), construct.helper), (0, Some(1)), "a construction is the VM's");
}

#[test]
fn a_natives_listed_arguments_and_bound_prefix_are_gathered_into_the_room() {
    let bound = Bound { this: Value::integer(9).to_bits(), arguments: WordList { start: 0, count: 1 } };
    let mut probe = Probe::new(native(true, Some(bound)));
    (probe.constants, probe.records, probe.method) = (vec![7], Some(vec![9, 6, 3]), Some(IDENTITY));
    probe.file[..7].copy_from_slice(&[0, Value::integer(1).to_bits(), 0, 20, 0, 0, 30]);
    let site = Invocation { callee: Callee::Method { atom: 8, site: 2 }, arguments: Arguments::List(RegisterList { start: 1, count: 2 }),
        ..site(Receiver::Register(1)) };
    assert_eq!(call(&mut probe, site), Some(Value::integer(55).to_bits()));
    assert_eq!(probe.natives, [(NativeId { beside: true, ..NATIVE }, Value::integer(9).to_bits(), vec![7, 30, 20])],
        "the bound this and the prefix, then registers 6 and 3");
}

#[test]
fn a_bound_function_enters_its_target_with_its_this_and_its_arguments_before_the_calls() {
    let bound = Bound { this: Value::UNDEFINED.to_bits(), arguments: WordList { start: 0, count: 2 } };
    let target = DirectPlan { kind: CalleeKind::Script, identity: Identity::Folded(IDENTITY), sloppy: Some(GLOBAL), registers: 5,
        returned: RETURNED, bound: Some(bound) };
    let mut probe = Probe::new(CallEntry::Direct(target));
    probe.constants = vec![4, 5];
    probe.file[..4].copy_from_slice(&[0, 0, 10, 20]);
    assert_eq!(call(&mut probe, site(Receiver::Register(1))), Some(GLOBAL), "the target is sloppy, so the bound undefined is the global");
    assert_eq!(probe.stack[..5], [4, 5, 10, 20, Value::UNDEFINED.to_bits()], "the prefix, the arguments, then undefined");
    let mut construct = Probe::new(CallEntry::Direct(DirectPlan { kind: CalleeKind::Class, ..target }));
    assert_eq!(call(&mut construct, site(Receiver::Allocated)), Some(0), "a construction through a bound function is the VM's");
    assert_eq!(construct.helper, Some(1));
}

#[test]
fn a_call_planned_inline_keeps_its_direct_entry_where_the_unit_did_not_splice_it() {
    let plan = DirectPlan { kind: CalleeKind::Script, identity: Identity::Folded(IDENTITY), sloppy: None, registers: 3,
        returned: RETURNED, bound: None };
    let mut declined = Probe::new(CallEntry::Inline(plan));
    declined.file[..4].copy_from_slice(&[0, Value::integer(1).to_bits(), 10, 20]);
    assert_eq!(call(&mut declined, site(Receiver::Register(1))), Some(Value::integer(1).to_bits()));
    assert_eq!(declined.entered[..2], [10, 20]);
}
