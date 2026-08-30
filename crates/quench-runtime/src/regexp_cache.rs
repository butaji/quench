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

/// Drop compiled patterns at realm boundaries so a long fixture sweep cannot
/// retain an unbounded amount of generated RegExp state.
/// Drop compiled patterns at a fixture or realm boundary.
pub fn reset_compiled_cache() {
    COMPILED_REGEXPS.with(|cache| cache.replace(HashMap::new()));
}

#[inline]
fn regexp_cache_key(source: &str, flags: &str) -> u64 {
    crate::strings::hash_str(source) ^ crate::strings::hash_str(flags).rotate_left(23)
}

fn compiled_for(_: &Value, source: &str, flags: &str) -> Result<Rc<Regex>, VmError> {
    let key = regexp_cache_key(source, flags);
    COMPILED_REGEXPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(&key) {
            if entry.source == source && entry.flags == flags {
                crate::execution_trace::event(crate::execution_trace::Event::RegExpCacheHit);
                return Ok(Rc::clone(&entry.regex));
            }
        }
        crate::execution_trace::event(crate::execution_trace::Event::RegExpCacheMiss);
        let regex = Rc::new(compile_linear(source, flags).map_err(VmError::EvalError)?);
        if cache.len() >= REGEXP_CACHE_LIMIT && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, CompiledEntry {
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
