const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}let got=[];const object={get go(){got.push(['get',Capture()]);return 1},set go(value){got.push(['set',Capture()])}};
object.go=1;out.push(got);got=[];
object['go']=1;out.push(got);got=[];
object.go++;out.push(got);got=[];
++object.go;out.push(got);got=[];
object.go+=1;out.push(got);got=[];
object['go']+=1;out.push(got);got=[];
object.go||=1;out.push(got);got=[];
console.log(JSON.stringify(out));
