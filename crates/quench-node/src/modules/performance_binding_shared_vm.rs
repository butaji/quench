//! Genuine performance observer host state; original SDK owns observer policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext,RootId,RootedError,Runtime,Value};
include!("performance_constants.rs");
type Context<'a>=NativeContext<'a,NodeHost>;
#[derive(Default)]pub(crate)struct State{counts:Option<RootId>,milestones:Option<RootId>,callback:Option<RootId>}
fn put(c:&mut Context<'_>,object:RootId,name:&str,value:RootId)->Result<(),RootedError>{let key=c.string_rooted(name);c.define_data_property_rooted(object,key,value,true,true,true)?;Ok(())}
fn unsupported<T>(c:&mut Context<'_>,message:&str)->Result<T,RootedError>{let error=c.error_rooted(message)?;super::node_errors_shared_vm::decorate_error(c,error,"ERR_NOT_IMPLEMENTED")?;Err(c.throw(error))}
fn milestone(label:&str)->usize{MILESTONES.iter().find(|row|row.0==label).expect("SDK milestone owner").1}
fn entry(label:&str)->Option<usize>{ENTRIES.iter().find(|row|row.0==label).map(|row|row.1)}
fn typed_array(c:&mut Context<'_>,name:&str,length:usize)->Result<RootId,RootedError>{c.with_startup_intrinsics(|c|{let global=c.global_root()?;let key=c.string_rooted(name);let constructor=c.get_property_rooted(global,key)?;let length=c.number(length as f64);c.construct_rooted(constructor,constructor,&[length])})}
pub(crate)fn binding(c:&mut Context<'_>)->Result<RootId,RootedError>{crate::host::shared_vm::commonjs::cached_native_binding(c,"\0binding:performance",build)}
fn build(c:&mut Context<'_>)->Result<RootId,RootedError>{
 let module=c.object_rooted()?;let constants=c.object_rooted()?;
 for&(name,value,enumerable)in CONSTANTS{let key=c.string_rooted(name);let value=c.number(value as f64);c.define_data_property_rooted(constants,key,value,false,enumerable,false)?;}
 let key=c.string_rooted("constants");c.define_data_property_rooted(module,key,constants,false,true,false)?;
 let counts=typed_array(c,"Uint32Array",ENTRIES.len())?;put(c,module,"observerCounts",counts)?;
 let milestones=typed_array(c,"Float64Array",MILESTONES.len())?;
 for&(_,index)in MILESTONES{let key=c.string_rooted(&index.to_string());let unknown=c.number(-1.0);c.set_property_rooted(milestones,key,unknown,milestones)?;}
 let shared=c.host_mut().shared_state();let origins=shared.borrow().process_control.performance_origins();
 let Some((monotonic_ns,unix_micros))=origins else{return unsupported(c,"Performance time origin requires native monotonic and wall clocks");};
 for(label,value)in[("timeOrigin",monotonic_ns),("timeOriginTimestamp",unix_micros),("environment",monotonic_ns)]{let key=c.string_rooted(&milestone(label).to_string());let value=c.number(value);c.set_property_rooted(milestones,key,value,milestones)?;}
 put(c,module,"milestones",milestones)?;
 let retained_counts=c.retain(counts)?;let retained_milestones=match c.retain(milestones){Ok(root)=>root,Err(error)=>{c.release_root(retained_counts);return Err(error);}};
 {let mut state=shared.borrow_mut();state.performance_native.counts=Some(retained_counts);state.performance_native.milestones=Some(retained_milestones);}
 for(name,operation)in[("now","performanceNativeNow"),("setupObservers","performanceSetupObservers"),("updateGarbageCollectionTracking","performanceGcTracking"),("notify","performanceNotify"),("markBootstrapComplete","performanceBootstrapMark")]{let function=c.host_function_named(crate::host::shared_vm::operation(operation),name)?;put(c,module,name,function)?;}
 Ok(module)
}
pub(crate)fn now(c:&mut Context<'_>,_:RootId,_:&[RootId])->Result<RootId,RootedError>{{let undefined=c.undefined();super::perf_hooks::now(c,undefined,&[])}}
pub(crate)fn setup(c:&mut Context<'_>,_:RootId,args:&[RootId])->Result<RootId,RootedError>{
 let callback=args.first().copied().unwrap_or_else(||c.undefined());if !c.is_callable_rooted(callback)?{let error=c.type_error_rooted("Performance observer callback must be a function")?;return Err(c.throw(error));}
 let retained=c.retain(callback)?;let shared=c.host_mut().shared_state();let previous=shared.borrow_mut().performance_native.callback.replace(retained);if let Some(previous)=previous{c.release_root(previous);}Ok(c.undefined())
}
fn count(c:&mut Context<'_>,index:usize)->Result<u32,RootedError>{let shared=c.host_mut().shared_state();let counts=shared.borrow().performance_native.counts.ok_or_else(||RootedError::host("Performance observer storage not initialized"))?;let key=c.string_rooted(&index.to_string());let value=c.get_property_rooted(counts,key)?;c.rooted_value(value).and_then(Value::as_number).map(|value|value as u32).ok_or_else(||RootedError::host("Performance observer storage is detached or unavailable"))}
pub(crate)fn gc_tracking(c:&mut Context<'_>,_:RootId,_:&[RootId])->Result<RootId,RootedError>{
 if count(c,entry("gc").expect("SDK GC slot"))?>0{return unsupported(c,"Garbage collection PerformanceObserver events require runtime collection hooks");}
 // No collection hooks are installed when no GC observers request them.
 Ok(c.undefined())
}
pub(crate)fn notify(c:&mut Context<'_>,_:RootId,args:&[RootId])->Result<RootId,RootedError>{
 let type_root=args.first().copied().unwrap_or_else(||c.undefined());let name=c.to_string(type_root)?;
 if let Some(index)=entry(&name){if count(c,index)?>0{let shared=c.host_mut().shared_state();let callback=shared.borrow().performance_native.callback.ok_or_else(||RootedError::host("Performance observer callback not registered"))?;let value=args.get(1).copied().unwrap_or_else(||c.undefined());let undefined=c.undefined();c.call_rooted(callback,undefined,&[value])?;}}
 Ok(c.undefined())
}
pub(crate)fn bootstrap_mark(c:&mut Context<'_>,_:RootId,_:&[RootId])->Result<RootId,RootedError>{let shared=c.host_mut().shared_state();let state=shared.borrow();let milestones=state.performance_native.milestones.ok_or_else(||RootedError::host("Performance milestones not initialized"))?;let(monotonic,_)=state.process_control.performance_origins().ok_or_else(||RootedError::host("Monotonic time origin unavailable"))?;let value=monotonic+state.process_control.uptime()*1_000_000_000.0;drop(state);let key=c.string_rooted(&milestone("bootstrapComplete").to_string());let value=c.number(value);c.set_property_rooted(milestones,key,value,milestones)?;Ok(c.undefined())}
pub(crate)fn cleanup(runtime:&mut Runtime<NodeHost>){let shared=runtime.host_mut().shared_state();let state=std::mem::take(&mut shared.borrow_mut().performance_native);for root in[state.counts,state.milestones,state.callback].into_iter().flatten(){runtime.release_root(root);}}
