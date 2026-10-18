use crate::{Engine, Host, HostFunction, NativeContext, RootId, RootedError, Runtime};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
struct TestHost {
    started: mpsc::Sender<()>,
    dropped: Arc<AtomicBool>,
}
impl Drop for TestHost {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release)
    }
}
impl Host for TestHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.
    }
    fn functions(&self) -> &[HostFunction<Self>] {
        &[HostFunction {
            name: "started",
            length: 0,
            global: Some("started"),
            call: started,
        }]
    }
}
fn started(
    ctx: &mut NativeContext<'_, TestHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.host_mut().started.send(()).unwrap();
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
 let mut runtime=Runtime::new(TestHost{started:ready,dropped:d});tx.send(runtime.execution_interrupt_handle()).unwrap();
 let source="var caught=false,finalized=false;function inner(){started();let i=0;try{for(;;){i++}}catch(e){caught=true}finally{finalized=true}}function outer(){try{inner()}catch(e){caught=true}finally{finalized=true}}outer();";
 let program=if specialized{Engine::specialize(source,"interrupt.js")}else{Engine::specialize_unspecialized(source,"interrupt.js")}.unwrap();
 let error=runtime.execute(&program).unwrap_err();assert!(error.is_execution_terminated());assert!(error.thrown_value().is_none());
 let global=runtime.global_root().unwrap();for name in["caught","finalized"]{let k=runtime.string_rooted(name);let v=runtime.get_property_rooted(global,k).unwrap();assert_eq!(runtime.rooted_value(v),Some(crate::Value::FALSE));runtime.release_root(v);runtime.release_root(k);}runtime.release_root(global);
 assert_eq!(runtime.heap_memory_composition().cell_size_bytes,32);
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
        TestHost {
            started: tx,
            dropped: dropped.clone(),
        },
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
