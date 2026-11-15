use super::super::super::Vm;
use crate::{WasmTrap, WasmValue};
use wasmparser::{MemArg, Operator, ValType};

const SOURCE: i32 = 64;
const TARGET: i32 = 256;
const PAGE: i32 = 65_536;
/// The lanes the source vectors hold: carries out of every lane, sign bits, and a low
/// word whose products overflow u32.
const LEFT: [u64; 2] = [u64::MAX, 0x8000_0000_0000_0001];
const RIGHT: [u64; 2] = [1, 0x8000_0000_ffff_ffff];

fn vector(offset: u64) -> MemArg {
    MemArg { align: 4, max_align: 4, offset, memory: 0 }
}
fn word(offset: u64) -> MemArg {
    MemArg { align: 2, max_align: 2, offset, memory: 0 }
}
fn local(index: u32) -> Operator<'static> {
    Operator::LocalGet { local_index: index }
}
fn constant(value: i32) -> Operator<'static> {
    Operator::I32Const { value }
}
fn lanes([low, high]: [u64; 2]) -> u128 {
    u128::from(low) | (u128::from(high) << 64)
}
/// `_mm_mul_epu32`'s shuffle: the low u32 of each u64 lane, twice.
fn low_words() -> [u8; 16] {
    std::array::from_fn(|byte| [0u8, 2][(byte / 4) % 2] * 4 + (byte % 4) as u8)
}

/// `body` storing the vector its operators leave at `TARGET + slot * 16`.
fn store_at(slot: u64, mut body: Vec<Operator<'static>>) -> Vec<Operator<'static>> {
    let mut operators = vec![local(1)];
    operators.append(&mut body);
    operators.push(Operator::V128Store { memarg: vector(slot * 16) });
    operators
}

/// Straight-line i64x2 arithmetic, shifts past the lane width (folded and dynamic),
/// rotations by 0, 64 and 24 in their shift-or form, and the shuffled extending
/// multiply: `lanes(source, target, count)`, the source vectors in locals 3 and 4.
fn lane_body() -> Vec<Operator<'static>> {
    let (v, w) = (|| local(3), || local(4));
    let rotation = |left: i32, right: i32| {
        vec![v(), constant(left), Operator::I64x2Shl, v(), constant(right), Operator::I64x2ShrU, Operator::V128Or]
    };
    let mut body = vec![
        local(0),
        Operator::V128Load { memarg: vector(0) },
        Operator::LocalSet { local_index: 3 },
        local(0),
        Operator::V128Load { memarg: vector(16) },
        Operator::LocalSet { local_index: 4 },
    ];
    for (slot, operators) in [
        vec![v(), w(), Operator::I64x2Add],
        vec![v(), w(), Operator::I64x2Sub],
        vec![v(), w(), Operator::I64x2Mul],
        vec![v(), constant(69), Operator::I64x2Shl],
        vec![v(), local(2), Operator::I64x2ShrS],
        vec![v(), constant(64), Operator::I64x2ShrU],
        rotation(0, 64),
        rotation(64, 0),
        rotation(40, 24),
        vec![
            v(),
            v(),
            Operator::I8x16Shuffle { lanes: low_words() },
            w(),
            w(),
            Operator::I8x16Shuffle { lanes: low_words() },
            Operator::I64x2ExtMulLowI32x4U,
        ],
        vec![v(), w(), Operator::I32x4Add],
    ]
    .into_iter()
    .enumerate()
    {
        body.extend(store_at(slot as u64, operators));
    }
    body.push(Operator::End);
    body
}

/// What `lane_body` stores, slot by slot, for shift count `count`.
fn expected_lanes(count: i32) -> Vec<u128> {
    let map = |operation: &dyn Fn(u64, u64) -> u64| {
        lanes([operation(LEFT[0], RIGHT[0]), operation(LEFT[1], RIGHT[1])])
    };
    let words = |value: u64| [value as u32, (value >> 32) as u32];
    let (left, right) = (lanes(LEFT), lanes(RIGHT));
    let quads: [u32; 4] = std::array::from_fn(|lane| {
        ((left >> (32 * lane)) as u32).wrapping_add((right >> (32 * lane)) as u32)
    });
    vec![
        map(&|a, b| a.wrapping_add(b)),
        map(&|a, b| a.wrapping_sub(b)),
        map(&|a, b| a.wrapping_mul(b)),
        map(&|a, _| a << (69 % 64)),
        map(&|a, _| ((a as i64) >> (count as u32 % 64)) as u64),
        map(&|a, _| a),
        map(&|a, _| a),
        map(&|a, _| a),
        map(&|a, _| a.rotate_left(40)),
        map(&|a, b| u64::from(words(a)[0]) * u64::from(words(b)[0])),
        quads.iter().enumerate().fold(0, |bits, (lane, &value)| bits | (u128::from(value) << (32 * lane))),
    ]
}

