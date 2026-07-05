// Closure capturing variable
// ECMA-262 sec-function-environment-records

function makeAdder(x) {
    return function(y) {
        return x + y;
    };
}
let add5 = makeAdder(5);
add5(10);
