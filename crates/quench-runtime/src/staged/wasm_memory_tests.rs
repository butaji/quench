//! The linear-memory rules of `staged::wasm_memory` on the interpreter's stage against the
//! declared accesses (`MemoryLoad::read`, `MemoryStore::write`, `checked_range`), the order of
//! a bulk operation's checks on a probe stage, and the opcode table.

use super::wasm_machine::ConcreteInstance;
use super::wasm_memory::{
    Access, Memory, WasmMemory, WasmMemoryWrite, copy, fill, load, size, store,
};
use super::wasm_step::Step;
use super::wasm_tests::Probe;
use crate::bytecode::{Op, WideInstruction};
use crate::wasm::memory::{
    LinearBytes, MemoryLoad, MemoryStore, WASM_PAGE_BITS, WASM_PAGE_BYTES, checked_range,
};
use crate::{WasmTrap, WasmType, WasmValue};

const MEMORY_BYTES: usize = 24;
const ADDRESSES: [u32; 9] = [0, 1, 7, 8, 16, 20, 23, 24, u32::MAX];
const OFFSETS: [u32; 4] = [0, 1, 4, u32::MAX];

fn pattern() -> Vec<u8> {
    (0..MEMORY_BYTES).map(|at| (at * 37 + 11) as u8).collect()
}

fn scalar_loads() -> impl Iterator<Item = (MemoryLoad, Access)> {
    (0..)
        .map_while(MemoryLoad::from_tag)
        .filter_map(|load| Some((load, Access::of_load(load)?)))
}

fn scalar_stores() -> impl Iterator<Item = (MemoryStore, Access)> {
    (0..)
        .map_while(MemoryStore::from_tag)
        .filter(|store| store.value_type() != WasmType::V128)
        .map(|store| (store, Access::of_store(store)))
}

fn instance(memory: &mut [u8]) -> ConcreteInstance<'_> {
    ConcreteInstance {
        globals: &mut [],
        memory,
    }
}

#[test]
fn a_load_of_the_stage_is_the_declared_read() {
    for (operator, access) in scalar_loads() {
        for (address, offset) in ADDRESSES
            .iter()
            .flat_map(|a| OFFSETS.iter().map(move |o| (*a, *o)))
        {
            let mut bytes = pattern();
            let declared = operator.read(&bytes, u64::from(address) + u64::from(offset));
            let mut stage = instance(&mut bytes);
            let index = stage.memory_index(u64::from(address));
            let staged = load(&mut stage, access, index, offset, false)
                .map(|word| WasmValue::from_bits(access.ty, word));
            assert_eq!(staged, declared, "{operator:?} {address} {offset}");
        }
    }
}

#[test]
fn a_store_of_the_stage_is_the_declared_write() {
    let words = [0, 1, u64::MAX, 0x1234_5678_9abc_def0, 0x8000_0000];
    for (operator, access) in scalar_stores() {
        for (address, word) in ADDRESSES
            .iter()
            .flat_map(|a| words.iter().map(move |w| (*a, *w)))
        {
            let value = WasmValue::from_bits(access.ty, word);
            let (mut declared_bytes, mut staged_bytes) = (pattern(), pattern());
            let declared = operator.write(&mut declared_bytes, u64::from(address), value);
            let mut stage = instance(&mut staged_bytes);
            let index = stage.memory_index(u64::from(address));
            let staged = store(&mut stage, access, index, 0, value.word(), false);
            assert_eq!(staged, declared, "{operator:?} {address} {word:#x}");
            assert_eq!(
                staged_bytes, declared_bytes,
                "{operator:?} {address} {word:#x}"
            );
        }
    }
}

#[test]
fn memory_size_is_the_length_in_pages() {
    for pages in [0, 1, 3] {
        let mut bytes = vec![0; pages * WASM_PAGE_BYTES];
        assert_eq!(size(&mut instance(&mut bytes), WASM_PAGE_BITS), pages as u64);
    }
}

#[test]
fn memory_size_counts_the_memory_type_pages() {
    let mut bytes = vec![0; 24];
    assert_eq!(size(&mut instance(&mut bytes), 0), 24);
    assert_eq!(size(&mut instance(&mut bytes), 3), 3);
}

const LENGTHS: [u32; 6] = [0, 4, 5, 24, 25, u32::MAX];

#[test]
fn a_fill_of_the_stage_is_the_declared_range() {
    for (destination, length) in ADDRESSES
        .iter()
        .flat_map(|d| LENGTHS.iter().map(move |n| (*d, *n)))
    {
        let (mut declared, mut staged) = (pattern(), pattern());
        let range = checked_range(u64::from(destination), length as usize, declared.len());
        let expected = range.map(|range| declared[range].fill(0xab));
        let result = fill(
            &mut instance(&mut staged),
            destination.into(),
            0xab,
            length.into(),
        );
        assert_eq!(result.is_ok(), expected.is_ok(), "{destination} {length}");
        assert_eq!(result.err(), expected.err(), "{destination} {length}");
        assert_eq!(staged, declared, "{destination} {length}");
    }
}

