const trace = [];
function closureReadWrite() {
  let value = 1;
  const read = () => value;
  const write = next => (value = next);
  write(2);
  const before = read();
  write(3);
  return [before, read()];
}
function nestedDepth() {
  let value = 'depth-two';
  return () => () => () => value;
}
function blockClones() {
  const reads = [];
  for (let i = 0; i < 4; i++) reads.push(() => i);
  return reads.map(read => read());
}
function directEvalMutation() {
  let value = 'before';
  const read = () => value;
  eval("value = 'after'");
  return read();
}
function tdzAndLiveCapture() {
  const read = () => value;
  let errorName = '';
  try { read(); } catch (error) { errorName = error.name; }
  let value = 'initialized';
  return [errorName, read()];
}
let scriptLexical = 'script-a';
function readScriptLexical() { return scriptLexical; }
scriptLexical = 'script-b';
trace.push(closureReadWrite());
trace.push(nestedDepth()()()());
trace.push(blockClones());
trace.push(directEvalMutation());
trace.push(tdzAndLiveCapture());
trace.push(readScriptLexical());
const result = 'CAPTURE_ENV_ORACLE:' + JSON.stringify(trace);
if (typeof print === 'function') print(result);
else console.log(result);
