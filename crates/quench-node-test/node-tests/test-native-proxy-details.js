'use strict';
const {internalBinding}=require('internal/test/binding');
const {getProxyDetails}=internalBinding('util');
const target={marker:42};let calls=0;
const handler={get(){calls++;throw Error('get trap');},ownKeys(){calls++;throw Error('ownKeys trap');}};
const pair=Proxy.revocable(target,handler);
for(const value of [undefined,null,1,{},target])console.log('ordinary',getProxyDetails(value)===undefined);
for(const option of [true,false,undefined,null,1,'yes',{}]){
 const result=getProxyDetails(pair.proxy,option);
 console.log('option',typeof option,option===true?result[0]===target&&result[1]===handler:result===target);
}
let result=getProxyDetails(pair.proxy);console.log('default',Array.isArray(result),result.length,result[0]===target,result[1]===handler,calls);
pair.revoke();result=getProxyDetails(pair.proxy);console.log('revoked',result[0]===null,result[1]===null,getProxyDetails(pair.proxy,false)===null,calls);

console.log("metadata",getProxyDetails.name,getProxyDetails.length,Function.prototype.toString.call(getProxyDetails),JSON.stringify(Object.getOwnPropertyDescriptor(getProxyDetails,"name")),Object.hasOwn(getProxyDetails,"prototype"));
