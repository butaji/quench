const assert=require('assert');
const ib=require('internal/test/binding').internalBinding;
const binding=ib('util');
const symbols=binding.privateSymbols;
const expected=[["arrow_message_private_symbol", "node:arrowMessage"], ["contextify_context_private_symbol", "node:contextify:context"], ["decorated_private_symbol", "node:decorated"], ["empty_context_frame_sentinel_symbol", "node:empty_context_frame_sentinel"], ["ffi_pointer_address_private_symbol", "node:ffi:pointer_address"], ["transfer_mode_private_symbol", "node:transfer_mode"], ["host_defined_option_symbol", "node:host_defined_option_symbol"], ["js_transferable_wrapper_private_symbol", "node:js_transferable_wrapper"], ["entry_point_module_private_symbol", "node:entry_point_module"], ["entry_point_promise_private_symbol", "node:entry_point_promise"], ["module_source_private_symbol", "node:module_source"], ["module_export_names_private_symbol", "node:module_export_names"], ["module_circular_visited_private_symbol", "node:module_circular_visited"], ["module_export_private_symbol", "node:module_export"], ["module_first_parent_private_symbol", "node:module_first_parent"], ["module_last_parent_private_symbol", "node:module_last_parent"], ["napi_type_tag", "node:napi:type_tag"], ["napi_wrapper", "node:napi:wrapper"], ["untransferable_object_private_symbol", "node:untransferableObject"], ["exit_info_private_symbol", "node:exit_info_private_symbol"], ["promise_trace_id", "node:promise_trace_id"], ["source_map_data_private_symbol", "node:source_map_data_private_symbol"]];
assert.deepStrictEqual(Object.keys(symbols),expected.map(row=>row[0]));
const seen=new Set();
for(const [name,description] of expected){
 const key=symbols[name];
 assert.strictEqual(typeof key,'symbol');assert.strictEqual(key.description,description);
 assert.strictEqual(String(key),'Symbol('+description+')');assert.strictEqual(Symbol.keyFor(key),undefined);
 assert.strictEqual(ib('util').privateSymbols[name],key);assert.strictEqual(seen.has(key),false);seen.add(key);
 const object={};object[key]=description;
 assert.strictEqual(object[key],description);
 assert.deepStrictEqual(Reflect.ownKeys(object),[]);
 assert.deepStrictEqual(Object.getOwnPropertySymbols(object),[]);
 assert.deepStrictEqual(Object.getOwnPropertyDescriptor(object,key),{value:description,writable:true,enumerable:false,configurable:true});
 assert.strictEqual(Object.create(object)[key],undefined);
 assert.strictEqual(Object.assign({},object)[key],undefined);
 assert.strictEqual(({...object})[key],undefined);
 let trap=false;const proxy=new Proxy(object,{get(){trap=true;throw Error('private get trap')},set(){trap=true;throw Error('private set trap')}});
 assert.strictEqual(proxy[key],undefined);assert.strictEqual(Reflect.set(proxy,key,1),false);assert.strictEqual(trap,false);
 const frozen=Object.freeze({});assert.strictEqual(Reflect.set(frozen,key,description),true);
 assert.strictEqual(frozen[key],description);assert.strictEqual(Object.isFrozen(frozen),true);
 assert.strictEqual(Reflect.deleteProperty(frozen,key),true);
}
console.log('private-symbols-22-ok');
