function make(feedback) {
    return {
        run(n, x) {
            n |= 0; x |= 0;
            for (let i = 0; i < n; i = (i + 1) | 0) x = (x >> 1) ^ feedback;
            return x | 0;
        },
        set(value) { feedback = value; }
    };
}
const pipe = make(0xD008);
for (let warm = 0; warm < 64; warm++) pipe.run(2, warm);
for (const value of [0, -1, 1.5, 4294967295, '7', null, undefined]) {
    pipe.set(value); print(pipe.run(17, 123456789));
}
let conversions = 0;
pipe.set({valueOf() { conversions++; pipe.set(3); return 7; }});
print(pipe.run(3, 123)); print(conversions);
pipe.set(1n);
print(pipe.run(0, 123));
try { pipe.run(1, 123); } catch (error) { print(error instanceof TypeError); }
{
    function before(n) {
        let x = 0;
        for (let i = 0; i < n; i = (i + 1) | 0) x ^= later;
        return x | 0;
    }
    print(before(0));
    try { before(1); } catch (error) { print(error instanceof ReferenceError); }
    const later = 7;
    print(before(1));
}
let changing = 1;
function update() { changing = (changing + 1) | 0; }
function calls(n) {
    let x = 0;
    for (let i = 0; i < n; i = (i + 1) | 0) { x ^= changing; update(); }
    return x | 0;
}
function stores(n) {
    let x = 0;
    for (let i = 0; i < n; i = (i + 1) | 0) { x ^= changing; changing = i; }
    return x | 0;
}
print(calls(17)); print(stores(17));
