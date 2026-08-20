// Domain API: callbacks execute with the active domain and errors can be routed.
function create() {
  const d = {
    members: [], active: false, disposed: false,
    enter() { if (!d.disposed) { module.exports.active = d; d.active = true; } return d; },
    exit() { if (module.exports.active === d) module.exports.active = null; d.active = false; return d; },
    add(x) { if (!d.disposed && d.members.indexOf(x) < 0) d.members.push(x); return d; },
    remove(x) { const i = d.members.indexOf(x); if (i >= 0) d.members.splice(i, 1); return d; },
    run(fn) { d.enter(); try { return fn(); } catch (e) { if (d._handler) return d._handler(e); throw e; } finally { d.exit(); } },
    bind(fn) { return function() { return d.run(() => fn.apply(this, arguments)); }; },
    intercept(fn) { return d.bind(function() { try { return fn.apply(this, arguments); } catch (e) { if (e && e.domain) delete e.domain; throw e; } }); },
    dispose() { d.disposed = true; d.members.length = 0; d.exit(); return d; },
    on(name, fn) { if (name === 'error') d._handler = fn; return d; },
    addEmitter(x) { return d.add(x); }
  };
  return d;
}
module.exports = { active: null, create };
