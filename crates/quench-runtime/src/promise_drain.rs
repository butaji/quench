thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
    static THEN_RESULTS: RefCell<HashMap<usize, VecDeque<Rc<PromiseData>>>> =
        RefCell::new(HashMap::new());
}

/// Drains all queued microtasks.
pub fn drain_microtasks() {
    while let Some(promise) = MICROTASK_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        process_promise(&promise);
    }
}

/// Repeatedly drain microtasks until none remain, so promise `.then`/`.catch`
/// reactions and synchronously-settling chains run to completion.
pub fn drain_microtasks_all() {
    loop {
        let pending = MICROTASK_QUEUE.with(|queue| queue.borrow().len());
        if pending == 0 {
            break;
        }
        drain_microtasks();
    }
}
