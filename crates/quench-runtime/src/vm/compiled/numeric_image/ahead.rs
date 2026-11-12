//! A whole lane function compiled for the target "ahead" (`LaneHelpers::Ahead`): every helper
//! a symbol, linked by relocation into a process that did not compile it.
use super::super::lane_region::{relocs, Completion, Coverage, ElementViews, NO_ENTRY_FIELDS, lower};
use super::*;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use std::collections::BTreeMap;

/// `source`'s function 1 lowered ahead and compiled to an image, with its caller registers.
fn ahead_image(source: &str) -> (RelocatableCode, usize) {
    let program = crate::Engine::specialize(source, "ahead-image.js").unwrap();
    let unit = lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let isa = crate::vm::jit_backend::target_isa().unwrap();
    let region = lower(&program, 1, &unit, LaneHelpers::Ahead, &BTreeMap::new(), &Default::default(),
        VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[],
        Coverage::Full, Completion::Body, ElementViews::Dense).unwrap();
    let image = crate::vm::jit_backend::compile_function(region.function, &*isa, None, |compiled, function| {
        RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
    }).unwrap();
    (image, usize::from(unit.caller_registers))
}

/// `pick`'s inputs and the results the entry returns for them.
const PICKS: [(f64, f64); 3] = [(0.0, 2.0), (5.0, 1.0), (-1.5, 1.0)];

/// Runs `linked` as a lane entry over `pick`'s inputs and checks each result.
fn run_pick(linked: &[u8], registers: usize) {
    let memory = CodeMemory::new(linked).unwrap();
    // SAFETY: the image is a lane entry of the shared ABI, its helpers now this process's.
    let entry = unsafe { std::mem::transmute::<*mut u8, super::super::BodyEntry>(memory.pointer()) };
    let mut frame = super::super::LaneFrame::new();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    frame.dispatch.refresh_object_bases(&vm);
    frame.dispatch.bind_native_storage(&mut vm);
    frame.dispatch.receiver = Value::UNDEFINED.to_bits();
    frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
    for (input, expected) in PICKS {
        let mut words = vec![Value::UNDEFINED; registers];
        words[0] = Value::number(input);
        let super::super::BodyReturn { status, value: word } = unsafe { entry(&mut frame.context, 0, 0, words.as_mut_ptr().cast()) };
        assert_eq!(status, super::super::LANE_BODY_RETURNED, "{input}");
        assert_eq!(Value::from_bits(word), Value::number(expected), "{input}");
    }
}

/// The image linked against this process's helper table.
fn link_here(image: &RelocatableCode) -> Vec<u8> {
    let table = LaneHelper::addresses::<crate::test_host::TestHost>();
    image.link(0, |namespace, key| match relocs::atom_key(namespace, key) {
        Some(atom) => Some(u64::from(atom)),
        None => table.get(relocs::helper_key(namespace, key)?).map(|address| *address as usize as u64),
    }).unwrap()
}

/// The image as a file: code, then each relocation as offset, kind, namespace, key, addend.
fn write_image(image: &RelocatableCode, registers: usize) -> Vec<u8> {
    let mut out = (registers as u64).to_le_bytes().to_vec();
    out.extend((image.code.len() as u64).to_le_bytes());
    out.extend(&*image.code);
    out.extend((image.relocations.len() as u64).to_le_bytes());
    for relocation in &*image.relocations {
        assert_eq!(relocation.kind, cranelift_codegen::binemit::Reloc::Abs8, "a symbol is an absolute word");
        out.extend((relocation.offset as u64).to_le_bytes());
        out.extend(relocation.namespace.to_le_bytes());
        out.extend(relocation.key.to_le_bytes());
        out.extend(relocation.addend.to_le_bytes());
    }
    out
}

/// A reader of the words `write_image` wrote.
struct Words<'a>(&'a [u8]);

impl Words<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let (head, rest) = self.0.split_at(N);
        self.0 = rest;
        head.try_into().unwrap()
    }
    fn word(&mut self) -> u64 { u64::from_le_bytes(self.take()) }
    fn half(&mut self) -> u32 { u32::from_le_bytes(self.take()) }
}

