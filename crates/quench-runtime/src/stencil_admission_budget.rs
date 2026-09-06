//! Process and owner bounds for optional native-admission metadata.

use crate::bounded_resource::{AtomicBudget, BudgetReservation};

pub(crate) const MAX_OWNER_ADMISSION_BYTES: usize = 256 << 10;
pub(crate) const MAX_GLOBAL_ADMISSION_BYTES: usize = 16 << 20;

static GLOBAL_BUDGET: AtomicBudget = AtomicBudget::new(MAX_GLOBAL_ADMISSION_BYTES);

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
        self.reservation.grow(bytes)
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.reservation.bytes()
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
    fn admission_charge_delegates_growth_to_its_owner() {
        let mut charge = AdmissionMetadataCharge::reserve(6).expect("initial charge");
        assert!(charge.grow(4));
        assert_eq!(charge.bytes(), 10);
    }
}
