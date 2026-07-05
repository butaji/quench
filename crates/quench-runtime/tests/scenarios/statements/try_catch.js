// Try/catch/throw with value preservation
// ECMA-262 sec-try-statement-runtime-semantics

(function() {
    try {
        throw "caught";
    } catch (e) {
        return e;
    }
})();
