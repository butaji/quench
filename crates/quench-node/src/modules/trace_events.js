(function (deps) {
  'use strict';

  var active = [];

  function add(category) {
    if (active.indexOf(category) < 0) active.push(category);
  }
  function remove(category) {
    var i = active.indexOf(category);
    if (i >= 0) active.splice(i, 1);
  }
  function categoriesString() {
    var out = [];
    for (var i = 0; i < active.length; i++) {
      if (active[i] === '*') continue;
      out.push(active[i]);
    }
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
}(deps));
