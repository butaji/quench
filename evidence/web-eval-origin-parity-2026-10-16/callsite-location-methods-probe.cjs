Error.prepareStackTrace=(_,frames)=>frames;
function capture(){ return new Error().stack }
function ordinary(){ return capture() }
const object={method(){return capture()}};
const arrow=()=>capture();
for(const frames of [ordinary(),object.method(),arrow(),eval('capture()')]){
 console.log(JSON.stringify(frames.slice(0,3).map(f=>Object.fromEntries(['getFunctionName','getPosition','getEnclosingLineNumber','getEnclosingColumnNumber','getScriptNameOrSourceURL','getScriptHash','isAsync','isPromiseAll','getPromiseIndex','isToplevel','isEval','getEvalOrigin'].map(k=>[k,f[k]()]))) ));
}
