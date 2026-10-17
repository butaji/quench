Error.prepareStackTrace=(_,frames)=>frames;
function outer(){return eval('new Error().stack')}
const indirect=(0,eval)('new Error().stack');
for(const frames of [outer(),indirect,Function('return new Error().stack')()]){
 const f=frames[0];console.log(JSON.stringify({name:f.getFunctionName(),method:f.getMethodName(),file:f.getFileName(),origin:f.getEvalOrigin(),callee:typeof f.getFunction(),receiver:f.getThis()===globalThis,type:f.getTypeName(),line:f.getLineNumber(),column:f.getColumnNumber(),text:String(f)}));
}
