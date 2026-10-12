//! Mutable process defaults shared by all stream constructors and public APIs.
pub(crate) const DEFAULT_OBJECT_HIGH_WATER_MARK: f64 = 16.0;
pub(crate) const DEFAULT_BYTE_HIGH_WATER_MARK: f64 =
    if cfg!(windows) { 16_384.0 } else { 65_536.0 };
pub(crate) const MAX_SAFE_HIGH_WATER_MARK: f64 = ((1_u64 << f64::MANTISSA_DIGITS) - 1) as f64;

pub(crate) struct HighWaterMarks {
    pub(crate) objects: f64,
    pub(crate) bytes: f64,
}
impl Default for HighWaterMarks {
    fn default() -> Self {
        Self {
            objects: DEFAULT_OBJECT_HIGH_WATER_MARK,
            bytes: DEFAULT_BYTE_HIGH_WATER_MARK,
        }
    }
}
