const HANDLER_TASK_FACT_SLOTS: usize = 64;
#[derive(Clone, Copy)]
struct HandlerTaskPlan {
    work_kind_slot: u16,
    data_size_slot: u16,
}
struct HandlerTaskFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    plan: Option<HandlerTaskPlan>,
}
thread_local! {
    static HANDLER_TASK_FACTS: std::cell::RefCell<Vec<Option<HandlerTaskFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
#[inline]
fn is_handler_task_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1
        && function.code.capture_slots().len() == 6
        && function.code.code().is_some_and(|code| code.len() == 17)
}
fn execute_handler_task(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = handler_task_fact(function) else {
        return Ok(None);
    };
    let crate::value::Value::Object(task) = receiver else {
        return Ok(None);
    };
    if task.has_replacement() {
        return Ok(None);
    }
    let Some(state) = HandlerState::new(function, task, arguments, plan) else {
        return Ok(None);
    };
    let result = state.execute()?;
    crate::execution_trace::kernel("handler_task_run_word_slots", false);
    Ok(Some(result))
}

struct HandlerState<'a> {
    incoming: Option<IncomingPacket<'a>>,
    route: HandlerRoute<'a>,
}

struct IncomingPacket<'a> {
    target: &'a crate::register_file::SlotWord,
    packet: crate::value::Value,
    queue: crate::value::Value,
    packet_link: *const crate::register_file::SlotWord,
    tail_link: Option<*const crate::register_file::SlotWord>,
}

enum HandlerRoute<'a> {
    Suspend {
        transition: SchedulerSuspendWordTransition,
    },
    Work {
        task_v2: &'a crate::register_file::SlotWord,
        next_v2: crate::value::Value,
        packet_a1: *const crate::register_file::SlotWord,
        work_a1: *const crate::register_file::SlotWord,
        value: f64,
        count: f64,
        scheduler: crate::value::Value,
        callee: crate::value::Value,
        packet: crate::value::Value,
        _work: crate::value::Value,
    },
    Complete {
        task_v1: &'a crate::register_file::SlotWord,
        next_v1: crate::value::Value,
        scheduler: crate::value::Value,
        callee: crate::value::Value,
        packet: crate::value::Value,
    },
}

impl<'a> HandlerState<'a> {
    fn new(
        function: &crate::value::FunctionValue,
        task: &'a crate::value::ObjectData,
        arguments: &[crate::value::Value],
        plan: HandlerTaskPlan,
    ) -> Option<Self> {
        let task_v1 = writable_own_word(task, "v1")?;
        let task_v2 = writable_own_word(task, "v2")?;
        let mut v1 = task_v1.load();
        let mut v2 = task_v2.load();
        let incoming = incoming_packet(function, arguments.first(), plan, task_v1, task_v2)?;
        if let Some(incoming) = &incoming {
            if std::ptr::eq(incoming.target, task_v1) {
                v1 = predicted_append(&incoming.packet, &incoming.queue)?;
            } else {
                v2 = predicted_append(&incoming.packet, &incoming.queue)?;
            }
        }
        let route = handler_route(function, task, task_v1, task_v2, v1, v2, plan)?;
        Some(Self { incoming, route })
    }

    fn execute(self) -> Result<crate::value::Value, crate::execute::VmError> {
        if let Some(incoming) = self.incoming {
            unsafe { &*incoming.packet_link }.store(crate::value::Value::Null);
            if let Some(tail_link) = incoming.tail_link {
                unsafe { &*tail_link }.store(incoming.packet.clone());
            }
            incoming.target.store(
                predicted_append(&incoming.packet, &incoming.queue)
                    .ok_or(crate::execute::VmError::MissingReturn)?,
            );
        }
        self.route.execute()
    }
}

