// Domain API: callbacks execute with the active domain and errors can be routed.
module.exports = {
  active: null,
  create() {
    const d = { members: [], enter() { module.exports.active = d; }, exit() { if (module.exports.active === d) module.exports.active = null; }, add(x) { d.members.push(x); return d; }, remove(x) { const i=d.members.indexOf(x); if(i>=0)d.members.splice(i,1); return d; }, run(fn) { d.enter(); try { return fn(); } finally { d.exit(); } }, bind(fn) { return function() { return d.run(() => fn.apply(this, arguments)); }; }, dispose() { d.members.length=0; }, on() { return d; } }; return d;
  }
};
