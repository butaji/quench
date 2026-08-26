fn execute_direct_schedule_cursor(
    receiver: &crate::value::Value,
    plan: &LinkedSchedulePlan<'_>,
    start: &crate::value::Value,
    current: &crate::register_file::SlotWord,
    current_id: &crate::register_file::SlotWord,
    run: &TaskControlRunPlan,
    table: &DirectTaskTable,
    owner: &crate::value::ObjectData,
    scheduler: &LinkedSchedulerWords,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if !table.matches_scheduler(owner) {
        return Ok(None);
    }
    let Some(mut cursor) = table.id_for_value(start) else { return Ok(None) };
    let packet_link_cache = std::cell::Cell::new(0);
    while let Some(index) = cursor {
        let Some(task) = table.for_id(index) else {
            return direct_schedule_slow(receiver, plan, current, current_id, table, index);
        };
        if task.is_held_or_suspended().unwrap_or(false) {
            let Some(next) = table.id_for_word(task.word(task.link)) else {
                return direct_schedule_slow(receiver, plan, current, current_id, table, index);
            };
            cursor = next;
        } else {
            current_id.store(crate::value::Value::Number(index as f64));
            let outcome = execute_task_control_run(
                task.tcb(),
                run,
                Some(task),
                &packet_link_cache,
                Some(table),
                Some(scheduler),
            )?;
            let Some(outcome) = outcome else {
                return direct_schedule_slow(receiver, plan, current, current_id, table, index);
            };
            cursor = match outcome {
                DirectTaskOutcome::Step(step) => step.next,
                DirectTaskOutcome::Miss(packet) => {
                    current.store(task.tcb_value.clone());
                    let run = crate::execute::get_property_result(&task.task_value, "run")?;
                    let value = crate::functions::execute_target(
                        &run,
                        &task.task_value,
                        &[packet],
                    )?;
                    let Some(next) = table.id_for_value(&value) else {
                        current.store(value);
                        return continue_linked_schedule_slow(receiver.clone(), plan).map(Some);
                    };
                    next
                }
                DirectTaskOutcome::Value(value) => {
                    let Some(next) = table.id_for_value(&value) else {
                        current.store(value);
                        return continue_linked_schedule_slow(receiver.clone(), plan).map(Some);
                    };
                    next
                }
            };
        }
        crate::execution_trace::kernel("linked_schedule_word_cursor", false);
    }
    current.store(crate::value::Value::Null);
    Ok(Some(crate::value::Value::Undefined))
}

fn direct_schedule_slow(
    receiver: &crate::value::Value,
    plan: &LinkedSchedulePlan<'_>,
    current: &crate::register_file::SlotWord,
    current_id: &crate::register_file::SlotWord,
    table: &DirectTaskTable,
    index: usize,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(task) = table.for_id(index) else { return Ok(None) };
    current.store(task.tcb_value.clone());
    current_id.store(crate::value::Value::Number(index as f64));
    continue_linked_schedule_slow(receiver.clone(), plan).map(Some)
}
