pub(crate) fn proxy_set(
    target: &Value,
    prop: &str,
    value: &Value,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    std::fs::OpenOptions::new().create(true).append(true).open("/tmp/ps_trace.log").ok()
        .map(|mut f| std::io::Write::write_all(&mut f, format!("proxy_set target={:?} prop={}\n", target, prop).as_bytes()).ok());
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "set") {
            let receiver = receiver.unwrap_or(target);
            let result = call_trap(
                &trap,
                &[
                    target.clone(),
                    Value::String(prop.to_string()),
                    value.clone(),
                    receiver.clone(),
                ],
                None,
            )?;
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
        }
        return proxy_set(&proxy.target, prop, value, Some(&proxy.target));
    }
    let receiver = receiver.unwrap_or(target);
    crate::properties::set_with_receiver(target, prop, value, receiver).map(Value::Boolean)
}
