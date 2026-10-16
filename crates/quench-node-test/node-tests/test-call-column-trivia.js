const out=[];Error.prepareStackTrace=(_,frames)=>frames.slice(0,2).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}const object={go:Capture};
out.push(object['go'] /* ( comment ) */ ());
out.push((object.go) /* ( comment ) */ ());
out.push(object.go?. /* ( comment ) */ ());
out.push(Capture /* ( comment ) */ ());
out.push(object.go /* ( comment ) */ ());
out.push(object['go']
());
console.log(JSON.stringify(out));
