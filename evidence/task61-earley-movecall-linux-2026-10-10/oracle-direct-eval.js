function directEvalOneArgument() { let value = 1; eval("value += 2"); return value; }
console.log(directEvalOneArgument());
