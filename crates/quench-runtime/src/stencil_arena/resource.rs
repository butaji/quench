//! Derived executable-resource accounting.
//!
//! The slab pool and its lease state remain the only mutable authorities.
//! This module exposes a value snapshot for diagnostics and deterministic
//! lifecycle contracts; it owns no counters or second retirement state.

use super::SharedStencilSlab;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExecutableResourceSnapshot {
    pub resident_bytes: usize,
    pub used_bytes: usize,
    pub retired_live_bytes: usize,
    pub cache_rows: usize,
    pub active_leases: usize,
    pub retired_owners: usize,
    pub process_resident_bytes: usize,
}

impl SharedStencilSlab {
    pub(crate) fn resource_snapshot(&self) -> ExecutableResourceSnapshot {
        let retired_live_bytes = self
            .slabs
            .iter()
            .filter(|slab| self.lease_state.is_retired(slab.id()))
            .map(|slab| slab.capacity())
            .sum();
        ExecutableResourceSnapshot {
            resident_bytes: self.capacity(),
            used_bytes: self.used(),
            retired_live_bytes,
            cache_rows: self.cache.len(),
            active_leases: self.active_leases(),
            retired_owners: self.lease_state.retired.borrow().len(),
            process_resident_bytes: super::GLOBAL_EXECUTABLE_BUDGET.used(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;
    use crate::stencil_fact::PatchValues;
    use crate::stencil_select::RenderedRegionCache;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn retired_live_accounting_follows_the_retaining_lease() {
        let pool = Rc::new(RefCell::new(SharedStencilSlab::new(4096).unwrap()));
        let key = crate::stencil_select::numeric_region_key(Opcode::Add).unwrap();
        let view = crate::stencil_select::select_physical(key).unwrap();
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let mut cache = RenderedRegionCache::new();
        let address = render(&pool, &mut cache, view, &values);
        let token = pool.borrow().owned_f64_entry(address).unwrap();
        let lease = SharedStencilSlab::acquire_owned(&pool, token).unwrap();

        pool.borrow_mut()
            .retire_allocation(address, &mut cache)
            .unwrap();
        assert_retired(pool.borrow().resource_snapshot());
        let replacement = render(&pool, &mut cache, view, &values);
        assert_ne!(replacement, address);
        assert_eq!(pool.borrow().resource_snapshot().resident_bytes, 8192);

        assert_eq!(lease.invoke(|entry| entry(3.0, 4.0)), Ok(7.0));
        assert_released(pool.borrow().resource_snapshot());
        assert_eq!(pool.borrow_mut().evict_idle_with_cache(&mut cache, 0), 1);
        assert_eq!(pool.borrow().resource_snapshot().resident_bytes, 0);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn render(
        pool: &Rc<RefCell<SharedStencilSlab>>,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, 2>,
    ) -> usize {
        let address = pool
            .borrow_mut()
            .render_physical_view_or_get(cache, view, values)
            .unwrap();
        pool.borrow_mut().make_executable(address).unwrap();
        address
    }

    fn assert_retired(snapshot: ExecutableResourceSnapshot) {
        assert_eq!(snapshot.resident_bytes, 4096);
        assert_eq!(snapshot.retired_live_bytes, 4096);
        assert_eq!(snapshot.active_leases, 1);
        assert_eq!(snapshot.retired_owners, 1);
        assert_eq!(snapshot.cache_rows, 0);
        assert!(snapshot.process_resident_bytes >= snapshot.resident_bytes);
    }

    fn assert_released(snapshot: ExecutableResourceSnapshot) {
        assert_eq!(snapshot.resident_bytes, 4096);
        assert_eq!(snapshot.retired_live_bytes, 0);
        assert_eq!(snapshot.active_leases, 0);
        assert_eq!(snapshot.retired_owners, 0);
        assert!(snapshot.cache_rows > 0);
    }
}
