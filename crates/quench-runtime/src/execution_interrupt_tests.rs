use crate::test_host::TestHost;
use crate::{Engine, NativeContext, RootId, RootedError, Runtime};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
/// What the interrupted scripts' `started` native signals, and whether the host was dropped.
struct InterruptProbe {
    started: mpsc::Sender<()>,
    dropped: Arc<AtomicBool>,
}
impl Drop for InterruptProbe {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release)
    }
}
fn interrupt_host(started: mpsc::Sender<()>, dropped: Arc<AtomicBool>) -> TestHost {
    TestHost::default()
        .with_natives(crate::host_functions![global "started" (0) => signal_started])
        .with_state(std::rc::Rc::new(InterruptProbe { started, dropped }))
}
fn signal_started(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.host_mut().state::<InterruptProbe>().started.send(()).unwrap();
    Ok(ctx.undefined())
}
#[test]
fn termination_unwinds_both_javascript_modes_without_guest_handlers() {
    for specialized in [false, true] {
        let (tx, rx) = mpsc::channel();
        let (ready, wait) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let d = dropped.clone();
        let join=thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move||{
 let mut runtime=Runtime::new(interrupt_host(ready,d));tx.send(runtime.execution_interrupt_handle()).unwrap();
 let source="var caught=false,finalized=false;function inner(){started();let i=0;try{for(;;){i++}}catch(e){caught=true}finally{finalized=true}}function outer(){try{inner()}catch(e){caught=true}finally{finalized=true}}outer();";
 let program=if specialized{Engine::specialize(source,"interrupt.js")}else{Engine::specialize_unspecialized(source,"interrupt.js")}.unwrap();
 let error=runtime.execute(&program).unwrap_err();assert!(error.is_execution_terminated());assert!(error.thrown_value().is_none());
 let global=runtime.global_root().unwrap();for name in["caught","finalized"]{let k=runtime.string_rooted(name);let v=runtime.get_property_rooted(global,k).unwrap();assert_eq!(runtime.rooted_value(v),Some(crate::Value::FALSE));runtime.release_root(v);runtime.release_root(k);}runtime.release_root(global);
 assert_eq!(runtime.heap_memory_composition().cell_size_bytes,(2+crate::value_vec::INLINE_PROPERTY_COUNT)*std::mem::size_of::<crate::Value>());
 }).unwrap();
        let handle = rx.recv().unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        thread::sleep(Duration::from_millis(10));
        handle.request_termination();
        join.join().unwrap();
        assert!(dropped.load(Ordering::Acquire));
        println!("uncatchable-js-termination-pass specialized={specialized}");
    }
}

#[test]
fn owned_termination_handle_stops_before_execution_and_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<crate::ExecutionInterrupt>();
    let (tx, rx) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let handle = crate::ExecutionInterrupt::default();
    handle.request_termination();
    let mut runtime = Runtime::new_with_execution_interrupt(
        interrupt_host(tx, dropped.clone()),
        handle,
    );
    let program = Engine::specialize("started();", "preterminated.js").unwrap();
    assert!(
        runtime
            .execute(&program)
            .unwrap_err()
            .is_execution_terminated()
    );
    assert!(rx.try_recv().is_err());
    drop(runtime);
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn termination_wakers_are_weak_and_late_registration_wakes() {
    use std::sync::atomic::AtomicUsize;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    let handle = crate::ExecutionInterrupt::default();
    handle.register_termination_waker(&wake);
    handle.request_termination();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    handle.request_termination();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    handle.register_termination_waker(&wake);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let dormant = crate::ExecutionInterrupt::default();
    dormant.register_termination_waker(&wake);
    assert_eq!(Arc::strong_count(&wake), 1);
    drop(wake);
    dormant.request_termination();
}

/// A loop the loop tier compiled stops at its next backward jump, through the poll page
/// where the host has one: the request ends the execution, and no guest handler runs.
#[test]
fn termination_stops_a_natively_compiled_loop() {
    let (tx, rx) = mpsc::channel();
    let (ready, wait) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let d = dropped.clone();
    let join = thread::Builder::new()
        .stack_size(crate::WORKER_STACK_SIZE)
        .spawn(move || {
            let mut runtime = Runtime::new(interrupt_host(ready, d));
            runtime.set_execution_mode(crate::ExecutionMode::Compiled);
            tx.send(runtime.execution_interrupt_handle()).unwrap();
            // Once entered, the loop runs to its end in native code: only the poll stops it.
            let source = "var finalized=false;function ring(x,size,n){var i=0;for(var k=0;k<n;k++){x[i]+=1;i++;if(i>=size)i=0}}function spin(){started();try{ring(new Array(1024).fill(0),1024,1/0)}finally{finalized=true}}spin();";
            let program = Engine::specialize(source, "native-loop.js").unwrap();
            let error = runtime.execute(&program).unwrap_err();
            assert!(error.is_execution_terminated());
            let global = runtime.global_root().unwrap();
            let key = runtime.string_rooted("finalized");
            let value = runtime.get_property_rooted(global, key).unwrap();
            assert_eq!(runtime.rooted_value(value), Some(crate::Value::FALSE));
        })
        .unwrap();
    let handle = rx.recv().unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    // Long enough for the loop to tier up and run natively.
    thread::sleep(Duration::from_millis(200));
    handle.request_termination();
    join.join().unwrap();
}
