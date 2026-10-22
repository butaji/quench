Error.prepareStackTrace=(_,frames)=>frames[0];
function Original(){return new Error().stack}
const out=[];
for(const object of [{Original},{renamed:Original},{a:Original,b:Original},Object.create({inherited:Original}),Object.defineProperty({},'hidden',{value:Original}),{0:Original}]){
 const key=Reflect.ownKeys(object)[0]??'inherited';const site=object[key]();out.push([key,site.getFunctionName(),site.getMethodName()]);
}
const strict={go:function Named(){'use strict';return new Error().stack}};out.push(strict.go().getMethodName());
const object={get value(){return new Error().stack}};out.push(object.value.getMethodName());
console.log(JSON.stringify(out));
