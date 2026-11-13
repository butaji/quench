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
function* resumable() { var got = []; try { got.push(yield 'a'); got.push(yield 'b'); } finally { got.push('cleanup'); log.push(got.join()); } return got.length; }
function* relay() { try { yield* resumable(); } catch (e) { log.push('relay ' + e); return 'relayed'; } }
var thrownAtStart = resumable();
var startError = 'none';
try { thrownAtStart.throw('early'); } catch (e) { startError = e; }
var running = resumable();
running.next();
var viaReturn = [running.return('x'), running.next()].map(function (r) { return r.value + ':' + r.done; }).join();
var relayed = relay();
relayed.next();
var viaThrow = [relayed.throw('t'), relayed.next()].map(function (r) { return r.value + ':' + r.done; }).join();
var selfResume = (function* me() { try { holder.next(); } catch (e) { yield e instanceof TypeError; } })();
var holder = selfResume;
var reentrant = selfResume.next().value;
var digest = [sum, steps.join(), returned.value, returned.done, delegated.join(), log.join('|'),
    startError, viaReturn, viaThrow, reentrant].join(';');
async function leaf(x) { await null; return x + 1; }
async function chain(n) {
    var total = 0;
    for (var i = 0; i < n; i++) total += await leaf(i);
    try { await Promise.reject(new Error('r')); } catch (e) { total += e.message.length; }
    return total;
}
async function failing(n) { await null; if (n % 2) throw new Error('odd ' + n); return n; }
async function rejections(n) {
    var seen = [];
    for (var i = 0; i < n; i++) {
        try { seen.push(await failing(i)); } catch (e) { seen.push(e.message); } finally { await null; }
    }
    return seen.join();
}
async function* ticks(n) { for (var i = 0; i < n; i++) yield await leaf(i); }
async function collect() { var out = []; for await (var t of ticks(4)) out.push(t); return out.join(); }
Promise.all([chain(50), rejections(6), collect(), failing(3).catch(function (e) { return e.message; })])
    .then(function (parts) { throw digest + ';' + parts.join('/'); });
if (typeof Promise === 'undefined') throw digest;