#[test]
fn a_copy_of_the_stage_is_the_declared_range() {
    for ((destination, source), length) in ADDRESSES
        .iter()
        .flat_map(|d| ADDRESSES.iter().map(move |s| (*d, *s)))
        .flat_map(|ranges| LENGTHS.iter().map(move |n| (ranges, *n)))
    {
        let (mut declared, mut staged) = (pattern(), pattern());
        let end = declared.len();
        let ranges = checked_range(u64::from(destination), length as usize, end)
            .and_then(|to| Ok((to, checked_range(u64::from(source), length as usize, end)?)));
        let expected = ranges.map(|(to, from)| declared.copy_within(from, to.start));
        let result = copy(
            &mut instance(&mut staged),
            destination.into(),
            source.into(),
            length.into(),
        );
        assert_eq!(
            result.is_ok(),
            expected.is_ok(),
            "{destination} {source} {length}"
        );
        assert_eq!(
            result.err(),
            expected.err(),
            "{destination} {source} {length}"
        );
        assert_eq!(staged, declared, "{destination} {source} {length}");
    }
}

impl Probe {
    fn trace(&mut self, entry: &str) -> usize {
        self.log.push(entry.to_owned());
        self.log.len() - 1
    }
}

impl WasmMemory for Probe {
    type Vector = ();

    fn guarded(&self) -> bool {
        self.guarded
    }

    fn memory_length(&mut self) -> usize {
        self.trace("length")
    }

    fn memory_index(&mut self, _: usize) -> usize {
        self.trace("index")
    }

    fn memory_load(&mut self, _: Access, _: usize, _: u32) -> usize {
        self.trace("load")
    }

    fn memory_load_vector(&mut self, _: usize, _: u32) {
        self.trace("load vector");
    }
}

impl WasmMemoryWrite for Probe {
    fn memory_store(&mut self, _: Access, _: usize, _: u32, _: usize) {
        self.trace("store");
    }

    fn memory_store_vector(&mut self, _: usize, _: u32, _: ()) {
        self.trace("store vector");
    }

    fn memory_fill(&mut self, _: usize, _: usize, _: usize) {
        self.trace("fill");
    }

    fn memory_copy(&mut self, _: usize, _: usize, _: usize) {
        self.trace("copy");
    }
}

const CHECK: [&str; 4] = [
    "Add I64",
    "length",
    "GreaterUnsigned I64",
    "trap OutOfBoundsMemory",
];

#[test]
fn a_copy_checks_the_destination_then_the_source_then_moves() {
    let mut probe = Probe::default();
    copy(&mut probe, 0, 0, 0).expect("the probe never traps");
    let mut expected = vec!["index"; 3];
    expected.extend(CHECK.iter().chain(&CHECK));
    expected.push("copy");
    assert_eq!(probe.log, expected);
}

#[test]
fn a_fill_checks_its_range_then_writes() {
    let mut probe = Probe::default();
    fill(&mut probe, 0, 0, 0).expect("the probe never traps");
    let mut expected = vec!["index"; 2];
    expected.extend(CHECK);
    expected.push("fill");
    assert_eq!(probe.log, expected);
}

#[test]
fn an_access_checks_its_extent_unless_the_guard_covers_it() {
    let access = Access {
        ty: WasmType::I32,
        bytes: 4,
        signed: false,
    };
    let checked = |guarded: bool, proven: bool| {
        let mut probe = Probe {
            guarded,
            ..Probe::default()
        };
        load(&mut probe, access, 0, 8, proven).expect("the probe never traps");
        probe.log
    };
    let mut full = vec!["constant I64 12".to_owned()];
    full.extend(CHECK.map(str::to_owned));
    full.push("load".into());
    assert_eq!(checked(false, true), full);
    assert_eq!(checked(true, false), full);
    assert_eq!(checked(true, true), ["load"]);
}

#[test]
fn memory_size_shifts_the_length_by_the_page_bits() {
    let mut probe = Probe::default();
    size(&mut probe, WASM_PAGE_BITS);
    assert_eq!(
        probe.log,
        [
            "length",
            "constant I64 16",
            "ShiftRightUnsigned I64",
            "WrapI64"
        ]
    );
}

