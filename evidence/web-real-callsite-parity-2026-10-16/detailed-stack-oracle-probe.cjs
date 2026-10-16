const util=require('util'),out=[];
function Capture(){return util.getCallSites(3)}
Object.defineProperty(Capture,'name',{value:'Changed'});
for(const sites of [Capture(),[1].map(Capture)[0]])out.push(sites.map(s=>({keys:Reflect.ownKeys(s),nullPrototype:Object.getPrototypeOf(s)===null,functionName:s.functionName,scriptId:s.scriptId,scriptName:s.scriptName,line:s.lineNumber,column:s.columnNumber,alias:s.column})));
Error.stackTraceLimit=0;Error.prepareStackTrace=()=>{throw Error('hook')};out.push(Capture().map(s=>s.functionName));
console.log(JSON.stringify(out));
