// Domain API: callbacks execute with the active domain and errors can be routed.
var stack = [];
function create() {
  const d = {
    members: [], active: false, disposed: false,
    enter() {
      if (!d.disposed) {
        var index = stack.indexOf(d);
        if (index >= 0) stack.splice(index, 1);
        stack.push(d);
        module.exports.active = d;
        d.active = true;
      }
      return d;
    },
    exit() {
      var index = stack.indexOf(d);
      if (index >= 0) stack.splice(index);
      d.active = false;
      module.exports.active = stack.length ? stack[stack.length - 1] : null;
      return d;
    },
    add(x) { if (!d.disposed && d.members.indexOf(x) < 0) { d.members.push(x); if (x && (typeof x === 'object' || typeof x === 'function')) Object.defineProperty(x, 'domain', { configurable: true, enumerable: false, value: d, writable: true }); } return d; },
    remove(x) { const i = d.members.indexOf(x); if (i >= 0) d.members.splice(i, 1); if (x && x.domain === d) delete x.domain; return d; },
    run(fn) { d.enter(); try { return fn(); } catch (e) { if (d._handler) return d._handler(e); throw e; } finally { d.exit(); } },
    bind(fn) { return function() { return d.run(() => fn.apply(this, arguments)); }; },
    intercept(fn) { return d.bind(function() { try { return fn.apply(this, arguments); } catch (e) { if (e && e.domain) delete e.domain; throw e; } }); },
    dispose() { d.members.forEach((member) => { if (member && member.domain === d) delete member.domain; }); d.disposed = true; d.members.length = 0; d.exit(); return d; },
    on(name, fn) { if (name === 'error') d._handler = fn; return d; },
    addEmitter(x) { return d.add(x); }
  };
  return d;
}
function Domain() { return create(); }
module.exports = { active: null, _stack: stack, Domain, create, createDomain: create };
