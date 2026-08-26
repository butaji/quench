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
) -> Option<DirectTaskStep> {
    let v1 = current.word(current.task_v1);
    if !packet.is_nullish() {
        let transition = LinkedHoldTransition::new(current, table, scheduler)?;
        v1.store(packet.clone());
        crate::execution_trace::kernel("linked_device_hold", false);
        return Some(DirectTaskStep::new(transition.execute()));
    }
    let queued = v1.load();
    if queued.is_nullish() {
        let result = linked_suspend(current)?;
        crate::execution_trace::kernel("linked_device_suspend", false);
        return Some(DirectTaskStep::new(Some(result)));
    }
    let transition = LinkedQueueTransition::new(current, table, scheduler, queued)?;
    v1.store(crate::value::Value::Null);
    crate::execution_trace::kernel("linked_device_queue", false);
    Some(DirectTaskStep::new(Some(transition.execute())))
}

fn execute_linked_idle(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    plan: IdleTaskPlan,
) -> Option<DirectTaskStep> {
    let count = current.word(current.task_count?);
    let next_count = count.number()? - 1.0;
    if next_count == 0.0 {
        let transition = LinkedHoldTransition::new(current, table, scheduler)?;
        count.store_number(next_count);
        crate::execution_trace::kernel("linked_idle_hold", false);
        return Some(DirectTaskStep::new(transition.execute()));
    }
    let v1 = current.word(current.task_v1);
    let value = crate::vm::vm_arithmetic::numeric_to_int32(v1.number()?);
    let even = value & 1 == 0;
    let next_v1 = if even {
        value >> 1
    } else {
        (value >> 1) ^ 0xD008
    };
    let slot = if even {
        plan.device_a_slot
    } else {
        plan.device_b_slot
    };
    let id = current.function.captures.get_number(slot)?;
    let transition =
        LinkedReleaseTransition::new(current, table.for_id(exact_linked_task_id(id)?)?)?;
    count.store_number(next_count);
    v1.store_number(f64::from(next_v1));
    crate::execution_trace::kernel("linked_idle_release", false);
    Some(DirectTaskStep::new(Some(transition.execute())))
}

fn execute_linked_worker(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    packet: &crate::value::Value,
    plan: WorkerTaskPlan,
) -> Option<DirectTaskStep> {
    if packet.is_nullish() {
        let next = linked_suspend(current)?;
        crate::execution_trace::kernel("linked_worker_suspend", false);
        return Some(DirectTaskStep::new(Some(next)));
    }
    let crate::value::Value::Object(packet_object) = packet else {
        return None;
    };
    if packet_object.has_replacement() {
        return None;
    }
    let packet_words = table.packet_words(packet_object)?;
    let v1 = current.word(current.task_v1);
    let v2 = current.word(current.task_v2?);
    let handler_a = current.function.captures.get_number(plan.handler_a_slot)?;
    let handler_b = current.function.captures.get_number(plan.handler_b_slot)?;
    let next_id = if v1.number()? == handler_a {
        handler_b
    } else {
        handler_a
    };
    let count = exact_worker_count(current.function.captures.get_number(plan.data_size_slot)?)?;
    let (values, next_v2) = worker_values(v2.number()?, count);
    let payload = linked_worker_payload(packet_words.a2, count)?;
    let target_id = exact_linked_task_id(next_id)?;
    let queue =
        LinkedQueueTransition::new_for_id(current, table, scheduler, packet.clone(), target_id)?;
    apply_linked_worker_payload(&payload, &values[..count])?;
    v1.store_number(next_id);
    v2.store_number(next_v2);
    packet_words.id.store_number(next_id);
    packet_words.a1.store_number(0.0);
    crate::execution_trace::kernel("linked_worker_task", false);
    Some(DirectTaskStep::new(Some(queue.execute())))
}

fn apply_linked_worker_payload(payload: &crate::value::ArrayData, values: &[f64]) -> Option<()> {
    if payload.is_holey() {
        for (index, value) in values.iter().copied().enumerate() {
            if !payload.append_preallocated_f64(index, value) {
                return None;
            }
        }
    } else {
        payload.numeric_kernel_words_mut()?[..values.len()].copy_from_slice(values);
    }
    Some(())
}

