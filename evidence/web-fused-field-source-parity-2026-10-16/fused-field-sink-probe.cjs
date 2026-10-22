const out=[];Error.prepareStackTrace=(_,frames)=>frames.filter(f=>f.getFileName()===__filename).map(f=>[f.getLineNumber(),f.getColumnNumber()]);function Capture(){return new Error().stack}
const leaf={get b(){out.push(['b',Capture()]);return 1}};
class Holder{get a(){out.push(['a',Capture()]);return leaf}set c(value){out.push(['c',Capture()])}write(){this.c=this.a.b}}
new Holder().write();
console.log(JSON.stringify(out));
