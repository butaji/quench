Error.prepareStackTrace=(_,frames)=>frames;
function outer(){return eval('eval("new Error().stack")')}
console.log(outer()[0].getEvalOrigin());
const fn=Function('return eval("new Error().stack")');
console.log(fn()[0].getEvalOrigin());
