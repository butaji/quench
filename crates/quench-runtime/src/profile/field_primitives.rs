//! Successful shared field operations, not attempts or exclusive instruction costs.

use std::cell::Cell;

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Path {
    HintOwnRead,
    HintHolderRead,
    PrimaryOwnRead,
    PrimaryStoreNoLog,
    PrimaryStoreLogged,
    HintedPrimaryStore,
    HintedStore,
    PolymorphicStore,
    CachedStore,
    GeneralStore,
}

impl Path {
    const COUNT: usize = Self::GeneralStore as usize + 1;
    const NAMES: [&'static str; Self::COUNT] = [
        "hint_own_read",
        "hint_holder_read",
        "primary_own_read",
        "primary_store_no_log",
        "primary_store_logged",
        "hinted_primary_store",
        "hinted_store",
        "polymorphic_store",
        "cached_store",
        "general_store",
    ];
}

#[derive(Default)]
pub(crate) struct Census(Cell<[u64; Path::COUNT]>);

impl Census {
    #[inline(always)]
    pub(crate) fn hit(&self, path: Path) {
        let mut counts = self.0.get();
        counts[path as usize] += 1;
        self.0.set(counts);
    }

    pub(crate) fn report(&self) {
        if std::env::var_os("QUENCH_FIELD_PRIMITIVE_COUNTS").is_some() {
            eprintln!(
                "{{\"kind\":\"quench-field-primitives\",\"names\":{:?},\"counts\":{:?}}}",
                Path::NAMES,
                self.0.get(),
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn count(&self, path: Path) -> u64 {
        self.0.get()[path as usize]
    }
}
