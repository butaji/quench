const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}let got;const object={get go(){got=Capture();return 1},set go(value){got=Capture()}};
void object.go;out.push(got);
void object['go'];out.push(got);
void object?.go;out.push(got);
void object?.['go'];out.push(got);
object.go=1;out.push(got);
object['go']=1;out.push(got);
object.go++;out.push(got);
class Holder{get #go(){return Capture()}run(){return this.#go}call(){return this.#go()}}
out.push(new Holder().run());
console.log(JSON.stringify(out));
