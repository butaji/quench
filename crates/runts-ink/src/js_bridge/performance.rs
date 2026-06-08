//! Performance API implementation for rquickjs bridge
//!
//! Provides performance.now(), performance.mark(), performance.measure()
//! and related timing functions.

use rquickjs::{Ctx, Function, Object, Result as JsResult};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic clock start time (initialized on first use)
static START_TIME: AtomicU64 = AtomicU64::new(0);

fn get_start_time() -> u64 {
    let current = START_TIME.load(Ordering::SeqCst);
    if current == 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let base = 1_700_000_000_000_000_000u64;
        let start = if now > base { now - base } else { 0 };
        START_TIME.store(start, Ordering::SeqCst);
        start
    } else {
        current
    }
}

fn now_ms() -> f64 {
    let start = get_start_time();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let elapsed = if now > start { now - start } else { 0 };
    elapsed as f64 / 1_000_000.0
}

pub fn install(ctx: &Ctx<'_>) -> JsResult<()> {
    let globals = ctx.globals();
    let perf = Object::new(ctx.clone())?;
    
    // performance.now() - returns time since page load in milliseconds
    perf.set("now", Function::new(ctx.clone(), |_ctx: Ctx| -> f64 { now_ms() }))?;
    
    // performance.mark() - no-op in TUI context
    perf.set("mark", Function::new(ctx.clone(), |_ctx: Ctx, _name: String| -> JsResult<()> { Ok(()) }))?;
    
    // performance.measure() - no-op in TUI context
    perf.set("measure", Function::new(ctx.clone(), |_ctx: Ctx, _name: String, _start: Option<String>, _end: Option<String>| -> JsResult<()> { Ok(()) }))?;
    
    // performance.clearMarks() - no-op
    perf.set("clearMarks", Function::new(ctx.clone(), |_ctx: Ctx, _name: Option<String>| -> JsResult<()> {
        let _ = _name;
        Ok(())
    }))?;
    
    // performance.clearMeasures() - no-op
    perf.set("clearMeasures", Function::new(ctx.clone(), |_ctx: Ctx, _name: Option<String>| -> JsResult<()> {
        let _ = _name;
        Ok(())
    }))?;
    
    // performance.getEntries() - no-op in TUI context
    perf.set("getEntries", Function::new(ctx.clone(), |_ctx: Ctx| -> JsResult<()> { Ok(()) }))?;
    
    globals.set("performance", perf)
}
