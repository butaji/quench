const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}
function invoke(){return this()}
out.push(invoke.call(Capture));
class Derived extends Capture{constructor(){super()}}
out.push(new Derived());
console.log(JSON.stringify(out));