fn linked_worker_payload(
    payload: &crate::register_file::SlotWord,
    count: usize,
) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    let crate::value::Value::Array(payload) = payload.load() else {
        return None;
    };
    (crate::locals::array_word_is_current(&payload)
        && payload.header_length() == count
        && ((payload.is_packed_ordinary() && payload.is_numeric_packed())
            || (payload.is_holey() && payload.physical_len() == 0)))
        .then_some(payload)
}

fn execute_linked_handler(
    current: &DirectTaskRunner,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
    packet: &crate::value::Value,
    plan: HandlerTaskPlan,
) -> Option<DirectTaskStep> {
    let task_v1 = current.word(current.task_v1);
    let task_v2 = current.word(current.task_v2?);
    let mut v1 = task_v1.load();
    let mut v2 = task_v2.load();
    let incoming =
        linked_incoming_packet(&current.function, packet, plan, task_v1, task_v2, table)?;
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
    Some(DirectTaskStep::new(Some(route.execute())))
}

fn linked_incoming_packet<'a>(
    function: &crate::value::FunctionValue,
    packet: &crate::value::Value,
    plan: HandlerTaskPlan,
    task_v1: &'a crate::register_file::SlotWord,
    task_v2: &'a crate::register_file::SlotWord,
    table: &DirectTaskTable,
) -> Option<Option<IncomingPacket<'a>>> {
    if packet.is_nullish() {
        return Some(None);
    }
    let crate::value::Value::Object(object) = packet else {
        return None;
    };
    let words = table.packet_words(object)?;
    let kind = words.kind.number()?;
    let work_kind = function.captures.get_number(plan.work_kind_slot)?;
    let target = if kind == work_kind { task_v1 } else { task_v2 };
    let queue = target.load();
    if !queue.is_nullish() && !matches!(queue, crate::value::Value::Object(_)) {
        return None;
    }
    let tail_link = match &queue {
        crate::value::Value::Object(head) => {
            Some(table.packet_tail_link(std::rc::Rc::as_ptr(head), std::rc::Rc::as_ptr(object))?)
        }
        _ => None,
    };
    handler_packet_add_callee(function, object, kind == work_kind)?;
    Some(Some(IncomingPacket {
        target,
        packet: packet.clone(),
        queue,
        packet_link: std::ptr::from_ref(words.link),
        tail_link,
    }))
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
    Suspend(usize),
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
    fn execute(self) -> usize {
        match self {
            Self::Suspend(result) => result,
            Self::Work {
                task_v2,
                next_v2,
                packet_a1,
                work_a1,
                value,
                count,
                queue,
            } => {
                task_v2.store(next_v2);
                unsafe { &*packet_a1 }.store_number(value);
                unsafe { &*work_a1 }.store_number(count + 1.0);
                queue.execute()
            }
            Self::Complete {
                task_v1,
                next_v1,
                queue,
            } => {
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
    let crate::value::Value::Object(work) = v1 else {
        return None;
    };
    if work.has_replacement() {
        return None;
    }
    let work_words = table.packet_words(&work)?;
    let count = work_words.a1.number()?;
    let data_size = current.function.captures.get_number(plan.data_size_slot)?;
    if count < data_size {
        return linked_handler_work(current, table, scheduler, task_v2, v2, work, count);
    }
    let next_v1 = work_words.link.load();
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
    let crate::value::Value::Object(packet) = v2 else {
        return None;
    };
    if packet.has_replacement() || count < 0.0 || count.fract() != 0.0 {
        return None;
    }
    let packet_words = table.packet_words(&packet)?;
    let work_words = table.packet_words(&work)?;
    let next_v2 = packet_words.link.load();
    let crate::value::Value::Array(payload) = work_words.a2.load() else {
        return None;
    };
    if !crate::locals::array_word_is_current(&payload) || !payload.is_packed_ordinary() {
        return None;
    }
    let value = payload.dense_number_at(count as usize)?;
    let packet_value = crate::value::Value::Object(packet.clone());
    Some(LinkedHandlerRoute::Work {
        task_v2,
        next_v2,
        packet_a1: std::ptr::from_ref(packet_words.a1),
        work_a1: std::ptr::from_ref(work_words.a1),
        value,
        count,
        queue: LinkedQueueTransition::new(current, table, scheduler, packet_value)?,
    })
}

struct LinkedReleaseTransition {
    state: *const crate::register_file::SlotWord,
    next_state: f64,
    result: usize,
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

    fn execute(self) -> usize {
        // SAFETY: the retained target TCB owns the canonical state word.
        unsafe { &*self.state }.store_number(self.next_state);
        self.result
    }
}

struct LinkedHoldTransition {
    hold_count: *const crate::register_file::SlotWord,
    next_count: f64,
    state: *const crate::register_file::SlotWord,
    next_state: f64,
    next: Option<usize>,
}

impl LinkedHoldTransition {
    fn new(
        current: &DirectTaskRunner,
        table: &DirectTaskTable,
        scheduler: &LinkedSchedulerWords,
    ) -> Option<Self> {
        let hold_count = scheduler.word(scheduler.hold_count);
        let state = current.word(current.state);
        let current_state = exact_i32(state.number()?)?;
        let next = table.id_for_word(current.word(current.link))?;
        Some(Self {
            hold_count: scheduler.hold_count,
            next_count: hold_count.number()? + 1.0,
            state: current.state,
            next_state: f64::from(current_state | current.held_mask),
            next,
        })
    }

    fn execute(self) -> Option<usize> {
        // SAFETY: admission retains the scheduler and current TCB, and every
        // pointer names a canonical ordinary own word proved before mutation.
        unsafe { &*self.hold_count }.store_number(self.next_count);
        unsafe { &*self.state }.store_number(self.next_state);
        self.next
    }
}

fn linked_suspend(current: &DirectTaskRunner) -> Option<usize> {
    let state = current.word(current.state);
    let next = exact_i32(state.number()?)? | current.suspended;
    state.store_number(f64::from(next));
    Some(current.index)
}

struct LinkedQueueTransition {
    queue_count: *const crate::register_file::SlotWord,
    next_count: f64,
    packet_link: *const crate::register_file::SlotWord,
    packet_id: *const crate::register_file::SlotWord,
    current_id: f64,
    packet: crate::value::Value,
    target: CheckPriorityTransition<usize>,
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
        let id = exact_linked_task_id(table.packet_words(packet_object)?.id.number()?)?;
        Self::new_for_id(current, table, scheduler, packet, id)
    }

    fn new_for_id(
        current: &DirectTaskRunner,
        table: &DirectTaskTable,
        scheduler: &LinkedSchedulerWords,
        packet: crate::value::Value,
        id: usize,
    ) -> Option<Self> {
        let crate::value::Value::Object(packet_object) = &packet else {
            return None;
        };
        if packet_object.has_replacement() {
            return None;
        }
        let packet_words = table.packet_words(packet_object)?;
        let target = table.for_id(id)?;
        let queue_count = scheduler.word(scheduler.queue_count);
        Some(Self {
            queue_count: scheduler.queue_count,
            next_count: queue_count.number()? + 1.0,
            packet_link: std::ptr::from_ref(packet_words.link),
            packet_id: std::ptr::from_ref(packet_words.id),
            current_id: current.word(current.id).number()?,
            target: linked_priority_transition(
                current,
                target,
                table,
                packet_object,
                scheduler.runnable,
            )?,
            packet,
        })
    }

    fn execute(self) -> usize {
        let packet_link = unsafe { &*self.packet_link };
        unsafe { &*self.queue_count }.store_number(self.next_count);
        packet_link.store(crate::value::Value::Null);
        unsafe { &*self.packet_id }.store_number(self.current_id);
        self.target.apply(packet_link, self.packet)
    }
}

fn linked_priority_transition(
    current: &DirectTaskRunner,
    target: &DirectTaskRunner,
    table: &DirectTaskTable,
    packet: &crate::value::ObjectData,
    runnable: i32,
) -> Option<CheckPriorityTransition<usize>> {
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
    let tail_link = table.packet_tail_link(std::rc::Rc::as_ptr(head_object), packet as *const _)?;
    Some(CheckPriorityTransition::Append {
        queue: target.queue,
        tail_link,
        head,
        result: current.index,
    })
}

fn linked_priority_result(current: &DirectTaskRunner, target: &DirectTaskRunner) -> Option<usize> {
    let target_priority = target.word(target.priority).number()?;
    let current_priority = current.word(current.priority).number()?;
    Some(if target_priority > current_priority {
        target.index
    } else {
        current.index
    })
}
