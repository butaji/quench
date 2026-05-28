use super::*;

#[derive(Clone)]
pub struct Context<T: Clone + Send + Sync + 'static> { _marker: std::marker::PhantomData<T> }

pub fn create_context<T: Clone + Send + Sync + 'static>(_default_value: T) -> Context<T> {
    Context { _marker: std::marker::PhantomData }
}

pub fn use_context<T>(_context: &Context<T>) -> T where T: Clone { todo!() }

pub fn use_debug_value<T>(_value: T) { }
pub fn use_debug_value_formatted<T, F>(_value: T, _format: F) { }
