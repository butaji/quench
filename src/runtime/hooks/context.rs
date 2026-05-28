//! Context hooks

pub struct Context<T>(pub std::marker::PhantomData<T>);
pub fn create_context<T: Clone + Send + Sync + 'static>(_default: T) -> Context<T> {
    Context(std::marker::PhantomData)
}
pub fn use_context<T: Clone>(_ctx: &Context<T>) -> T {
    todo!()
}
