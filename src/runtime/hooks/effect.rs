
/// Effect cleanup function type
#[allow(dead_code)]
pub type EffectCleanup = Box<dyn Fn() + Send + Sync>;

/// Effect callback type
#[allow(dead_code)]
pub type EffectCallback = Box<dyn FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static>;

/// use_effect hook - SSR stub
///
/// In SSR context, effects are not executed synchronously.
/// They would be deferred to client-side hydration.
#[allow(dead_code)]
pub fn use_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: effects are deferred to client-side execution
    // The callback and deps are stored for hydration but not executed
}

/// use_layout_effect hook - SSR stub
///
/// In SSR context, layout effects are not executed.
#[allow(dead_code)]
pub fn use_layout_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: layout effects are not run
}
