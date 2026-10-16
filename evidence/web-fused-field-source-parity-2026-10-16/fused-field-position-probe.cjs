const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}
const leaf={get b(){out.push(['b',Capture()]);return 1}};
class Holder{get a(){out.push(['a',Capture()]);return leaf}read(){return this.a.b}}
new Holder().read();
console.log(JSON.stringify(out));
