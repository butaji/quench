const VIRTUAL_BUILTIN_CACHE_TAG: u64 = 1 << 62;
const VIRTUAL_BUILTIN_CACHE_SLOTS: usize = 4096;

#[derive(Clone)]
struct VirtualBuiltinMethodCache {
    site: usize,
    receiver_layout: u32,
    key: crate::identity::PropertyKeyId,
    method: crate::ops::Builtin,
    intrinsic_generation: u64,
}

thread_local! {
    static VIRTUAL_BUILTIN_METHOD_CACHES:
        std::cell::RefCell<Vec<Option<VirtualBuiltinMethodCache>>> = const {
            std::cell::RefCell::new(Vec::new())
        };
}

fn cacheable_virtual_builtin_method(
    object: &crate::value::ObjectData,
    key: &str,
    value: &crate::value::Value,
) -> Option<VirtualBuiltinMethodCache> {
    if key != "exec"
        || !matches!(value, crate::value::Value::Builtin(crate::ops::Builtin::RegExpExec))
        || object.physical_slot_for_name(key).is_some()
        || object
            .physical_slot_for_name(&crate::builtins::deleted_key(key))
            .is_some()
        || object
            .physical_slot_for_name(&crate::builtins::descriptor_key(key))
            .is_some()
        || !object.has_regexp_internal_slot()
    {
        return None;
    }
    Some(VirtualBuiltinMethodCache {
        site: 0,
        receiver_layout: object.semantic_layout_id(),
        key: crate::identity::property_key_id(key),
        method: crate::ops::Builtin::RegExpExec,
        intrinsic_generation: crate::builtins::intrinsic_override_generation(),
    })
}

fn virtual_builtin_cache_hit(
    _object: &crate::value::ObjectData,
    layout: u32,
    cache: u64,
    site: usize,
) -> Option<NamedCachedPayload> {
    let index = virtual_builtin_cache_index(cache)?;
    VIRTUAL_BUILTIN_METHOD_CACHES.with(|caches| {
        let entry = caches.borrow().get(index).and_then(Clone::clone)?;
        (entry.site == site
            && entry.receiver_layout == layout
            && crate::builtins::intrinsic_override_generation() == entry.intrinsic_generation)
            .then_some(NamedCachedPayload::Value(crate::value::Value::Builtin(entry.method)))
    })
}

fn install_virtual_builtin_cache(
    cache: &std::cell::Cell<u64>,
    mut entry: VirtualBuiltinMethodCache,
) {
    let site = cache as *const _ as usize;
    entry.site = site;
    let index = (site
        .wrapping_mul(0x9e37_79b1)
        .wrapping_add(entry.receiver_layout as usize)
        .wrapping_add(entry.key.0 as usize))
        & (VIRTUAL_BUILTIN_CACHE_SLOTS - 1);
    VIRTUAL_BUILTIN_METHOD_CACHES.with(|caches| {
        let mut caches = caches.borrow_mut();
        if caches.is_empty() {
            caches.resize_with(VIRTUAL_BUILTIN_CACHE_SLOTS, || None);
        }
        caches[index] = Some(entry);
    });
    cache.set(VIRTUAL_BUILTIN_CACHE_TAG | index as u64 + 1);
}

fn virtual_builtin_cache_index(cache: u64) -> Option<usize> {
    (cache & VIRTUAL_BUILTIN_CACHE_TAG != 0 && cache & PROTOTYPE_CACHE_TAG == 0)
        .then(|| usize::try_from((cache & !VIRTUAL_BUILTIN_CACHE_TAG).checked_sub(1)?).ok())?
}
