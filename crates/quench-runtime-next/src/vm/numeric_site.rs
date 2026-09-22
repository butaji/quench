#[derive(Clone, Copy, Default)]
pub(super) struct NumericSite {
    pub(super) consistent_fast: u16,
    pub(super) slow_path: u16,
    pub(super) armed: bool,
}