impl HandlerRoute<'_> {
    fn execute(self) -> Result<crate::value::Value, crate::execute::VmError> {
        match self {
            Self::Suspend { transition } => {
                let result = transition.execute();
                crate::execution_trace::kernel("handler_suspend_word_transition", false);
                Ok(result)
            }
            Self::Work {
                task_v2,
                next_v2,
                packet_a1,
                work_a1,
                value,
                count,
                scheduler,
                callee,
                packet,
                _work,
            } => {
                task_v2.store(next_v2);
                unsafe { &*packet_a1 }.store(crate::value::Value::Number(value));
                unsafe { &*work_a1 }.store(crate::value::Value::Number(count + 1.0));
                crate::functions::execute_target(&callee, &scheduler, &[packet])
            }
            Self::Complete {
                task_v1,
                next_v1,
                scheduler,
                callee,
                packet,
            } => {
                task_v1.store(next_v1);
                crate::functions::execute_target(&callee, &scheduler, &[packet])
            }
        }
    }
}

fn incoming_packet<'a>(
    function: &crate::value::FunctionValue,
    packet: Option<&crate::value::Value>,
    plan: HandlerTaskPlan,
    task_v1: &'a crate::register_file::SlotWord,
    task_v2: &'a crate::register_file::SlotWord,
) -> Option<Option<IncomingPacket<'a>>> {
    let packet = packet.cloned().unwrap_or(crate::value::Value::Undefined);
    if packet.is_nullish() {
        return Some(None);
    }
    let crate::value::Value::Object(object) = &packet else {
        return None;
    };
    if object.has_replacement() {
        return None;
    }
    let kind = crate::vm::proven_own_word(object, "kind")?.number()?;
    let work_kind = function.captures.get_number(plan.work_kind_slot)?;
    let target = if kind == work_kind { task_v1 } else { task_v2 };
    let queue = target.load();
    if !queue.is_nullish() && !matches!(queue, crate::value::Value::Object(_)) {
        return None;
    }
    let (packet_link, tail_link) = handler_packet_add_preflight(object, &queue)?;
    handler_packet_add_callee(function, object, kind == work_kind)?;
    Some(Some(IncomingPacket {
        target,
        packet,
        queue,
        packet_link,
        tail_link,
    }))
}

fn handler_packet_add_preflight(
    packet: &crate::value::ObjectData,
    queue: &crate::value::Value,
) -> Option<(
    *const crate::register_file::SlotWord,
    Option<*const crate::register_file::SlotWord>,
)> {
    let packet_link = std::ptr::from_ref(writable_own_word(packet, "link")?);
    let crate::value::Value::Object(head) = queue else {
        return queue.is_nullish().then_some((packet_link, None));
    };
    let tail_link = packet_tail_link(std::rc::Rc::as_ptr(head), packet as *const _)?;
    Some((packet_link, Some(tail_link)))
}

fn predicted_append(
    packet: &crate::value::Value,
    queue: &crate::value::Value,
) -> Option<crate::value::Value> {
    if queue.is_nullish() {
        Some(packet.clone())
    } else if matches!(queue, crate::value::Value::Object(_)) {
        Some(queue.clone())
    } else {
        None
    }
}

fn handler_route<'a>(
    function: &crate::value::FunctionValue,
    task: &'a crate::value::ObjectData,
    task_v1: &'a crate::register_file::SlotWord,
    task_v2: &'a crate::register_file::SlotWord,
    v1: crate::value::Value,
    v2: crate::value::Value,
    plan: HandlerTaskPlan,
) -> Option<HandlerRoute<'a>> {
    let scheduler = task_scheduler(task)?;
    if v1.is_nullish() {
        return handler_suspend_transition(function, &scheduler);
    }
    let crate::value::Value::Object(work) = v1 else {
        return None;
    };
    if work.has_replacement() {
        return None;
    }
    let count = writable_own_word(&work, "a1")?.number()?;
    let data_size = function.captures.get_number(plan.data_size_slot)?;
    if count < data_size {
        handler_work_route(function, task_v2, v2, work, count, scheduler)
    } else {
        let next_v1 = crate::vm::proven_own_word(&work, "link")?.load();
        Some(HandlerRoute::Complete {
            task_v1,
            next_v1,
            callee: handler_queue_callee(function, &scheduler, false)?,
            scheduler,
            packet: crate::value::Value::Object(work),
        })
    }
}

