//! Context hooks

#[allow(dead_code)]
pub struct Context<T>(pub std::marker::PhantomData<T>);
#[allow(dead_code)]
pub fn create_context<T: Clone + Send + Sync + 'static>(_default: T) -> Context<T> {
    Context(std::marker::PhantomData)
}
#[allow(dead_code)]
pub fn use_context<T: Clone>(_ctx: &Context<T>) -> T {
    todo!()
}
