//! Fast layer: specialised representation that still carries a guard.

/// Guarded specialised payload. Climbing here does not prove Native.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fast {
    I32(i32),
    Number(f64),
}

impl Fast {
    pub fn as_i32(self) -> Option<i32> {
        match self {
            Self::I32(value) => Some(value),
            Self::Number(value) if value as i32 as f64 == value => Some(value as i32),
            Self::Number(_) => None,
        }
    }

    pub fn as_number(self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(value),
            Self::I32(value) => Some(value as f64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Fast;

    #[test]
    fn i32_guard_rejects_fraction() {
        assert_eq!(Fast::Number(1.5).as_i32(), None);
        assert_eq!(Fast::Number(2.0).as_i32(), Some(2));
    }
}
