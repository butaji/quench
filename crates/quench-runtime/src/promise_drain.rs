thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
    static PROMISE_TRIGGER: RefCell<Option<Rc<PromiseData>>> =
        const { RefCell::new(None) };
    static THEN_RESULTS: RefCell<HashMap<usize, VecDeque<Rc<PromiseData>>>> =
        RefCell::new(HashMap::new());
    static JOB_QUEUE: RefCell<VecDeque<Rc<dyn Fn()>>> = const { RefCell::new(VecDeque::new()) };
}

pub fn enqueue_job(job: Rc<dyn Fn()>) {
    JOB_QUEUE.with(|queue| queue.borrow_mut().push_back(job));
}

pub fn clear_jobs() {
    JOB_QUEUE.with(|queue| queue.borrow_mut().clear());
}

/// Drains all queued microtasks.
pub fn drain_microtasks() {
    while let Some(promise) = MICROTASK_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        process_promise(&promise);
    }
    if let Some(job) = JOB_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        job();
    }
}

/// Run exactly one queued promise reaction or host job.
pub(crate) fn drain_one_microtask() -> bool {
    if let Some(promise) = MICROTASK_QUEUE.with(|queue| queue.borrow_mut().pop_front()) {
        process_promise(&promise);
        return true;
    }
    if let Some(job) = JOB_QUEUE.with(|queue| queue.borrow_mut().pop_front()) {
        job();
        return true;
    }
    false
}

/// Repeatedly drain microtasks until none remain, so promise `.then`/`.catch`
/// reactions and synchronously-settling chains run to completion.
pub fn drain_microtasks_all() {
    loop {
        let promises = MICROTASK_QUEUE.with(|queue| queue.borrow().len());
        let jobs = JOB_QUEUE.with(|queue| queue.borrow().len());
        if promises == 0 && jobs == 0 {
            break;
        }
        drain_microtasks();
    }
}
