// Runts Client Runtime v0.1.0
// Built from: runtime.ts
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
    debug: window.__RUNTS_CONFIG__?.debug ?? false,
    defaultMode: window.__RUNTS_CONFIG__?.defaultMode ?? 'visible',
    bundlesPath: window.__RUNTS_CONFIG__?.bundlesPath ?? '/_runts/islands',
    version: window.__RUNTS_CONFIG__?.version ?? '0.5.0',
  };

  const log = (...args) => {
    if (CONFIG.debug) {
      console.log('[runts]', ...args);
    }
  };

  // ============================================================
  // Signal System (Preact Signals Compatible)
  // ============================================================

  class Signal {
    #value;
    #subscribers = new Set();
    #effectCleanup = null;

    constructor(value) {
      this.#value = value;
    }

    get value() {
      // Track dependency
      if (Signal.currentComputation) {
        this.#subscribers.add(Signal.currentComputation);
        Signal.currentComputation.dependencies.add(this);
      }
      return this.#value;
    }

    set value(newValue) {
      this.#setValue(newValue);
    }

    #setValue(newValue, force = false) {
      const oldValue = this.#value;
      this.#value = newValue;

      if (force || !Object.is(oldValue, newValue)) {
        // Notify all subscribers
        const toNotify = new Set(this.#subscribers);
        toNotify.forEach(effect => {
          effect.invalidate();
        });
      }
    }

    peek() {
      return this.#value;
    }

    update(fn) {
      this.#setValue(fn(this.#value));
    }

    // For internal use
    _addSubscriber(computation) {
      this.#subscribers.add(computation);
    }

    _removeSubscriber(computation) {
      this.#subscribers.delete(computation);
    }
  }

  class ComputedSignal extends Signal {
    #computeFn;
    #dependencies = new Set();
    #dirty = true;
    #cachedValue;
    #computation;

    constructor(fn) {
      super(undefined);
      this.#computeFn = fn;
      this.#run();
    }

    #run() {
      const prev = Signal.currentComputation;
      Signal.currentComputation = this.#conputation = {
        fn: this.#computeFn,
        dependencies: new Set(),
        dirty: false,
        invalidate: () => this.#invalidate(),
      };

      try {
        this.#cachedValue = this.#computeFn();
      } finally {
        Signal.currentComputation = prev;
        // Clean up old dependencies
        this.#dependencies.forEach(dep => {
          dep._removeSubscriber(this.#computation);
        });
        // Add new dependencies
        this.#conputation.dependencies.forEach(dep => {
          dep._addSubscriber(this.#computation);
          this.#dependencies.add(dep);
        });
        this.#dirty = false;
      }
    }

    #invalidate() {
      if (!this.#dirty) {
        this.#dirty = true;
        // Propagate to subscribers
        this.#subscribers.forEach(effect => effect.invalidate());
      }
    }

    get value() {
      if (this.#dirty) {
        this.#run();
      }
      if (Signal.currentComputation) {
        Signal.currentComputation.dependencies.add(this);
      }
      return this.#cachedValue;
    }
  }

  Signal.currentComputation = null;

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
    const prev = Signal.currentComputation;
    Signal.currentComputation = null;
    try {
      return fn();
    } finally {
      Signal.currentComputation = prev;
    }
  }

  // ============================================================
  // Effects
  // ============================================================

  function effect(fn) {
    const effect_ = {
      fn,
      dependencies: new Set(),
      cleanup: null,
      dirty: true,
      invalidate() {
        if (!this.dirty) {
          this.dirty = true;
          if (batchDepth > 0) {
            batchedEffects.add(this.#run.bind(this));
          } else {
            queueMicrotask(this.#run.bind(this));
          }
        }
      },
      #run() {
        if (this.cleanup) {
          try {
            this.cleanup();
          } catch (e) {
            console.error('[runts] Effect cleanup error:', e);
          }
        }
        // Clean up old dependencies
        this.dependencies.forEach(dep => {
          dep._removeSubscriber(effect_);
        });
        this.dependencies.clear();

        const prev = Signal.currentComputation;
        Signal.currentComputation = this;
        try {
          this.cleanup = this.fn() ?? null;
        } catch (e) {
          console.error('[runts] Effect error:', e);
        } finally {
          Signal.currentComputation = prev;
          this.dirty = false;
        }
      }
    };

    effect_#run();
    return () => {
      if (effect_.cleanup) {
        effect_.cleanup();
      }
      effect_.dependencies.forEach(dep => {
        dep._removeSubscriber(effect_);
      });
      effect_.dependencies.clear();
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
  function registerIsland(name, component, options = {}) {
    ISLANDS.set(name, {
      component,
      options: {
        hydration: options.hydration ?? CONFIG.defaultMode,
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
      console.error(`[runts] Unknown island: ${name}`);
      return null;
    }

    const id = element.dataset.id ?? `island-${++islandIdCounter}`;
    const hydration = element.dataset.hydration ?? islandDef.options.hydration;

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
      // Call the component with props
      const vnode = island.component(island.props);
      
      // Render to DOM
      renderVNode(vnode, island.element);
      
      island.hydrated = true;
      log('Hydrated:', island.id);
    } catch (e) {
      console.error(`[runts] Hydration error for ${island.name}:`, e);
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

    // DOM element
    const element = vnode.type.startsWith('$')
      ? document.createElement(vnode.type.slice(1))
      : document.createElement(vnode.type);

    // Apply attributes
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

    // Render children
    const children = vnode.props?.children;
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
            if (entries[0].isIntersecting) {
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
        if ('requestIdleCallback' in window) {
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
      // Don't auto-hydrate, wait for explicit trigger
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
      const propsJson = element.dataset.props ?? '{}';
      const props = JSON.parse(propsJson);

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
    
    // Group by hydration strategy
    const byStrategy = {
      eager: [],
      visible: [],
      idle: [],
      manual: [],
    };

    islands.forEach(island => {
      byStrategy[island.hydration]?.push(island) ?? byStrategy.visible.push(island);
    });

    // Eager islands first
    await Promise.all(byStrategy.eager.map(hydrationStrategies.eager));

    // Visible islands with shared observer
    if (byStrategy.visible.length > 0) {
      const visibleObserver = new IntersectionObserver(
        async (entries) => {
          for (const entry of entries) {
            if (entry.isIntersecting) {
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

    // Manual islands are set up but not hydrated
    log('Bootstrap complete');
  }

  // ============================================================
  // Global API
  // ============================================================

  const RuntsClient = {
    // Version
    version: CONFIG.version,

    // Core APIs
    signal,
    computed,
    effect,
    batch,
    untrack,

    // Island APIs
    registerIsland,
    hydrateIsland,

    // Internal APIs
    _discoverIslands: discoverIslands,
    _bootstrapIslands: bootstrapIslands,

    // Config
    CONFIG,

    // Debug
    ISLANDS,
  };

  // Expose globally
  global.Runts = RuntsClient;

  // Auto-bootstrap on DOMContentLoaded
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', bootstrapIslands);
  } else {
    bootstrapIslands();
  }

})(typeof window !== 'undefined' ? window : global);

