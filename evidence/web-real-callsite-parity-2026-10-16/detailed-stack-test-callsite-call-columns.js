const out=[];Error.prepareStackTrace=(_,frames)=>frames.slice(0,2).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}const object={go:Capture};
out.push(object.go());
out.push(object['go']());
out.push(object[`go`]());
out.push((object.go)());
out.push(Reflect.apply(Capture,null,[]));
out.push(new Capture());
console.log(JSON.stringify(out));
