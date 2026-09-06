thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Rc<PromiseData>>> =
        const { RefCell::new(VecDeque::new()) };
    static PROMISE_TRIGGER: RefCell<Option<Rc<PromiseData>>> =
        const { RefCell::new(None) };
    static THEN_RESULTS: RefCell<HashMap<usize, VecDeque<crate::promise::ThenResult>>> =
        RefCell::new(HashMap::new());
    static JOB_QUEUE: RefCell<VecDeque<Rc<dyn Fn()>>> = const { RefCell::new(VecDeque::new()) };
    static UNHANDLED_REJECTIONS: RefCell<VecDeque<(Rc<PromiseData>, Value)>> =
        const { RefCell::new(VecDeque::new()) };
}

pub fn enqueue_job(job: Rc<dyn Fn()>) {
    JOB_QUEUE.with(|queue| queue.borrow_mut().push_back(job));
}

pub(crate) fn queue_unhandled_rejection(promise: Rc<PromiseData>, reason: Value) {
    UNHANDLED_REJECTIONS.with(|queue| queue.borrow_mut().push_back((promise, reason)));
}

pub(crate) fn remove_unhandled_rejection(promise: &Rc<PromiseData>) {
    UNHANDLED_REJECTIONS.with(|queue| {
        queue
            .borrow_mut()
            .retain(|(queued, _)| !Rc::ptr_eq(queued, promise));
    });
}

pub fn take_unhandled_rejections() -> Vec<(Rc<PromiseData>, Value)> {
    UNHANDLED_REJECTIONS.with(|queue| queue.borrow_mut().drain(..).collect())
}

pub fn has_pending_unhandled_rejections() -> bool {
    UNHANDLED_REJECTIONS.with(|queue| !queue.borrow().is_empty())
}

pub fn clear_jobs() {
    JOB_QUEUE.with(|queue| queue.borrow_mut().clear());
}

/// Whether a promise reaction or host job is waiting to run.
pub fn has_pending_jobs() -> bool {
    MICROTASK_QUEUE.with(|queue| !queue.borrow().is_empty())
        || JOB_QUEUE.with(|queue| !queue.borrow().is_empty())
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
