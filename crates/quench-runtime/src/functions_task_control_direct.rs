#[derive(Clone, Copy)]
struct TaskControlRunPlan {
    suspended_runnable: f64,
    running: f64,
    runnable: f64,
}

struct TaskControlWords<'a> {
    state: &'a crate::register_file::SlotWord,
    queue: &'a crate::register_file::SlotWord,
    task: &'a crate::register_file::SlotWord,
    direct: Option<&'a DirectTaskRunner>,
}

fn task_control_words<'a>(
    tcb: &'a crate::value::ObjectData,
    direct: Option<&'a DirectTaskRunner>,
) -> Option<TaskControlWords<'a>> {
    if tcb.has_replacement() {
        return None;
    }
    let state = direct
        .map(|runner| runner.word(runner.state))
        .or_else(|| writable_own_word(tcb, "state"))?;
    let queue = direct
        .map(|runner| runner.word(runner.queue))
        .or_else(|| writable_own_word(tcb, "queue"))?;
    let task = direct
        .map(|runner| runner.word(runner.task_word))
        .or_else(|| crate::vm::proven_own_word(tcb, "task"))?;
    let pointer = task.object_or_null_ptr().flatten();
    let direct = direct.filter(|runner| pointer.is_some_and(|task| runner.matches(task)));
    Some(TaskControlWords { state, queue, task, direct })
}

fn take_task_packet(
    words: &TaskControlWords<'_>,
    plan: &TaskControlRunPlan,
    packet_link_cache: &std::cell::Cell<u64>,
) -> Option<crate::value::Value> {
    if words.state.number()? != plan.suspended_runnable {
        return Some(crate::value::Value::Null);
    }
    let packet = words.queue.load();
    let crate::value::Value::Object(packet_object) = &packet else { return None };
    if packet_object.has_replacement() { return None; }
    let link = linked_schedule_word(packet_object, "link", packet_link_cache)?;
    let next = link.load();
    words.queue.store(next.clone());
    let next_state = if next.is_nullish() { plan.running } else { plan.runnable };
    words.state.store(crate::value::Value::Number(next_state));
    Some(packet)
}

fn execute_task_control_run(
    tcb: &crate::value::ObjectData,
    plan: &TaskControlRunPlan,
    direct: Option<&DirectTaskRunner>,
    packet_link_cache: &std::cell::Cell<u64>,
    task_runners: Option<&DirectTaskTable>,
    linked_scheduler: Option<&LinkedSchedulerWords>,
) -> Result<Option<DirectTaskOutcome>, crate::execute::VmError> {
    let Some(words) = task_control_words(tcb, direct) else { return Ok(None) };
    let Some(packet) = take_task_packet(&words, plan, packet_link_cache) else { return Ok(None) };
    let outcome = match words.direct {
        Some(runner) => execute_direct_task(runner, packet, task_runners, linked_scheduler),
        None => {
            let Some(outcome) = execute_dynamic_task(words.task.load(), packet)? else {
                return Ok(None);
            };
            outcome
        }
    };
    crate::execution_trace::kernel("task_control_run_word_slots", false);
    Ok(Some(outcome))
}

fn execute_direct_task(
    runner: &DirectTaskRunner,
    packet: crate::value::Value,
    task_runners: Option<&DirectTaskTable>,
    linked_scheduler: Option<&LinkedSchedulerWords>,
) -> DirectTaskOutcome {
    let step = task_runners.and_then(|table| runner.execute(&packet, table, linked_scheduler));
    crate::execution_trace::kernel("linked_task_direct", step.is_none());
    step.map_or(DirectTaskOutcome::Miss(packet), DirectTaskOutcome::Step)
}

fn execute_dynamic_task(
    task: crate::value::Value,
    packet: crate::value::Value,
) -> Result<Option<DirectTaskOutcome>, crate::execute::VmError> {
    let run = crate::execute::get_property_result(&task, "run")?;
    if !crate::conversion::is_callable(&run) {
        return Ok(None);
    }
    let value = crate::functions::execute_target(&run, &task, &[packet])?;
    Ok(Some(DirectTaskOutcome::Value(value)))
}
