const positions=[];Error.prepareStackTrace=(_,frames)=>frames.find(site=>site.getFunctionName()==='read').getColumnNumber();
const leaf={get b(){positions.push(new Error().stack);return 1}};
class Holder{get a(){positions.push(new Error().stack);return leaf}read(){return this.a.b}}
new Holder().read();console.log(JSON.stringify(positions));
