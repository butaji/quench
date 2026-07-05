// Break and continue in loops
// ECMA-262 sec-break-statement, sec-continue-statement

let result = 0;
for (let i = 0; i < 10; i = i + 1) {
    if (i === 5) {
        continue;
    }
    if (i === 8) {
        break;
    }
    result = result + i;
}
result;