#[test]
fn the_table_names_every_direct_access_and_bulk_operation() {
    for (operator, access) in scalar_loads() {
        let Some(op) = operator.direct_op() else {
            panic!("{operator:?} has a scalar access but no direct opcode");
        };
        assert_eq!(Memory::of(op), Some(Memory::Load(access)), "{operator:?}");
    }
    for (operator, access) in scalar_stores() {
        if let Some(op) = operator.direct_op() {
            assert_eq!(Memory::of(op), Some(Memory::Store(access)), "{operator:?}");
        }
    }
    assert_eq!(Memory::of(Op::WasmMemorySize), Some(Memory::Size));
    assert_eq!(Memory::of(Op::WasmMemoryFill), Some(Memory::Fill));
    assert_eq!(Memory::of(Op::WasmMemoryCopy), Some(Memory::Copy));
    assert_eq!(Memory::of(Op::WasmI32Add), None);
    assert_eq!(Access::of_load(MemoryLoad::V128Load), None);
    assert!(Step::of(Op::WasmI64Load32S, 0).is_some_and(Step::reads_memory));
    assert!(!Step::of(Op::WasmI32Add, 0).is_some_and(Step::reads_memory));
    let narrow = Memory::of(Op::WasmI64Load32S);
    assert_eq!(
        narrow,
        Some(Memory::Load(Access {
            ty: WasmType::I64,
            bytes: 4,
            signed: true
        }))
    );
}

#[test]
fn a_trap_is_out_of_bounds_memory() {
    let mut bytes = pattern();
    let mut stage = instance(&mut bytes);
    let access = Access {
        ty: WasmType::I32,
        bytes: 4,
        signed: false,
    };
    let index = stage.memory_index(MEMORY_BYTES as u64 - 3);
    assert_eq!(
        load(&mut stage, access, index, 0, false),
        Err(WasmTrap::OutOfBoundsMemory)
    );
}

#[test]
fn a_narrow_load_extends_its_little_endian_bytes() {
    let bytes = [0x80, 0xff, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
    let read = |load: MemoryLoad| load.read(&bytes, 0);
    assert_eq!(read(MemoryLoad::I32Load8S), Ok(WasmValue::I32(-0x80)));
    assert_eq!(read(MemoryLoad::I32Load8U), Ok(WasmValue::I32(0x80)));
    assert_eq!(read(MemoryLoad::I32Load16S), Ok(WasmValue::I32(-0x80)));
    assert_eq!(read(MemoryLoad::I64Load32U), Ok(WasmValue::I64(0x0201_ff80)));
    assert_eq!(read(MemoryLoad::I64Load32S), Ok(WasmValue::I64(0x0201_ff80)));
    assert_eq!(
        read(MemoryLoad::I64Load),
        Ok(WasmValue::I64(0x0605_0403_0201_ff80))
    );
}

#[test]
fn an_address_that_wraps_the_word_is_out_of_bounds() {
    let bytes = pattern();
    let last = u64::MAX - 1;
    assert_eq!(
        MemoryLoad::I32Load.read(&bytes, last),
        Err(WasmTrap::OutOfBoundsMemory)
    );
    let mut written = pattern();
    assert_eq!(
        MemoryStore::I32Store.write(&mut written, last, WasmValue::I32(1)),
        Err(WasmTrap::OutOfBoundsMemory)
    );
    assert_eq!(written, bytes);
}

#[test]
fn a_growth_names_its_memory_and_page_count_and_calls_out() {
    assert_eq!(Memory::of(Op::WasmMemoryGrow), Some(Memory::Grow));
    let grow = WideInstruction::new(Op::WasmMemoryGrow, 1, 2, 3, 0);
    assert_eq!(Memory::Grow.memories(grow), [Some(2), None]);
    assert_eq!(Memory::Grow.operands(grow), [Some(3), None, None]);
    assert!(Memory::Grow.needs_compiled_memory());
    let callouts: Vec<_> = [Memory::Size, Memory::Fill, Memory::Copy, Memory::Grow]
        .into_iter()
        .filter(|step| step.needs_callouts())
        .collect();
    assert_eq!(callouts, [Memory::Grow]);
}

#[test]
fn a_growth_returns_the_pages_before_and_a_failed_one_changes_nothing() {
    let (page, maximum) = (WASM_PAGE_BYTES, 4 * WASM_PAGE_BYTES);
    let mut bytes = LinearBytes::heap(vec![0; 2 * page]);
    assert_eq!(bytes.grow_pages(1, page, maximum), Some(2));
    assert_eq!(bytes.len(), 3 * page);
    assert_eq!(bytes.grow_pages(2, page, maximum), None);
    assert_eq!(bytes.len(), 3 * page);
    assert_eq!(bytes.grow_pages(0, page, maximum), Some(3));
    assert_eq!(bytes.grow_pages(u64::MAX, page, usize::MAX), None);
    assert_eq!(bytes.len(), 3 * page);
}
