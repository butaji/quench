const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}let got;const object={get go(){got=Capture();return Capture}};
out.push(object.go(),got);
out.push(object['go'](),got);
out.push((object.go)(),got);
out.push(object?.go(),got);
out.push(object.go?.(),got);
console.log(JSON.stringify(out));
