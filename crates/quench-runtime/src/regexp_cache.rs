use std::{cell::RefCell, collections::HashMap, rc::Rc};

const REGEXP_CACHE_LIMIT: usize = 512;

struct CompiledEntry {
    source: String,
    flags_key: u64,
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
fn regexp_cache_key(source: &str, flags_key: u64) -> u64 {
    crate::strings::hash_str(source) ^ flags_key.rotate_left(23)
}

fn compiled_for(_: &Value, source: &str, flags: &str) -> Result<Rc<Regex>, VmError> {
    let flags_key = regexp_flags_key(flags);
    let key = regexp_cache_key(source, flags_key);
    COMPILED_REGEXPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(&key) {
            if entry.source == source && entry.flags_key == flags_key {
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
            flags_key,
            regex: Rc::clone(&regex),
        });
        Ok(regex)
    })
}

fn regexp_flags_key(flags: &str) -> u64 {
    let mut seen = 0u16;
    let mut semantic = 0u8;
    for flag in flags.bytes() {
        let (seen_bit, semantic_bit) = match flag {
            b'd' => (1 << 0, 0),
            b'g' => (1 << 1, 0),
            b'i' => (1 << 2, 1 << 0),
            b'm' => (1 << 3, 1 << 1),
            b's' => (1 << 4, 1 << 2),
            b'u' => (1 << 5, 1 << 3),
            b'v' => (1 << 6, 1 << 4),
            b'y' => (1 << 7, 0),
            _ => return crate::strings::hash_str(flags) | (1 << 63),
        };
        if seen & seen_bit != 0 {
            return crate::strings::hash_str(flags) | (1 << 63);
        }
        seen |= seen_bit;
        semantic |= semantic_bit;
    }
    if semantic & (1 << 3) != 0 && semantic & (1 << 4) != 0 {
        return crate::strings::hash_str(flags) | (1 << 63);
    }
    u64::from(semantic)
}

fn compile_linear(source: &str, flags: &str) -> Result<Regex, String> {
    compile(source, flags)
}
