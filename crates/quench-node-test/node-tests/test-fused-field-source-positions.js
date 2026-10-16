const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}
const leaf={get b(){out.push(['b',Capture()]);return 1}};
class Holder{get a(){out.push(['a',Capture()]);return leaf}read(){return this.a.b}}
new Holder().read();
console.log(JSON.stringify(out));

class Writer extends Holder { set c(value){out.push(['c',value,Capture()])} write(){this.c=this.a.b} }
new Writer().write();
class ThrowFirst { get a(){out.push(['throw-a',Capture()]);throw new Error('first')} read(){return this.a.b} }
const throwingLeaf={get b(){out.push(['throw-b',Capture()]);throw new Error('second')}};
class ThrowSecond { get a(){return throwingLeaf} read(){return this.a.b} }
for(const holder of [new ThrowFirst(),new ThrowSecond()]) {
  try { holder.read() } catch(error) { out.push([error.message,error.stack]) }
  out.push(['after-throw',Capture()]);
  new Holder().read();
}
console.log(JSON.stringify(out));
