use std::{cell::RefCell, collections::HashMap, rc::Rc};

const REGEXP_CACHE_LIMIT: usize = 512;

struct CompiledEntry {
    source: String,
    flags: String,
    regex: Rc<Regex>,
}

thread_local! {
    static COMPILED_REGEXPS: RefCell<HashMap<u64, CompiledEntry>> =
        RefCell::new(HashMap::new());
}

fn regexp_identity(receiver: &Value) -> Option<u64> {
    match receiver {
        Value::Object(object) => Some(object.identity()),
        Value::ObjectAlias(alias) => alias.target().map(|object| object.identity()),
        _ => None,
    }
}

fn compiled_for(receiver: &Value, source: &str, flags: &str) -> Result<Rc<Regex>, VmError> {
    let Some(identity) = regexp_identity(receiver) else {
        return compile_linear(source, flags).map(Rc::new).map_err(VmError::EvalError);
    };
    COMPILED_REGEXPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(&identity) {
            if entry.source == source && entry.flags == flags {
                return Ok(Rc::clone(&entry.regex));
            }
        }
        let regex = Rc::new(compile_linear(source, flags).map_err(VmError::EvalError)?);
        if cache.len() >= REGEXP_CACHE_LIMIT && !cache.contains_key(&identity) {
            cache.clear();
        }
        cache.insert(identity, CompiledEntry {
            source: source.to_string(),
            flags: flags.to_string(),
            regex: Rc::clone(&regex),
        });
        Ok(regex)
    })
}

fn compile_linear(source: &str, flags: &str) -> Result<Regex, String> {
    compile(source, flags)
}