fn handler_work_route<'a>(
    function: &crate::value::FunctionValue,
    task_v2: &'a crate::register_file::SlotWord,
    v2: crate::value::Value,
    work: std::rc::Rc<crate::value::ObjectData>,
    count: f64,
    scheduler: crate::value::Value,
) -> Option<HandlerRoute<'a>> {
    if v2.is_nullish() {
        return handler_suspend_transition(function, &scheduler);
    }
    let crate::value::Value::Object(packet) = v2 else {
        return None;
    };
    if packet.has_replacement() || count < 0.0 || count.fract() != 0.0 {
        return None;
    }
    let next_v2 = crate::vm::proven_own_word(&packet, "link")?.load();
    let payload = crate::vm::proven_own_word(&work, "a2")?.load();
    let crate::value::Value::Array(payload) = payload else {
        return None;
    };
    if !crate::locals::array_word_is_current(&payload) || !payload.is_packed_ordinary() {
        return None;
    }
    let value = payload.dense_number_at(count as usize)?;
    Some(HandlerRoute::Work {
        task_v2,
        next_v2,
        packet_a1: std::ptr::from_ref(writable_own_word(&packet, "a1")?),
        work_a1: std::ptr::from_ref(writable_own_word(&work, "a1")?),
        value,
        count,
        callee: handler_queue_callee(function, &scheduler, true)?,
        scheduler,
        packet: crate::value::Value::Object(packet),
        _work: crate::value::Value::Object(work),
    })
}

fn handler_packet_add_callee(
    function: &crate::value::FunctionValue,
    packet: &crate::value::ObjectData,
    work: bool,
) -> Option<crate::value::Value> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(4)? else {
        return None;
    };
    let code = then_ops.code()?;
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(5)?
    else {
        return None;
    };
    let code = if work {
        then_ops.code()?
    } else {
        else_ops.code()?
    };
    let value = crate::vm::get_named_cached_object(packet, &code.metadata_at(4)?.named_cache)?;
    let crate::value::Value::Function(callee) = &value else {
        return None;
    };
    packet_add_fact(callee).then_some(value)
}

fn handler_suspend_callee(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
) -> Option<crate::value::Value> {
    cached_shape_method(scheduler, function.code.code()?, 13)
}

fn handler_suspend_transition<'a>(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
) -> Option<HandlerRoute<'a>> {
    let callee = handler_suspend_callee(function, scheduler)?;
    Some(HandlerRoute::Suspend {
        transition: scheduler_suspend_word_transition(&callee, scheduler)?,
    })
}

fn handler_queue_callee(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
    work: bool,
) -> Option<crate::value::Value> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(10)? else {
        return None;
    };
    let code = then_ops.code()?;
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(8)?
    else {
        return None;
    };
    let code = if work {
        let code = then_ops.code()?;
        let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(5)? else {
            return None;
        };
        then_ops.code()?
    } else {
        else_ops.code()?
    };
    cached_shape_method(scheduler, code, if work { 29 } else { 12 })
}

fn handler_task_fact(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<HandlerTaskPlan> {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (HANDLER_TASK_FACT_SLOTS - 1);
    if let Some(plan) = HANDLER_TASK_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.plan)
    }) {
        return plan;
    }
    let plan = match_handler_task(function);
    HANDLER_TASK_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(HANDLER_TASK_FACT_SLOTS, || None);
        }
        facts[index] = Some(HandlerTaskFact {
            function: std::rc::Rc::downgrade(function),
            plan,
        });
    });
    plan
}

include!("functions_handler_task_match.rs");
