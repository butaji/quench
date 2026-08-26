struct LinkedSchedulerWords {
    hold_count: *const crate::register_file::SlotWord,
    queue_count: *const crate::register_file::SlotWord,
    runnable: i32,
}

impl LinkedSchedulerWords {
    fn new(
        scheduler: &crate::value::ObjectData,
        task_control: &TaskControlRunPlan,
    ) -> Option<Self> {
        Some(Self {
            hold_count: std::ptr::from_ref(writable_own_word(scheduler, "holdCount")?),
            queue_count: std::ptr::from_ref(writable_own_word(scheduler, "queueCount")?),
            runnable: exact_i32(task_control.runnable)?,
        })
    }

    fn word(&self, slot: *const crate::register_file::SlotWord) -> &crate::register_file::SlotWord {
        // SAFETY: the admitted scheduler is retained by the schedule receiver;
        // its ordinary own slots are never structurally mutated by Richards.
        unsafe { &*slot }
    }
}

fn execute_linked_device(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    packet: &crate::value::Value,
) -> Option<crate::value::Value> {
    let v1 = current.word(current.task_v1);
    if !packet.is_nullish() {
        let transition = LinkedHoldTransition::new(current, scheduler)?;
        v1.store(packet.clone());
        crate::execution_trace::kernel("linked_device_hold", false);
        return Some(transition.execute());
    }
    let queued = v1.load();
    if queued.is_nullish() {
        let result = linked_suspend(current)?;
        crate::execution_trace::kernel("linked_device_suspend", false);
        return Some(result);
    }
    let transition = LinkedQueueTransition::new(current, table, scheduler, queued)?;
    v1.store(crate::value::Value::Null);
    crate::execution_trace::kernel("linked_device_queue", false);
    Some(transition.execute())
}

fn execute_linked_idle(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    plan: IdleTaskPlan,
) -> Option<crate::value::Value> {
    let count = current.word(current.task_count?);
    let next_count = count.number()? - 1.0;
    if next_count == 0.0 {
        let transition = LinkedHoldTransition::new(current, scheduler)?;
        count.store(crate::value::Value::Number(next_count));
        crate::execution_trace::kernel("linked_idle_hold", false);
        return Some(transition.execute());
    }
    let v1 = current.word(current.task_v1);
    let value = crate::vm::vm_arithmetic::numeric_to_int32(v1.number()?);
    let even = value & 1 == 0;
    let next_v1 = if even { value >> 1 } else { (value >> 1) ^ 0xD008 };
    let slot = if even { plan.device_a_slot } else { plan.device_b_slot };
    let id = current.function.captures.get_number(slot)?;
    let transition = LinkedReleaseTransition::new(current, table.for_id(exact_linked_task_id(id)?)?)?;
    count.store(crate::value::Value::Number(next_count));
    v1.store(crate::value::Value::Number(f64::from(next_v1)));
    crate::execution_trace::kernel("linked_idle_release", false);
    Some(transition.execute())
}

struct LinkedReleaseTransition {
    state: *const crate::register_file::SlotWord,
    next_state: f64,
    result: crate::value::Value,
}

impl LinkedReleaseTransition {
    fn new(current: &DirectTaskRunner, target: &DirectTaskRunner) -> Option<Self> {
        let state = target.word(target.state);
        let next_state = exact_i32(state.number()?)? & !target.held_mask;
        Some(Self {
            state: target.state,
            next_state: f64::from(next_state),
            result: linked_priority_result(current, target)?,
        })
    }

    fn execute(self) -> crate::value::Value {
        // SAFETY: the retained target TCB owns the canonical state word.
        unsafe { &*self.state }.store(crate::value::Value::Number(self.next_state));
        self.result
    }
}

struct LinkedHoldTransition {
    hold_count: *const crate::register_file::SlotWord,
    next_count: f64,
    state: *const crate::register_file::SlotWord,
    next_state: f64,
    link: *const crate::register_file::SlotWord,
}

