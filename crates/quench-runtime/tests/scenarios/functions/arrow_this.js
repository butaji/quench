// Arrow function lexical this
// ECMA-262 sec-arrow-function-definitions-runtime-semantics

(function() {
    let obj = {
        method: () => this,
        regular: function() { return this; }
    };
    return obj.method() === obj;
})();
