pub type CallHandler =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;
pub type ConstructHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;
thread_local! {
    static OS_PRIORITY: Cell<i32> = const { Cell::new(0) };
    static EVENT_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static GC_EPOCH: Cell<u64> = const { Cell::new(0) };
}
static PROCESS_START: OnceLock<Instant> = OnceLock::new();
pub fn console_log(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, false)
}
pub fn console_warn(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, true)
}
pub fn console_trace(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::trace(state, args)
}
thread_local! { static LINKED_LISTS: RefCell<HashMap<u64, (Value, Value)>> = RefCell::new(HashMap::new()); }