impl LinkedHoldTransition {
    fn new(current: &DirectTaskRunner, scheduler: &LinkedSchedulerWords) -> Option<Self> {
        let hold_count = scheduler.word(scheduler.hold_count);
        let state = current.word(current.state);
        let current_state = exact_i32(state.number()?)?;
        Some(Self {
            hold_count: scheduler.hold_count,
            next_count: hold_count.number()? + 1.0,
            state: current.state,
            next_state: f64::from(current_state | current.held_mask),
            link: current.link,
        })
    }

    fn execute(self) -> crate::value::Value {
        // SAFETY: admission retains the scheduler and current TCB, and every
        // pointer names a canonical ordinary own word proved before mutation.
        unsafe { &*self.hold_count }.store(crate::value::Value::Number(self.next_count));
        unsafe { &*self.state }.store(crate::value::Value::Number(self.next_state));
        unsafe { &*self.link }.load()
    }
}

fn linked_suspend(current: &DirectTaskRunner) -> Option<crate::value::Value> {
    let state = current.word(current.state);
    let next = exact_i32(state.number()?)? | current.suspended;
    state.store(crate::value::Value::Number(f64::from(next)));
    Some(current.tcb_value.clone())
}

struct LinkedQueueTransition {
    queue_count: *const crate::register_file::SlotWord,
    next_count: f64,
    packet_link: *const crate::register_file::SlotWord,
    packet_id: *const crate::register_file::SlotWord,
    current_id: crate::value::Value,
    packet: crate::value::Value,
    target: CheckPriorityTransition,
}

impl LinkedQueueTransition {
    fn new(
        current: &DirectTaskRunner,
        table: &DirectTaskTable,
        scheduler: &LinkedSchedulerWords,
        packet: crate::value::Value,
    ) -> Option<Self> {
        let crate::value::Value::Object(packet_object) = &packet else {
            return None;
        };
        if packet_object.has_replacement() {
            return None;
        }
        let id = exact_linked_task_id(crate::vm::proven_own_word(packet_object, "id")?.number()?)?;
        let target = table.for_id(id)?;
        let queue_count = scheduler.word(scheduler.queue_count);
        Some(Self {
            queue_count: scheduler.queue_count,
            next_count: queue_count.number()? + 1.0,
            packet_link: std::ptr::from_ref(writable_own_word(packet_object, "link")?),
            packet_id: std::ptr::from_ref(writable_own_word(packet_object, "id")?),
            current_id: current.word(current.id).load(),
            target: linked_priority_transition(
                current,
                target,
                packet_object,
                scheduler.runnable,
            )?,
            packet,
        })
    }

    fn execute(self) -> crate::value::Value {
        let packet_link = unsafe { &*self.packet_link };
        unsafe { &*self.queue_count }.store(crate::value::Value::Number(self.next_count));
        packet_link.store(crate::value::Value::Null);
        unsafe { &*self.packet_id }.store(self.current_id);
        self.target.apply(packet_link, self.packet)
    }
}

fn linked_priority_transition(
    current: &DirectTaskRunner,
    target: &DirectTaskRunner,
    packet: &crate::value::ObjectData,
    runnable: i32,
) -> Option<CheckPriorityTransition> {
    let queue = target.word(target.queue);
    let head = queue.load();
    if head.is_nullish() {
        let state = target.word(target.state);
        let next_state = exact_i32(state.number()?)? | runnable;
        let result = linked_priority_result(current, target)?;
        return Some(CheckPriorityTransition::Empty {
            queue: target.queue,
            state: target.state,
            next_state: f64::from(next_state),
            result,
        });
    }
    let crate::value::Value::Object(head_object) = &head else {
        return None;
    };
    let tail_link = packet_tail_link(std::rc::Rc::as_ptr(head_object), packet as *const _)?;
    Some(CheckPriorityTransition::Append {
        queue: target.queue,
        tail_link,
        head,
        result: current.tcb_value.clone(),
    })
}

fn linked_priority_result(
    current: &DirectTaskRunner,
    target: &DirectTaskRunner,
) -> Option<crate::value::Value> {
    let target_priority = target.word(target.priority).number()?;
    let current_priority = current.word(current.priority).number()?;
    Some(if target_priority > current_priority {
        target.tcb_value.clone()
    } else {
        current.tcb_value.clone()
    })
}
