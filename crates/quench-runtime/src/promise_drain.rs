thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
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
    loop {
        let promise = MICROTASK_QUEUE.with(|q| q.borrow_mut().pop_front());
        let job = JOB_QUEUE.with(|q| q.borrow_mut().pop_front());
        match (promise, job) {
            (None, None) => break,
            (Some(promise), job) => {
                process_promise(&promise);
                if let Some(job) = job {
                    job();
                }
            }
            (None, Some(job)) => job(),
        }
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
