//! Host adapter for the native-image integration test; guest code is in objects.

use quench_runtime::{KeptBytes, NativeJsProgram, ResidualProgram, Runtime, SystemHost, Value};
use std::{
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

quench_runtime::export_native_js_operations!(SystemHost, semantic_exports);

static ENTRIES: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn native_image_entered(image: u64) {
    ENTRIES.fetch_or(1 << image, Ordering::Relaxed);
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
