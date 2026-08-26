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

fn execute_linked_handler(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    packet: &crate::value::Value,
    plan: HandlerTaskPlan,
) -> Option<crate::value::Value> {
    let task_v1 = current.word(current.task_v1);
    let task_v2 = current.word(current.task_v2?);
    let mut v1 = task_v1.load();
    let mut v2 = task_v2.load();
    let incoming = incoming_packet(
        &current.function,
        Some(packet),
        plan,
        task_v1,
        task_v2,
    )?;
    if let Some(incoming) = &incoming {
        let appended = predicted_append(&incoming.packet, &incoming.queue)?;
        if std::ptr::eq(incoming.target, task_v1) {
            v1 = appended;
        } else {
            v2 = appended;
        }
    }
    let route = linked_handler_route(current, table, scheduler, task_v1, task_v2, v1, v2, plan)?;
    if let Some(incoming) = incoming {
        apply_linked_incoming(incoming)?;
    }
    crate::execution_trace::kernel("linked_handler_task", false);
    Some(route.execute())
}

fn apply_linked_incoming(incoming: IncomingPacket<'_>) -> Option<()> {
    unsafe { &*incoming.packet_link }.store(crate::value::Value::Null);
    if let Some(tail_link) = incoming.tail_link {
        unsafe { &*tail_link }.store(incoming.packet.clone());
    }
    incoming
        .target
        .store(predicted_append(&incoming.packet, &incoming.queue)?);
    Some(())
}

enum LinkedHandlerRoute<'a> {
    Suspend(crate::value::Value),
    Work {
        task_v2: &'a crate::register_file::SlotWord,
        next_v2: crate::value::Value,
        packet_a1: *const crate::register_file::SlotWord,
        work_a1: *const crate::register_file::SlotWord,
        value: f64,
        count: f64,
        queue: LinkedQueueTransition,
    },
    Complete {
        task_v1: &'a crate::register_file::SlotWord,
        next_v1: crate::value::Value,
        queue: LinkedQueueTransition,
    },
}

impl LinkedHandlerRoute<'_> {
    fn execute(self) -> crate::value::Value {
        match self {
            Self::Suspend(result) => result,
            Self::Work { task_v2, next_v2, packet_a1, work_a1, value, count, queue } => {
                task_v2.store(next_v2);
                unsafe { &*packet_a1 }.store(crate::value::Value::Number(value));
                unsafe { &*work_a1 }.store(crate::value::Value::Number(count + 1.0));
                queue.execute()
            }
            Self::Complete { task_v1, next_v1, queue } => {
                task_v1.store(next_v1);
                queue.execute()
            }
        }
    }
}

fn linked_handler_route<'a>(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    task_v1: &'a crate::register_file::SlotWord,
    task_v2: &'a crate::register_file::SlotWord,
    v1: crate::value::Value,
    v2: crate::value::Value,
    plan: HandlerTaskPlan,
) -> Option<LinkedHandlerRoute<'a>> {
    if v1.is_nullish() {
        return linked_suspend(current).map(LinkedHandlerRoute::Suspend);
    }
    let crate::value::Value::Object(work) = v1 else { return None };
    if work.has_replacement() { return None; }
    let count = writable_own_word(&work, "a1")?.number()?;
    let data_size = current.function.captures.get_number(plan.data_size_slot)?;
    if count < data_size {
        return linked_handler_work(current, table, scheduler, task_v2, v2, work, count);
    }
    let next_v1 = crate::vm::proven_own_word(&work, "link")?.load();
    let packet = crate::value::Value::Object(work);
    Some(LinkedHandlerRoute::Complete {
        task_v1,
        next_v1,
        queue: LinkedQueueTransition::new(current, table, scheduler, packet)?,
    })
}

fn linked_handler_work<'a>(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    task_v2: &'a crate::register_file::SlotWord,
    v2: crate::value::Value,
    work: std::rc::Rc<crate::value::ObjectData>,
    count: f64,
) -> Option<LinkedHandlerRoute<'a>> {
    if v2.is_nullish() {
        return linked_suspend(current).map(LinkedHandlerRoute::Suspend);
    }
    let crate::value::Value::Object(packet) = v2 else { return None };
    if packet.has_replacement() || count < 0.0 || count.fract() != 0.0 { return None; }
    let next_v2 = crate::vm::proven_own_word(&packet, "link")?.load();
    let crate::value::Value::Array(payload) = crate::vm::proven_own_word(&work, "a2")?.load() else { return None };
    if !crate::locals::array_word_is_current(&payload) || !payload.is_packed_ordinary() { return None; }
    let value = payload.dense_number_at(count as usize)?;
    let packet_value = crate::value::Value::Object(packet.clone());
    Some(LinkedHandlerRoute::Work {
        task_v2,
        next_v2,
        packet_a1: std::ptr::from_ref(writable_own_word(&packet, "a1")?),
        work_a1: std::ptr::from_ref(writable_own_word(&work, "a1")?),
        value,
        count,
        queue: LinkedQueueTransition::new(current, table, scheduler, packet_value)?,
    })
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