/// A vector doubled and stored, an i32 division whose trap leaves it stored, and the
/// vector, live across the division, stored again plus one:
/// `divide(target, dividend, divisor)`.
fn division_body() -> Vec<Operator<'static>> {
    vec![
        local(0),
        Operator::V128Load { memarg: vector(0) },
        Operator::LocalTee { local_index: 3 },
        local(3),
        Operator::I64x2Add,
        Operator::LocalSet { local_index: 3 },
        local(0),
        local(3),
        Operator::V128Store { memarg: vector(0) },
        local(1),
        local(2),
        Operator::I32DivS,
        Operator::LocalSet { local_index: 4 },
        local(0),
        local(3),
        local(3),
        Operator::I64x2Add,
        Operator::V128Store { memarg: vector(16) },
        local(0),
        local(4),
        Operator::I32Store { memarg: word(32) },
        Operator::End,
    ]
}

/// A vector loaded from `source` and stored at `target`: `copy(source, target)`.
fn copy_body() -> Vec<Operator<'static>> {
    vec![
        local(1),
        local(0),
        Operator::V128Load { memarg: vector(0) },
        local(0),
        Operator::V128Load { memarg: vector(0) },
        Operator::I64x2Add,
        Operator::V128Store { memarg: vector(0) },
        Operator::End,
    ]
}

struct Fixture {
    vm: Vm<crate::SystemHost>,
    instance: crate::WasmInstance,
    module: crate::WasmModule,
}

const LANES: u32 = 0;
const STORE: u32 = 1;
const LOAD: u32 = 2;
const DIVIDE: u32 = 3;
const COPY: u32 = 4;

fn fixture(mode: crate::ExecutionMode) -> Fixture {
    let body = |params: usize, locals: Vec<ValType>, result: bool, operators: Vec<Operator<'static>>| {
        crate::WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(crate::WasmSignature {
                params: vec![crate::WasmType::I32; params],
                results: if result { vec![crate::WasmType::I32] } else { vec![] },
            }),
            locals,
            operators: operators.into_iter().map(Ok),
        }
    };
    let module = crate::Engine::lower_wasm_module_with_state(
        "vector-regions",
        [
            body(3, vec![ValType::V128, ValType::V128], false, lane_body()),
            body(2, vec![], false, vec![local(0), local(1), Operator::I32Store { memarg: word(0) }, Operator::End]),
            body(1, vec![], true, vec![local(0), Operator::I32Load { memarg: word(0) }, Operator::End]),
            body(3, vec![ValType::V128, ValType::I32], false, division_body()),
            body(2, vec![], false, copy_body()),
        ],
        &crate::WasmTypes::default(),
        &[],
        &[crate::WasmMemory {
            ty: wasmparser::MemoryType {
                initial: 1,
                maximum: Some(1),
                memory64: false,
                shared: false,
                page_size_log2: None,
            },
            import: None,
        }],
        &[],
    )
    .unwrap();
    let mut vm = Vm::new(crate::SystemHost);
    vm.programs.compile_wasm_modules_at_first_entry();
    vm.set_execution_mode(mode);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    Fixture { vm, instance, module }
}

impl Fixture {
    fn call(&mut self, function: u32, args: &[i32]) -> Result<Option<WasmValue>, crate::JsError> {
        let args: Vec<WasmValue> = args.iter().copied().map(WasmValue::I32).collect();
        self.vm.invoke_wasm(&self.instance, function, &args)
    }
    fn write(&mut self, address: i32, bits: u128) {
        for index in 0..4 {
            self.call(STORE, &[address + index * 4, (bits >> (32 * index)) as u32 as i32]).unwrap();
        }
    }
    fn read(&mut self, address: i32) -> u128 {
        (0..4).fold(0, |bits, index| {
            let Some(WasmValue::I32(word)) = self.call(LOAD, &[address + index * 4]).unwrap() else {
                unreachable!("an i32 load")
            };
            bits | (u128::from(word as u32) << (32 * index))
        })
    }
    /// How many vector regions `function`'s lane view holds.
    fn regions(&self, function: u32) -> usize {
        let program = self.vm.programs.find_shared(&self.module.program).expect("the module's program");
        let view = self.vm.lane_view_any(program, function).expect("a lane view");
        // SAFETY: views live as long as their program entry.
        unsafe { &*view }.vector_regions.len()
    }
}

