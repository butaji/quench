/**
 * Runts Client-Side Runtime
 *
 * Provides selective hydration for islands using Preact Signals.
 * Zero dependencies - vanilla JS implementation.
 *
 * @version 1.0.0
 */

(function(global) {
  'use strict';

  // ============================================================
  // Configuration
  // ============================================================

  const CONFIG = {
    debug: (global.__RUNTS_CONFIG__ && global.__RUNTS_CONFIG__.debug) || false,
    defaultMode: (global.__RUNTS_CONFIG__ && global.__RUNTS_CONFIG__.defaultMode) || 'visible',
    bundlesPath: (global.__RUNTS_CONFIG__ && global.__RUNTS_CONFIG__.bundlesPath) || '/_runts/islands',
    version: (global.__RUNTS_CONFIG__ && global.__RUNTS_CONFIG__.version) || '0.5.0',
  };

  const log = (...args) => {
    if (CONFIG.debug) {
      console.log('[runts]', ...args);
    }
  };

  // ============================================================
  // Signal System (Preact Signals Compatible)
  // ============================================================

  let currentComputation = null;

  class Signal {
    constructor(value) {
      this._value = value;
      this._subscribers = new Set();
    }

    get value() {
      if (currentComputation) {
        this._subscribers.add(currentComputation);
        currentComputation._dependencies.add(this);
      }
      return this._value;
    }

    set value(newValue) {
      this._setValue(newValue);
    }

    _setValue(newValue, force = false) {
      const oldValue = this._value;
      this._value = newValue;

      if (force || !Object.is(oldValue, newValue)) {
        const toNotify = new Set(this._subscribers);
        toNotify.forEach(effect => {
          effect._invalidate();
        });
      }
    }

    peek() {
      return this._value;
    }

    update(fn) {
      this._setValue(fn(this._value));
    }

    _addSubscriber(computation) {
      this._subscribers.add(computation);
    }

    _removeSubscriber(computation) {
      this._subscribers.delete(computation);
    }
  }

  class ComputedSignal extends Signal {
    constructor(fn) {
      super(undefined);
      this._computeFn = fn;
      this._dependencies = new Set();
      this._dirty = true;
      this._cachedValue = undefined;
      this._computation = null;
      this._run();
    }

    _run() {
      const prev = currentComputation;
      this._computation = {
        fn: this._computeFn,
        _dependencies: new Set(),
        _dirty: false,
        _invalidate: () => this._invalidate(),
      };
      currentComputation = this._computation;

      try {
        this._cachedValue = this._computeFn();
      } finally {
        currentComputation = prev;
        // Clean up old dependencies
        this._dependencies.forEach(dep => {
          dep._removeSubscriber(this._computation);
        });
        // Add new dependencies
        this._computation._dependencies.forEach(dep => {
          dep._addSubscriber(this._computation);
          this._dependencies.add(dep);
        });
        this._dirty = false;
      }
    }

    _invalidate() {
      if (!this._dirty) {
        this._dirty = true;
        this._subscribers.forEach(effect => effect._invalidate());
      }
    }

    get value() {
      if (this._dirty) {
        this._run();
      }
      if (currentComputation) {
        currentComputation._dependencies.add(this);
      }
      return this._cachedValue;
    }
  }

  function signal(initialValue) {
    return new Signal(initialValue);
  }

  function computed(fn) {
    return new ComputedSignal(fn);
  }

  // ============================================================
  // Batch Updates
  // ============================================================

  let batchDepth = 0;
  const batchedEffects = new Set();

  function batch(fn) {
    batchDepth++;
    try {
      return fn();
    } finally {
      batchDepth--;
      if (batchDepth === 0) {
        batchedEffects.forEach(effect => effect());
        batchedEffects.clear();
      }
    }
  }

  function untrack(fn) {
    const prev = currentComputation;
    currentComputation = null;
    try {
      return fn();
    } finally {
      currentComputation = prev;
    }
  }

  // ============================================================
  // Effects
  // ============================================================

  function effect(fn) {
    const effectObj = {
      fn,
      _dependencies: new Set(),
      cleanup: null,
      _dirty: true,
      _invalidate() {
        if (!this._dirty) {
          this._dirty = true;
          if (batchDepth > 0) {
            batchedEffects.add(this._run.bind(this));
          } else {
            queueMicrotask(this._run.bind(this));
          }
        }
      },
      _run() {
        if (this.cleanup) {
          try {
            this.cleanup();
          } catch (e) {
            console.error('[runts] Effect cleanup error:', e);
          }
        }
        // Clean up old dependencies
        this._dependencies.forEach(dep => {
          dep._removeSubscriber(effectObj);
        });
        this._dependencies.clear();

        const prev = currentComputation;
        currentComputation = this;
        try {
          this.cleanup = this.fn() || null;
        } catch (e) {
          console.error('[runts] Effect error:', e);
        } finally {
          currentComputation = prev;
          this._dirty = false;
        }
      }
    };

    effectObj._run();
    return () => {
      if (effectObj.cleanup) {
        effectObj.cleanup();
      }
      effectObj._dependencies.forEach(dep => {
        dep._removeSubscriber(effectObj);
      });
      effectObj._dependencies.clear();
    };
  }

  // ============================================================
  // Island Hydration
  // ============================================================

  const ISLANDS = new Map();
  let islandIdCounter = 0;

  /**
   * Register an island component
   */
  function registerIsland(name, component, options) {
    options = options || {};
    ISLANDS.set(name, {
      component,
      options: {
        hydration: options.hydration || CONFIG.defaultMode,
        ...options,
      },
    });
    log('Registered island:', name);
  }

  /**
   * Create an island instance
   */
  function createIsland(element, name, props) {
    const islandDef = ISLANDS.get(name);
    if (!islandDef) {
      console.error('[runts] Unknown island: ' + name);
      return null;
    }

    const id = element.dataset.id || ('island-' + (++islandIdCounter));
    const hydration = element.dataset.hydration || islandDef.options.hydration;

    return {
      id,
      name,
      element,
      props,
      hydration,
      hydrated: false,
      component: islandDef.component,
    };
  }

  /**
   * Hydrate an island
   */
  async function hydrateIsland(island) {
    if (island.hydrated) {
      log('Already hydrated:', island.id);
      return;
    }

    log('Hydrating island:', island.id, island.name);

    try {
      const vnode = island.component(island.props);
      renderVNode(vnode, island.element);
      island.hydrated = true;
      log('Hydrated:', island.id);
    } catch (e) {
      console.error('[runts] Hydration error for ' + island.name + ':', e);
    }
  }

  /**
   * Simple VNode renderer
   */
  function renderVNode(vnode, container) {
    if (typeof vnode === 'string' || typeof vnode === 'number') {
      container.textContent = String(vnode);
      return;
    }

    if (vnode === null || vnode === undefined) {
      container.innerHTML = '';
      return;
    }

    if (Array.isArray(vnode)) {
      container.innerHTML = '';
      vnode.forEach(child => renderVNode(child, container));
      return;
    }

    if (vnode.type === 'TEXT') {
      container.textContent = vnode.props.nodeValue;
      return;
    }

    const element = vnode.type && vnode.type.startsWith('$')
      ? document.createElement(vnode.type.slice(1))
      : document.createElement(vnode.type || 'div');

    for (const [key, value] of Object.entries(vnode.props || {})) {
      if (key === 'children') continue;

      if (key.startsWith('on') && key.length > 2) {
        const eventName = key.slice(2).toLowerCase();
        element.addEventListener(eventName, value);
      } else if (key === 'className') {
        element.className = value;
      } else if (key === 'style' && typeof value === 'object') {
        Object.assign(element.style, value);
      } else if (key.startsWith('data-')) {
        element.dataset[key.slice(5)] = value;
      } else {
        element.setAttribute(key, value);
      }
    }

    const children = vnode.props && vnode.props.children;
    if (children) {
      if (Array.isArray(children)) {
        children.forEach(child => renderVNode(child, element));
      } else if (typeof children === 'object') {
        renderVNode(children, element);
      } else {
        element.textContent = String(children);
      }
    }

    container.innerHTML = '';
    container.appendChild(element);
  }

  // ============================================================
  // Hydration Strategies
  // ============================================================

  const hydrationStrategies = {
    eager: async (island) => {
      await hydrateIsland(island);
    },

    visible: async (island) => {
      return new Promise((resolve) => {
        const observer = new IntersectionObserver(
          async (entries) => {
            if (entries[0] && entries[0].isIntersecting) {
              observer.disconnect();
              await hydrateIsland(island);
              resolve();
            }
          },
          { threshold: 0.1 }
        );
        observer.observe(island.element);
      });
    },

    idle: async (island) => {
      return new Promise((resolve) => {
        if (typeof requestIdleCallback !== 'undefined') {
          requestIdleCallback(async () => {
            await hydrateIsland(island);
            resolve();
          });
        } else {
          setTimeout(async () => {
            await hydrateIsland(island);
            resolve();
          }, 1);
        }
      });
    },

    manual: async (island) => {
      log('Manual hydration for:', island.id);
    },
  };

  // ============================================================
  // Island Discovery & Bootstrapping
  // ============================================================

  function discoverIslands() {
    const elements = document.querySelectorAll('[data-island]');
    const islands = [];

    elements.forEach(element => {
      const name = element.dataset.island;
      const propsJson = element.dataset.props || '{}';
      let props = {};
      try {
        props = JSON.parse(propsJson);
      } catch (e) {
        console.error('[runts] Failed to parse props for island', name, e);
      }

      const island = createIsland(element, name, props);
      if (island) {
        islands.push(island);
      }
    });

    log('Discovered islands:', islands.length);
    return islands;
  }

  async function bootstrapIslands() {
    const islands = discoverIslands();

    const byStrategy = {
      eager: [],
      visible: [],
      idle: [],
      manual: [],
    };

    islands.forEach(island => {
      const strategy = byStrategy[island.hydration];
      if (strategy) {
        strategy.push(island);
      } else {
        byStrategy.visible.push(island);
      }
    });

    // Eager islands first
    await Promise.all(byStrategy.eager.map(hydrationStrategies.eager));

    // Visible islands with shared observer
    if (byStrategy.visible.length > 0) {
      const visibleObserver = new IntersectionObserver(
        async (entries) => {
          for (const entry of entries) {
            if (entry.isIntersecting && entry.target._runtsIsland) {
              const island = entry.target._runtsIsland;
              visibleObserver.unobserve(entry.target);
              await hydrationStrategies.visible(island);
            }
          }
        },
        { threshold: 0.1 }
      );

      byStrategy.visible.forEach(island => {
        island.element._runtsIsland = island;
        visibleObserver.observe(island.element);
      });
    }

    // Idle islands
    byStrategy.idle.forEach(island => {
      hydrationStrategies.idle(island);
    });

    log('Bootstrap complete');
  }

  // ============================================================
  // Global API
  // ============================================================

  const RuntsClient = {
    version: CONFIG.version,
    signal,
    computed,
    effect,
    batch,
    untrack,
    registerIsland,
    hydrateIsland,
    _discoverIslands: discoverIslands,
    _bootstrapIslands: bootstrapIslands,
    CONFIG,
    ISLANDS,
  };

  // Expose globally
  global.Runts = RuntsClient;

  // Auto-bootstrap after all resources (including island bundles) have loaded
  if (typeof window !== 'undefined') {
    if (document.readyState === 'complete') {
      bootstrapIslands();
    } else {
      window.addEventListener('load', bootstrapIslands);
    }
  }

})(typeof window !== 'undefined' ? window : (typeof global !== 'undefined' ? global : this));
