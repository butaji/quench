(function (deps) {
  'use strict';

  var active = Object.create(null);

  function add(category) {
    active[category] = (active[category] || 0) + 1;
  }
  function remove(category) {
    if (!active[category]) return;
    if (--active[category] === 0) delete active[category];
  }
  function categoriesString() {
    var out = [];
    for (var category in active) out.push(category);
    return out.join(',');
  }
  function createTracing(options) {
    if (!options || !Array.isArray(options.categories)) {
      throw new TypeError('The "options.categories" argument must be an instance of Array');
    }
    var list = options.categories.slice();
    for (var i = 0; i < list.length; i++) {
      if (typeof list[i] !== 'string') throw new TypeError('Category must be a string');
    }
    var tracing = {
      categories: list.join(','),
      enabled: false,
      enable: function () {
        if (!tracing.enabled) {
          tracing.enabled = true;
          for (var j = 0; j < list.length; j++) add(list[j]);
        }
      },
      disable: function () {
        if (tracing.enabled) {
          tracing.enabled = false;
          for (var j = 0; j < list.length; j++) remove(list[j]);
        }
      }
    };
    return tracing;
  }

  function getEnabledCategories() { return categoriesString(); }

  return {
    createTracing: createTracing,
    getEnabledCategories: getEnabledCategories,
    Tracing: Object,
    WRITE_METADATA: 1,
    WRITE_EVENTS: 2
  };
});
