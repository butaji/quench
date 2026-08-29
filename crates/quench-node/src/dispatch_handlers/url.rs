pub fn url_parse(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::parse(state, args)
}
pub fn url_format(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::format(state, receiver, args)
}
pub fn url_resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::resolve(state, args)
}
pub fn url_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url_whatwg::new_url(state, args)
}
pub fn url_legacy_new(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let object = Value::object(Vec::new());
    Ok(quench_runtime::execute::set_prototype_of(
        &object,
        &crate::modules::url::legacy_url_prototype(),
    )?)
}
pub fn url_resolve_object(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::resolve_object(state, receiver, args)
}
pub fn url_search_params(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::new_search_params(state, args)
}
pub fn url_path_to_file_url(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url_file::path_to_file_url(state, None, args)
}
