fn append_promise_node(
    promise: &Rc<PromiseData>,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    if let Some(&id) = ids.get(&(Rc::as_ptr(promise) as usize)) {
        output.push(id);
    }
}

fn append_promise_edges(
    promise: &Rc<PromiseData>,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    if let Some(value) = promise.prototype.borrow().as_ref() {
        append_edges(value, ids, output);
    }
    for (_, value) in promise.properties.borrow().iter() {
        append_edges(value, ids, output);
    }
    append_promise_state(&promise.state.borrow(), ids, output);
    if let Some(value) = promise.result.borrow().as_ref() {
        append_edges(value, ids, output);
    }
    for (resolve, reject) in promise.then_actions.borrow().iter() {
        resolve.iter().chain(reject.iter()).for_each(|value| {
            append_edges(value, ids, output);
        });
    }
    for continuation in promise.continuations.borrow().iter() {
        append_continuation_edges(continuation, ids, output);
    }
    for (aggregate, _) in promise.aggregate_hooks.borrow().iter() {
        append_aggregate_edges(aggregate, ids, output);
    }
}

fn append_promise_state(
    state: &crate::value::PromiseState,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    match state {
        crate::value::PromiseState::Pending => {}
        crate::value::PromiseState::Fulfilled(value)
        | crate::value::PromiseState::Rejected(value) => append_edges(value, ids, output),
    }
}

fn append_continuation_edges(
    continuation: &crate::value::PromiseContinuation,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    use crate::value::PromiseContinuation;
    match continuation {
        PromiseContinuation::AsyncGenerator {
            generator, result, ..
        }
        | PromiseContinuation::AsyncGeneratorYield { generator, result } => {
            append_edges(&Value::Generator(Rc::clone(generator)), ids, output);
            append_promise_node(result, ids, output);
        }
        PromiseContinuation::ArrayFromAsync {
            result,
            iterator,
            receiver,
            mapper,
            this_arg,
            values,
            array_like,
            target,
            ..
        } => {
            append_promise_node(result, ids, output);
            append_array_continuation_values(
                [
                    Some(iterator),
                    receiver.as_ref(),
                    mapper.as_ref(),
                    Some(this_arg),
                    target.as_ref(),
                ],
                values,
                array_like.as_ref(),
                ids,
                output,
            );
        }
        PromiseContinuation::Aggregate { aggregate, .. } => {
            append_aggregate_edges(aggregate, ids, output);
        }
        PromiseContinuation::Thenable {
            target,
            thenable,
            then,
        } => {
            append_promise_node(target, ids, output);
            append_edges(thenable, ids, output);
            append_edges(then, ids, output);
        }
    }
}

fn append_aggregate_edges(
    aggregate: &Rc<crate::value::PromiseAggregate>,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    append_edges(&aggregate.resolve, ids, output);
    append_edges(&aggregate.reject, ids, output);
    aggregate
        .values
        .borrow()
        .iter()
        .for_each(|value| append_edges(value, ids, output));
}

fn append_array_continuation_values(
    direct: [Option<&Value>; 5],
    values: &[Value],
    array_like: Option<&(Value, usize)>,
    ids: &HashMap<usize, usize>,
    output: &mut Vec<usize>,
) {
    direct
        .into_iter()
        .flatten()
        .for_each(|value| append_edges(value, ids, output));
    values
        .iter()
        .for_each(|value| append_edges(value, ids, output));
    if let Some((value, _)) = array_like {
        append_edges(value, ids, output);
    }
}

fn clear_promise_edges(promise: &Rc<PromiseData>) {
    *promise.prototype.borrow_mut() = None;
    promise.properties.borrow_mut().clear();
    *promise.state.borrow_mut() = crate::value::PromiseState::Pending;
    *promise.result.borrow_mut() = None;
    promise.then_actions.borrow_mut().clear();
    promise.continuations.borrow_mut().clear();
    promise.aggregate_hooks.borrow_mut().clear();
}
