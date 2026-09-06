//! Shared RAII accounting for bounded process resources.

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub(crate) struct AtomicBudget {
    used: AtomicUsize,
    limit: usize,
}

impl AtomicBudget {
    pub(crate) const fn new(limit: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit,
        }
    }

    pub(crate) fn reserve(&self, bytes: usize) -> Option<BudgetReservation<'_>> {
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

    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub(crate) struct BudgetReservation<'a> {
    budget: &'a AtomicBudget,
    bytes: usize,
}

impl BudgetReservation<'_> {
    pub(crate) fn grow(&mut self, bytes: usize) -> bool {
        let Some(mut extra) = self.budget.reserve(bytes) else {
            return false;
        };
        self.bytes = self.bytes.saturating_add(extra.bytes);
        extra.bytes = 0;
        true
    }

    #[cfg(test)]
    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for BudgetReservation<'_> {
    fn drop(&mut self) {
        if self.bytes != 0 {
            self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_grow_and_release_from_one_owner() {
        let budget = AtomicBudget::new(10);
        let mut owner = budget.reserve(6).expect("initial reservation");
        assert!(!owner.grow(5));
        assert!(owner.grow(4));
        assert_eq!(owner.bytes(), 10);
        assert_eq!(budget.used(), 10);
        drop(owner);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn overflow_cannot_wrap_the_budget() {
        let budget = AtomicBudget::new(usize::MAX);
        let owner = budget.reserve(usize::MAX).expect("full reservation");
        assert!(budget.reserve(1).is_none());
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
}
