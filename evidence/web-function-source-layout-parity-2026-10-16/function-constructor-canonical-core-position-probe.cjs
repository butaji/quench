
Error.prepareStackTrace=(_,frames)=>frames;
const fn=Function(' \n return new Error().stack[0];\n '),frame=fn();
if(frame.getLineNumber()!==4||frame.getColumnNumber()!==9)throw Error('body position');
if(fn.toString()!=='function anonymous(\n) {\n \n return new Error().stack[0];\n \n}')throw Error('body source');
const parameter=Function('p = new Error().stack[0]','return p;')();
console.log(parameter.getLineNumber(),parameter.getColumnNumber());
if(Function('return typeof anonymous')()!=='undefined')throw Error('synthetic self binding');
if(Function('anonymous=17','return anonymous')()!==17)throw Error('parameter shadow');
let threw=false;try{Function('"use strict";return anonymous')()}catch(error){threw=error instanceof ReferenceError}
if(!threw)throw Error('strict synthetic self binding');
