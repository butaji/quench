// Optional chaining function call returns undefined when base is null
var obj = null;
var result = obj?.fn?.();
result === undefined;
