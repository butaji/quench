
#[allow(dead_code)]
pub fn use_effect<F, D>(_callback: F, _deps: D)
where
    F: Fn() + 'static,
    D: AsRef<[usize]>,
{
}

#[allow(dead_code)]
pub fn use_layout_effect<F, D>(_callback: F, _deps: D)
where
    F: Fn() + 'static,
    D: AsRef<[usize]>,
{
}
