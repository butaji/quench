const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}function factory(){return Capture}
out.push(factory()());
out.push(function(){return Capture()}());
out.push((function(){return Capture()})());
out.push((0,Capture)());
out.push((()=>Capture())());
console.log(JSON.stringify(out));
