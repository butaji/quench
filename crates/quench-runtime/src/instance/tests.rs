use super::{store_bytes, Instance, InvokeError};
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};
use wasmparser::WasmFeatures;

fn leb(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return out;
        }
    }
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![id];
    out.extend(leb(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn type_section(types: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut payload = leb(types.len() as u64);
    for (params, results) in types {
        payload.push(0x60);
        payload.extend(leb(params.len() as u64));
        payload.extend_from_slice(params);
        payload.extend(leb(results.len() as u64));
        payload.extend_from_slice(results);
    }
    section(1, &payload)
}

fn function_section(types: &[u32]) -> Vec<u8> {
    let mut payload = leb(types.len() as u64);
    for ty in types {
        payload.extend(leb(*ty as u64));
    }
    section(3, &payload)
}

fn code_section(bodies: &[&[u8]]) -> Vec<u8> {
    let mut payload = leb(bodies.len() as u64);
    for body in bodies {
        let size = 1 + body.len(); // local-declaration vector + operators
        payload.extend(leb(size as u64));
        payload.push(0); // no local declarations
        payload.extend_from_slice(body);
    }
    section(10, &payload)
}

fn export_section(exports: &[(&str, u8, u32)]) -> Vec<u8> {
    let mut payload = leb(exports.len() as u64);
    for (name, kind, index) in exports {
        payload.extend(leb(name.len() as u64));
        payload.extend_from_slice(name.as_bytes());
        payload.push(*kind);
        payload.extend(leb(*index as u64));
    }
    section(7, &payload)
}

fn memory_section(initial: u32, maximum: Option<u32>) -> Vec<u8> {
    let mut payload = vec![1];
    payload.push(u8::from(maximum.is_some()));
    payload.extend(leb(initial as u64));
    if let Some(maximum) = maximum {
        payload.extend(leb(maximum as u64));
    }
    section(5, &payload)
}

fn global_section(value: i32, mutable: bool) -> Vec<u8> {
    let mut payload = vec![1, 0x7f, u8::from(mutable), 0x41];
    payload.extend(leb(value as u64));
    payload.push(0x0b);
    section(6, &payload)
}

fn start_section(function: u32) -> Vec<u8> {
    section(8, &leb(function as u64))
}

fn data_section(bytes: &[u8]) -> Vec<u8> {
    let mut payload = vec![1, 0, 0x41, 0, 0x0b];
    payload.extend(leb(bytes.len() as u64));
    payload.extend_from_slice(bytes);
    section(11, &payload)
}

fn table_section(initial: u32) -> Vec<u8> {
    let mut payload = vec![1, 0x70, 0];
    payload.extend(leb(initial as u64));
    section(4, &payload)
}

fn active_funcref_element(function: u32) -> Vec<u8> {
    let mut payload = vec![1, 0, 0x41, 0, 0x0b, 1];
    payload.extend(leb(function as u64));
    section(9, &payload)
}

fn module(mut sections: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    for section in sections.drain(..) {
        bytes.extend(section);
    }
    bytes
}

fn instantiate(bytes: &[u8]) -> Instance {
    Instance::from_bytes(bytes, WasmFeatures::all(), |_, _, _, _| {
        Err(InvokeError::Unlinkable("test module has no imports"))
    })
    .expect("instantiate")
}

#[test]
fn invoke_is_typed_and_returns_native_values() {
    let bytes = module(vec![
        type_section(&[(&[0x7f, 0x7f], &[0x7f])]),
        function_section(&[0]),
        export_section(&[("add", 0, 0)]),
        code_section(&[&[0x20, 0, 0x20, 1, 0x6a, 0x0b]]),
    ]);
    let instance = instantiate(&bytes);
    let result = instance
        .invoke("add", &[Slot::native_i32(20), Slot::native_i32(22)])
        .expect("invoke");
    assert_eq!(result, vec![Slot::native_i32(42)]);
    assert_eq!(
        instance.invoke("add", &[Slot::native_i32(1)]),
        Err(InvokeError::TypeMismatch)
    );
    assert_eq!(
        instance.invoke("add", &[Slot::Native(Native::I64(1)), Slot::native_i32(2)]),
        Err(InvokeError::TypeMismatch)
    );
}

#[test]
fn integer_division_by_zero_is_a_wasm_trap() {
    let bytes = module(vec![
        type_section(&[(&[], &[0x7f])]),
        function_section(&[0]),
        export_section(&[("divide", 0, 0)]),
        code_section(&[&[0x41, 1, 0x41, 0, 0x6d, 0x0b]]),
    ]);
    let instance = instantiate(&bytes);
    assert_eq!(
        instance.invoke("divide", &[]),
        Err(InvokeError::Failure(Failure::Trap(
            Trap::IntegerDivideByZero
        )))
    );
}

#[test]
fn instantiation_runs_start_and_applies_active_data() {
    // The start function runs before the instance is returned; active data is
    // visible in linear memory before the first invocation as required by the
    // module instantiation algorithm.
    let bytes = module(vec![
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        memory_section(1, Some(1)),
        global_section(0, true),
        export_section(&[("g", 3, 0), ("memory", 2, 0)]),
        start_section(0),
        code_section(&[&[0x41, 42, 0x24, 0, 0x0b]]),
        data_section(b"wasm"),
    ]);
    let instance = instantiate(&bytes);
    assert_eq!(instance.get_global("g"), Ok(Slot::native_i32(42)));
    let memory = instance.memory(0).expect("memory");
    assert_eq!(&memory.borrow().data[..4], b"wasm");
}

#[test]
fn linear_memory_is_little_endian_and_bounds_checked() {
    let bytes = module(vec![memory_section(1, Some(1))]);
    let instance = instantiate(&bytes);
    store_bytes(&instance, 0, 2, 0, &[0x78, 0x56, 0x34, 0x12]).expect("store");
    assert_eq!(
        super::load_bytes(&instance, 0, 2, 0, 4).expect("load"),
        vec![0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(
        super::load_bytes(&instance, 0, u64::MAX, 0, 1),
        Err(Trap::OutOfBoundsMemory)
    );
    assert_eq!(
        store_bytes(&instance, 0, 65_535, 0, &[1, 2]),
        Err(Trap::OutOfBoundsMemory)
    );
}

#[test]
fn memory_grow_returns_old_pages_and_minus_one_on_failure() {
    let bytes = module(vec![
        type_section(&[(&[], &[0x7f])]),
        function_section(&[0]),
        memory_section(1, Some(2)),
        export_section(&[("grow", 0, 0)]),
        code_section(&[&[0x41, 1, 0x40, 0, 0x0b]]),
    ]);
    let instance = instantiate(&bytes);
    assert_eq!(instance.invoke("grow", &[]), Ok(vec![Slot::native_i32(1)]));
    assert_eq!(instance.invoke("grow", &[]), Ok(vec![Slot::native_i32(-1)]));
    assert_eq!(instance.memory(0).unwrap().borrow().pages(), 2);
}

#[test]
fn call_indirect_uses_funcref_table_and_reports_null_entries() {
    let bytes = module(vec![
        type_section(&[(&[], &[0x7f]), (&[], &[0x7f])]),
        function_section(&[0, 1]),
        table_section(1),
        export_section(&[("call", 0, 1)]),
        active_funcref_element(0),
        code_section(&[&[0x41, 42, 0x0b], &[0x41, 0, 0x11, 0, 0, 0x0b]]),
    ]);
    let instance = instantiate(&bytes);
    assert_eq!(instance.invoke("call", &[]), Ok(vec![Slot::native_i32(42)]));

    let null_bytes = module(vec![
        type_section(&[(&[], &[0x7f])]),
        function_section(&[0]),
        table_section(1),
        export_section(&[("call", 0, 0)]),
        code_section(&[&[0x41, 0, 0x11, 0, 0, 0x0b]]),
    ]);
    let null_instance = instantiate(&null_bytes);
    assert_eq!(
        null_instance.invoke("call", &[]),
        Err(InvokeError::Failure(Failure::Trap(
            Trap::UninitializedElement
        )))
    );
}

#[test]
fn ref_null_is_the_zero_funcref_value() {
    let bytes = module(vec![table_section(1)]);
    let instance = instantiate(&bytes);
    assert_eq!(
        instance.table(0).unwrap().borrow().elems,
        vec![RefVal::Null]
    );
}
