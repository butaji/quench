// Suspend and resume: iteration, return(), throw(), yield*, async chains. The program's
// final value is thrown, so every execution mode must end with the same digest.
var log = [];
function* count(n) { for (var i = 0; i < n; i++) log.push(yield i); return 'done'; }
function* guarded() {
    try { yield 1; yield 2; } catch (e) { log.push('caught ' + e); yield 3; } finally { log.push('finally'); }
    return 4;
}
function* outer() { var r = yield* count(2); log.push(r); yield* [7, 8]; }
var sum = 0;
for (var k = 0; k < 200; k++) for (var v of count(5)) sum += v;
var g = guarded();
var steps = [g.next().value, g.throw('boom').value, g.next().value, g.next().done];
var early = guarded();
early.next();
var returned = early.return(9);
var delegated = [...outer()];
var digest = [sum, steps.join(), returned.value, returned.done, delegated.join(), log.join('|')].join(';');
async function leaf(x) { await null; return x + 1; }
async function chain(n) {
    var total = 0;
    for (var i = 0; i < n; i++) total += await leaf(i);
    try { await Promise.reject(new Error('r')); } catch (e) { total += e.message.length; }
    return total;
}
chain(50).then(function (total) { throw digest + ';' + total; });
if (typeof Promise === 'undefined') throw digest;
