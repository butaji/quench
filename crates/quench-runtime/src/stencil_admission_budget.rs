//! Process and owner bounds for optional native-admission metadata.

use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) const MAX_OWNER_ADMISSION_BYTES: usize = 256 << 10;
pub(crate) const MAX_GLOBAL_ADMISSION_BYTES: usize = 16 << 20;

static GLOBAL_BUDGET: AtomicBudget = AtomicBudget::new(MAX_GLOBAL_ADMISSION_BYTES);

#[derive(Debug)]
struct AtomicBudget {
    used: AtomicUsize,
    limit: usize,
}

impl AtomicBudget {
    const fn new(limit: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit,
        }
    }

    fn reserve(&self, bytes: usize) -> Option<BudgetReservation<'_>> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.limit)
            })
            .ok()?;
        Some(BudgetReservation {
            budget: self,
            bytes,
        })
    }

    fn release(&self, bytes: usize) {
        if bytes != 0 {
            self.used.fetch_sub(bytes, Ordering::AcqRel);
        }
    }

    #[cfg(test)]
    fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct BudgetReservation<'a> {
    budget: &'a AtomicBudget,
    bytes: usize,
}

impl Drop for BudgetReservation<'_> {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

impl BudgetReservation<'_> {
    fn absorb(&mut self, mut other: Self) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        other.bytes = 0;
    }
}

#[derive(Debug)]
pub(crate) struct AdmissionMetadataCharge {
    reservation: BudgetReservation<'static>,
}

impl AdmissionMetadataCharge {
    pub(crate) fn reserve(bytes: usize) -> Option<Self> {
        Some(Self {
            reservation: GLOBAL_BUDGET.reserve(bytes)?,
        })
    }

    pub(crate) fn grow(&mut self, bytes: usize) -> bool {
        let Some(extra) = GLOBAL_BUDGET.reserve(bytes) else {
            return false;
        };
        self.reservation.absorb(extra);
        true
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.reservation.bytes
    }
}

pub(crate) const fn slice_bytes<T>(len: usize) -> usize {
    std::mem::size_of::<T>().saturating_mul(len)
}

pub(crate) const fn shared_value_bytes<T>() -> usize {
    // Conservative policy charge for the Rc header, payload, and padding.
    std::mem::size_of::<T>()
        .saturating_add(std::mem::size_of::<usize>() * 2)
        .saturating_add(std::mem::align_of::<T>().saturating_sub(1))
}

#[cfg(test)]
pub(crate) fn global_admission_bytes() -> usize {
    GLOBAL_BUDGET.used()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_is_bounded_and_released_once() {
        let budget = AtomicBudget::new(10);
        let first = budget.reserve(6).expect("first reservation");
        assert!(budget.reserve(5).is_none());
        let second = budget.reserve(4).expect("boundary reservation");
        assert_eq!(budget.used(), 10);
        drop(first);
        assert_eq!(budget.used(), 4);
        drop(second);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn overflow_cannot_wrap_the_budget() {
        let budget = AtomicBudget::new(usize::MAX);
        let reservation = budget.reserve(usize::MAX).expect("full reservation");
        assert!(budget.reserve(1).is_none());
        drop(reservation);
        assert_eq!(budget.used(), 0);
    }
}