/// The image `write_image` wrote, and its caller registers.
fn read_image(file: &[u8]) -> (RelocatableCode, usize) {
    let mut words = Words(file);
    let registers = words.word() as usize;
    let length = words.word() as usize;
    let code: Box<[u8]> = words.0[..length].into();
    words.0 = &words.0[length..];
    let relocations = (0..words.word()).map(|_| crate::vm::jit_backend::relocatable_code::Relocation {
        offset: words.word() as usize, kind: cranelift_codegen::binemit::Reloc::Abs8,
        namespace: words.half(), key: words.half(), addend: words.word() as i64 }).collect();
    (RelocatableCode { code, relocations }, registers)
}

#[test]
fn a_lane_function_compiled_ahead_names_only_helper_symbols_and_runs_once_linked() {
    let (image, registers) = ahead_image("function pick(o){return o?1:2;}");
    assert!(!image.relocations.is_empty(), "the truthiness helper is a symbol");
    assert!(image.relocations.iter().all(|relocation| relocs::helper_key(relocation.namespace, relocation.key).is_some()),
        "no address but a helper's is named: {:?}", image.relocations);
    run_pick(&link_here(&image), registers);
}

/// The image of `pick` written to a file and run by a second process, whose helpers live at
/// other addresses: the file holds no address of the process that compiled it.
#[test]
fn a_lane_image_written_out_is_linked_and_run_by_a_fresh_process() {
    const FILE: &str = "QUENCH_AHEAD_IMAGE";
    if let Ok(path) = std::env::var(FILE) {
        let (image, registers) = read_image(&std::fs::read(path).unwrap());
        run_pick(&link_here(&image), registers);
        println!("ahead image ran in {}", std::process::id());
        return;
    }
    let (image, registers) = ahead_image("function pick(o){return o?1:2;}");
    let path = std::env::temp_dir().join(format!("quench-ahead-{}.img", std::process::id()));
    std::fs::write(&path, write_image(&image, registers)).unwrap();
    let name = format!("{}::a_lane_image_written_out_is_linked_and_run_by_a_fresh_process", module_path!().split_once("::").unwrap().1);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture", "--test-threads=1"]).env(FILE, &path).output().unwrap();
    std::fs::remove_file(&path).ok();
    let stdout = String::from_utf8_lossy(&child.stdout);
    assert!(child.status.success(), "{stdout}{}", String::from_utf8_lossy(&child.stderr));
    assert!(stdout.contains("ahead image ran in") && !stdout.contains(&format!("ran in {}", std::process::id())), "{stdout}");
}

/// What each of these names ahead, from static facts alone: field reads, calls and global
/// reads lower to helper calls, so their images carry helper symbols and nothing else.
const WIDER: [&str; 3] = ["function f(o){return o.x+1;}",
    "function g(a){return a+1;} function f(x){return g(x)+g(2);}", "var k=3; function f(x){return x+k;}"];

#[test]
fn field_reads_calls_and_global_reads_compile_ahead_to_helper_symbols_only() {
    for source in WIDER {
        let program = crate::Engine::specialize(source, "ahead-wider.js").unwrap();
        let id = program.function_count() - 1;
        let unit = lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(), &|_| None).unwrap();
        let isa = crate::vm::jit_backend::target_isa().unwrap();
        let region = lower(&program, id as u32, &unit, LaneHelpers::Ahead, &BTreeMap::new(), &Default::default(),
            VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[],
            Coverage::Full, Completion::Body, ElementViews::Dense).expect(source);
        let image = crate::vm::jit_backend::compile_function(region.function, &*isa, None, |compiled, function| {
            RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
        }).expect(source);
        assert!(!image.relocations.is_empty(), "{source}: a helper call is a symbol");
        assert!(image.relocations.iter().all(|relocation| relocs::helper_key(relocation.namespace, relocation.key).is_some() || relocs::atom_key(relocation.namespace, relocation.key).is_some()), "{source}: {:?}", image.relocations);
        link_here(&image);
    }
}
