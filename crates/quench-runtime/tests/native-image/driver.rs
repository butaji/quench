//! Host adapter for the native-image integration test; guest code is in objects.

use quench_runtime::{KeptBytes, NativeJsProgram, ResidualProgram, Runtime, SystemHost, Value};
use std::{
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

quench_runtime::export_native_js_operations!(SystemHost, semantic_exports);

static ENTRIES: AtomicUsize = AtomicUsize::new(0);
static BOOLEAN_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static STORE_BODY_ENTRIES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

#[unsafe(no_mangle)]
pub extern "C" fn native_image_entered(image: u64) {
    ENTRIES.fetch_or(1 << image, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn native_boolean_entered(image: u64) {
    BOOLEAN_ENTRIES.fetch_or(1 << image, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn native_store_body_entered(image: u64) {
    STORE_BODY_ENTRIES[image as usize].fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" {
    static fixture_js_entries: u8;
    static fixture_ts_entries: u8;
}

fn run() -> Result<(), String> {
    let images: [(&'static [u8], *const u8); 2] = [
        (
            include_bytes!(env!("QUENCH_NATIVE_JS_METADATA")),
            std::ptr::addr_of!(fixture_js_entries),
        ),
        (
            include_bytes!(env!("QUENCH_NATIVE_TS_METADATA")),
            std::ptr::addr_of!(fixture_ts_entries),
        ),
    ];
    for (bytes, entries) in images {
        let bytes: KeptBytes = Rc::new(bytes);
        let program = ResidualProgram::from_binary_bytes(bytes)?;
        // SAFETY: metadata and entry tables were emitted together; their code
        // and data are statically linked into this executable for SystemHost.
        let native = unsafe { NativeJsProgram::from_linked_image(program, entries) }?;
        let mut runtime = Runtime::new(SystemHost);
        match runtime.execute_native(&native) {
            Err(error) if error.thrown_value() == Some(Value::number(42.0)) => {}
            result => return Err(format!("incorrect native result: {result:?}")),
        }
    }
    let expected = (1 << images.len()) - 1;
    if ENTRIES.load(Ordering::Relaxed) != expected {
        return Err("program roots did not enter the linked objects".into());
    }
    if BOOLEAN_ENTRIES.load(Ordering::Relaxed) != expected {
        return Err("guarded boolean calls did not enter both linked objects".into());
    }
    // One cold entry, two non-number captures, a setter, a proxy, 64 stores
    // after descriptor replacement makes a dictionary, and the frozen error.
    const COLD_AND_FALLBACK_STORE_ENTRIES: usize = 70;
    for count in &STORE_BODY_ENTRIES {
        let count = count.load(Ordering::Relaxed);
        if count == 0 || count > COLD_AND_FALLBACK_STORE_ENTRIES {
            return Err(format!("captured stores did not elide warmed callee entries: {count}"));
        }
    }
    println!("native JS and TS images passed");
    Ok(())
}

fn main() {
    let result = std::thread::Builder::new()
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run)
        .expect("native worker")
        .join()
        .expect("native worker panic");
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