#[test]
fn vector_regions_compute_lanes_exactly() {
    for count in [0, 5, 63, 64, 70, -1] {
        for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
            let mut fixture = fixture(mode);
            fixture.write(SOURCE, lanes(LEFT));
            fixture.write(SOURCE + 16, lanes(RIGHT));
            fixture.call(LANES, &[SOURCE, TARGET, count]).unwrap();
            let stored: Vec<u128> = (0..11).map(|slot| fixture.read(TARGET + slot * 16)).collect();
            assert_eq!(stored, expected_lanes(count), "count {count} in {mode:?}");
        }
    }
    let fixture = fixture(crate::ExecutionMode::Interpreted);
    assert!(fixture.regions(LANES) > 0, "the lane body runs as a vector region");
}

#[test]
fn vector_region_traps_leave_earlier_effects_and_boxed_vectors() {
    let doubled = lanes([LEFT[0].wrapping_add(LEFT[0]), LEFT[1].wrapping_add(LEFT[1])]);
    let quadrupled = lanes([(doubled as u64).wrapping_mul(2), ((doubled >> 64) as u64).wrapping_mul(2)]);
    for (dividend, divisor, trap) in [
        (7, 0, Some(WasmTrap::IntegerDivideByZero)),
        (i32::MIN, -1, Some(WasmTrap::IntegerOverflow)),
        (i32::MIN, 1, None),
        (-7, 2, None),
    ] {
        let mut fixture = fixture(crate::ExecutionMode::Interpreted);
        fixture.write(TARGET, lanes(LEFT));
        fixture.write(TARGET + 16, 0);
        let result = fixture.call(DIVIDE, &[TARGET, dividend, divisor]);
        assert_eq!(fixture.read(TARGET), doubled, "the store before the division");
        match trap {
            Some(trap) => {
                assert_eq!(result.unwrap_err().wasm_trap(), Some(trap), "{dividend} / {divisor}");
                assert_eq!(fixture.read(TARGET + 16), 0, "nothing after the trap");
            }
            None => {
                result.unwrap();
                assert_eq!(fixture.read(TARGET + 16), quadrupled, "the vector live across the division");
                let Some(WasmValue::I32(quotient)) = fixture.call(LOAD, &[TARGET + 32]).unwrap() else {
                    unreachable!("an i32 load")
                };
                assert_eq!(quotient, dividend.wrapping_div(divisor));
            }
        }
    }
    assert!(fixture(crate::ExecutionMode::Interpreted).regions(DIVIDE) > 0);
}

#[test]
fn vector_region_accesses_trap_out_of_bounds() {
    let mut fixture = fixture(crate::ExecutionMode::Interpreted);
    fixture.write(SOURCE, lanes(LEFT));
    let last = PAGE - 16;
    fixture.write(last, lanes(RIGHT));
    // A store whose last bytes pass the end traps and writes nothing; a load does too.
    for (source, target) in [(SOURCE, last + 1), (SOURCE, PAGE - 8), (SOURCE, -16), (last + 1, TARGET), (PAGE, TARGET)] {
        let error = fixture.call(COPY, &[source, target]).unwrap_err();
        assert_eq!(error.wasm_trap(), Some(WasmTrap::OutOfBoundsMemory), "copy {source} -> {target}");
        assert_eq!(fixture.read(last), lanes(RIGHT), "memory past the trap is unchanged");
    }
    // The last whole vector of the memory is in bounds.
    fixture.call(COPY, &[SOURCE, last]).unwrap();
    let doubled = lanes([LEFT[0].wrapping_add(LEFT[0]), LEFT[1].wrapping_add(LEFT[1])]);
    assert_eq!(fixture.read(last), doubled);
    assert!(fixture.regions(COPY) > 0);
}
