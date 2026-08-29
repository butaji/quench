pub fn linked_list_init(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let item = args.first().ok_or(VmError::NotCallable)?.clone();
    if let Some(id) = item.object_identity() {
        LINKED_LISTS.with(|lists| {
            lists.borrow_mut().insert(id, (item.clone(), item));
        });
    }
    Ok(Value::Undefined)
}
pub fn linked_list_remove(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let item = args.first().ok_or(VmError::NotCallable)?.clone();
    let Some(id) = item.object_identity() else {
        return Ok(Value::Undefined);
    };
    LINKED_LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        if let Some((after, before)) = lists.get(&id).cloned() {
            if let Some(before_id) = before.object_identity() {
                if let Some(entry) = lists.get_mut(&before_id) {
                    entry.0 = after.clone();
                }
            }
            if let Some(after_id) = after.object_identity() {
                if let Some(entry) = lists.get_mut(&after_id) {
                    entry.1 = before.clone();
                }
            }
            lists.insert(id, (item.clone(), item));
        }
    });
    Ok(Value::Undefined)
}
pub fn linked_list_append(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?.clone();
    let item = args.get(1).ok_or(VmError::NotCallable)?.clone();
    if !LINKED_LISTS.with(|lists| {
        list.object_identity()
            .is_some_and(|id| lists.borrow().contains_key(&id))
    }) {
        linked_list_init(state, receiver, std::slice::from_ref(&list))?;
    }
    if !LINKED_LISTS.with(|lists| {
        item.object_identity()
            .is_some_and(|id| lists.borrow().contains_key(&id))
    }) {
        linked_list_init(state, receiver, std::slice::from_ref(&item))?;
    }
    linked_list_remove(state, receiver, std::slice::from_ref(&item))?;
    let Some(list_id) = list.object_identity() else {
        return Ok(Value::Undefined);
    };
    let Some(item_id) = item.object_identity() else {
        return Ok(Value::Undefined);
    };
    LINKED_LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let tail = lists
            .get(&list_id)
            .map(|entry| entry.1.clone())
            .unwrap_or_else(|| list.clone());
        let tail_id = tail.object_identity().unwrap_or(list_id);
        if let Some(entry) = lists.get_mut(&tail_id) {
            entry.0 = item.clone();
        }
        lists.insert(item_id, (list.clone(), tail));
        if let Some(entry) = lists.get_mut(&list_id) {
            entry.1 = item;
        }
    });
    Ok(Value::Undefined)
}
pub fn linked_list_is_empty(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?;
    let id = list.object_identity();
    Ok(Value::Boolean(LINKED_LISTS.with(|lists| {
        id.and_then(|id| {
            lists
                .borrow()
                .get(&id)
                .map(|entry| entry.0.object_identity() == Some(id))
        })
        .unwrap_or(true)
    })))
}
pub fn linked_list_peek(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?;
    let id = list.object_identity();
    Ok(LINKED_LISTS.with(|lists| {
        id.and_then(|id| {
            lists.borrow().get(&id).map(|entry| {
                if entry.0.object_identity() == Some(id) {
                    Value::Null
                } else {
                    entry.0.clone()
                }
            })
        })
        .unwrap_or(Value::Null)
    }))
}
pub fn dns_lookup(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::lookup(state, args)
}
pub fn dns_resolve4(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::resolve4(state, args)
}
pub fn net_connect(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match receiver {
        Some(receiver) if crate::modules::net::net_id(receiver).is_some() => {
            crate::modules::net::connect_existing(state, receiver, args)
        }
        _ => crate::modules::net::connect(state, args),
    }
}
pub fn net_lookup_callback(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let result = host_api::array(args.to_vec());
    state.borrow_mut().net.lookup_result = Some(result.clone());
    Ok(result)
}
pub fn net_socket_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::net::socket_construct(state, args)
}
pub fn net_is_ip(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::net::is_ip(args) as f64))
}
pub fn net_is_ipv4(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv4(args)))
}
pub fn net_is_ipv6(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv6(args)))
}
pub fn net_create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::net::create_server(state, args)
}
/// `net.createServer(...)` is also invocable as a plain function (the
/// construct path handles `new net.createServer(...)`).
pub fn net_create_server_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::net::create_server(state, args)
}
pub fn http_request(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::request(state, args)
}
pub fn http_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::get(state, args)
}
pub fn http_create_server(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::create_server(state, args)
}
pub fn http_create_server_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::create_server(state, args)
}
pub fn stream_pipeline(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::pipeline(state, args)
}
pub fn stream_readable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_readable(state, args)
}
pub fn stream_writable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_writable(state, args)
}
pub fn stream_duplex(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_duplex(state, args)
}
pub fn stream_transform(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_transform(state, args)
}
pub fn readline_create_interface(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn net_get_asf_timeout(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(250.0))
}
pub fn net_set_asf_timeout(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn fetch(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
