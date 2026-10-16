//! Native option queries project the same startup state used by feature gates.
use crate::host::NodeHost;
use quench_runtime::{NativeContext,RootId,RootedError};
const CACHE_KEY: &str="\0native:options";
pub(crate) fn binding(context: &mut NativeContext<'_,NodeHost>) -> Result<RootId,RootedError> {
    let shared=context.host_mut().shared_state();
    if let Some(root)=shared.borrow().builtin_cache.get(CACHE_KEY).copied() { return Ok(root); }
    let module=context.object_rooted()?;
    for (name,operation) in [("getCLIOptionsValues","cliOptionsValues"),("parseNodeOptionsEnvVar","cliParseNodeOptions")]{
        let function=context.host_function_named(crate::host::shared_vm::operation(operation),name)?;
        let key=context.string_rooted(name);context.define_data_property_rooted(module,key,function,true,true,true)?;
    }
    let root=context.retain(module)?;shared.borrow_mut().builtin_cache.insert(CACHE_KEY.into(),root);Ok(module)
}
pub(crate) fn values(context: &mut NativeContext<'_,NodeHost>,_:RootId,_:&[RootId])->Result<RootId,RootedError>{
    let options=context.host_mut().shared_state().borrow().cli_options.clone();
    let result=context.null_object_rooted()?;
    for (name,enabled) in options.values(){
        for (name,enabled) in [(name.to_owned(),enabled),(format!("--no-{}",name.strip_prefix("--").expect("long option")),!enabled)]{
            let key=context.string_rooted(&name);let value=context.boolean(enabled);context.define_data_property_rooted(result,key,value,true,true,true)?;
        }
    }
    for (name,value) in options.strings(){
        let key=context.string_rooted(name);let value=context.string_rooted(value);context.define_data_property_rooted(result,key,value,true,true,true)?;
    }
    for (name,values) in options.lists(){
        let key=context.string_rooted(name);let values=values.iter().map(|value|context.string_rooted(value)).collect::<Vec<_>>();let values=context.array_rooted(&values)?;context.define_data_property_rooted(result,key,values,true,true,true)?;
    }
    Ok(result)
}
pub(crate) fn parse_environment(context: &mut NativeContext<'_,NodeHost>,_:RootId,args:&[RootId])->Result<RootId,RootedError>{
    let value=args.first().copied().unwrap_or_else(||context.undefined());let text=context.to_string(value)?;
    match super::cli_options::parse_node_options(&text){
        Ok(arguments)=>{let arguments=arguments.iter().map(|arg|context.string_rooted(arg)).collect::<Vec<_>>();context.array_rooted(&arguments)}
        Err(message)=>{let error=context.error_rooted(message)?;Err(context.throw(error))}
    }
}
