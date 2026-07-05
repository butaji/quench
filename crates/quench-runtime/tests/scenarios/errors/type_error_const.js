// TypeError on assignment to const
// ECMA-262 sec-static-semantics-early-errors

(function() {
    const x = 1;
    x = 2;
})();
