impl Default for FilesystemNodeHost {
    fn default() -> Self {
        Self {
            resolver: RefCell::new(None),
            source_cache: RefCell::new(HashMap::new()),
        }
    }
}

impl NodeHost for FilesystemNodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if parent.is_none() && Path::new(request).exists() {
            return Ok(PathBuf::from(request));
        }
        let base = parent
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        if request == "../common"
            || request.ends_with("/common")
            || request.ends_with("/common/index.js")
        {
            let fixture_common = Path::new("tests/node/test/common/index.js");
            if fixture_common.exists() {
                return Ok(fixture_common.to_path_buf());
            }
        }
        let mut resolver = self.resolver.borrow_mut();
        let resolver = resolver
            .get_or_insert_with(|| Resolver::new(ResolveOptions::default()));
        resolver
            .resolve(base, request)
            .map(|resolution| resolution.full_path().to_path_buf())
            .map_err(|error| error.to_string().into())
    }

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(source) = self.source_cache.borrow().get(path).cloned() {
            return Ok(source);
        }
        let source = std::fs::read_to_string(path)?;
        self.source_cache
            .borrow_mut()
            .insert(path.to_path_buf(), source.clone());
        Ok(source)
    }
}

/// Host state is a product of independent keyed stores. Declare the empty
/// stores as data once and let the macro expand the uniform `RefCell` wiring;
/// counters and state machines remain explicit below.
macro_rules! empty_host_maps {
    ($($field:ident),+ $(,)?) => {
        $( $field: RefCell::new(HashMap::new()), )+
    };
}

pub(crate) trait NodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>>;

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>>;
}

pub(crate) trait JsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>>;

}
pub(crate) struct QuenchRuntime;

struct QuenchNodeHost {
    hashes: RefCell<HashMap<u16, (String, Vec<u8>)>>,
    hash_objects: RefCell<HashMap<u16, Value>>,
    dgram_states: RefCell<HashMap<u16, (bool, bool, u16)>>,
    dgram_listeners: RefCell<HashMap<u16, Value>>,
    next_dgram: Cell<u16>,
    streams: RefCell<HashMap<u16, StreamState>>,
    next_hash: Cell<u16>,
    next_stream: Cell<u16>,
    http: RefCell<HttpState>,
    urls: RefCell<HashMap<u16, String>>,
    url_objects: RefCell<HashMap<u16, Value>>,
    next_url: Cell<u16>,
    params_state: RefCell<HashMap<u16, Vec<(String, String)>>>,
    params_objects: RefCell<HashMap<u16, Value>>,
    next_params: Cell<u16>,
    event_max: RefCell<HashMap<u16, f64>>,
    next_event: Cell<u16>,
    fd_paths: RefCell<HashMap<i32, String>>,
    next_fd: Cell<i32>,
    fd_modes: RefCell<HashMap<i32, u32>>,
    directories: RefCell<HashMap<u16, (Vec<Value>, usize)>>,
    next_directory: Cell<u16>,
    common_wrappers: RefCell<HashMap<u16, (Value, bool, u32, Value)>>,
    next_common_wrapper: Cell<u16>,
    promisified: RefCell<HashMap<u16, Value>>,
    next_promisified: Cell<u16>,
    deprecated: RefCell<HashMap<u16, Value>>,
    next_deprecated: Cell<u16>,
    pending_promises: RefCell<HashMap<u16, Rc<quench_runtime::value::PromiseData>>>,
    next_promise: Cell<u16>,
}

struct StreamState {
    transform: Option<Value>,
    read: Option<Value>,
    data: Option<Value>,
    end: Option<Value>,
    drain: Option<Value>,
    error: Option<Value>,
    close: Option<Value>,
    destroy: Option<Value>,
    source: Vec<Value>,
    need_drain: bool,
    destroyed: bool,
    errored: Option<Value>,
}

struct HttpState {
    server_callback: Option<Value>,
    body: String,
    data_callback: Option<Value>,
    end_callback: Option<Value>,
}

impl Default for QuenchNodeHost {
    fn default() -> Self {
        Self {
            empty_host_maps!(hashes, hash_objects, dgram_states, dgram_listeners),
            next_dgram: Cell::new(1),
            empty_host_maps!(streams),
            next_hash: Cell::new(100),
            next_stream: Cell::new(200),
            http: RefCell::new(HttpState {
                server_callback: None,
                body: String::new(),
                data_callback: None,
                end_callback: None,
            }),
            empty_host_maps!(urls, url_objects),
            next_url: Cell::new(600),
            empty_host_maps!(params_state, params_objects),
            next_params: Cell::new(700),
            empty_host_maps!(event_max),
            next_event: Cell::new(900),
            empty_host_maps!(fd_paths),
            next_fd: Cell::new(3),
            empty_host_maps!(fd_modes, directories),
            next_directory: Cell::new(1),
            empty_host_maps!(common_wrappers),
            next_common_wrapper: Cell::new(CapabilityName::CommonWrapperFirst),
            empty_host_maps!(promisified),
            next_promisified: Cell::new(CapabilityName::UtilPromisifiedFirst),
            empty_host_maps!(deprecated),
            next_deprecated: Cell::new(CapabilityName::UtilDeprecatedFirst),
            empty_host_maps!(pending_promises),
            next_promise: Cell::new(CapabilityName::UtilResolverFirst),
        }
    }
}
