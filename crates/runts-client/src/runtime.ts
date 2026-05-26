/**
 * runts Client Runtime
 * 
 * Provides client-side hydration and interactivity for islands.
 * This is the minimal JavaScript needed for island hydration.
 */

(function(global) {
    'use strict';

    // ============================================================================
    // Configuration
    // ============================================================================

    const CONFIG = {
        // Hydration strategies
        STRATEGY: {
            EAGER: 'eager',
            VISIBLE: 'visible',
            IDLE: 'idle',
            MANUAL: 'manual',
            STATIC: 'static'
        },
        
        // Debounce delay for visible observer (ms)
        VISIBLE_DEBOUNCE: 100,
        
        // Max idle time before hydrating idle islands (ms)
        IDLE_TIMEOUT: 5000
    };

    // ============================================================================
    // Utility Functions
    // ============================================================================

    /**
     * Safe JSON parse
     */
    function safeJsonParse(str) {
        try {
            return JSON.parse(str);
        } catch (e) {
            console.error('[runts] Failed to parse JSON:', e);
            return null;
        }
    }

    /**
     * Query selector with fallback
     */
    function querySelector(selector) {
        return document.querySelector(selector);
    }

    function querySelectorAll(selector) {
        return Array.from(document.querySelectorAll(selector));
    }

    // ============================================================================
    // Island Registry
    // ============================================================================

    /**
     * Island registry for client-side components
     */
    const Islands = {
        _components: {},
        _instances: {},

        /**
         * Register an island component
         */
        register(name, component) {
            this._components[name] = component;
            console.debug(`[runts] Registered island: ${name}`);
        },

        /**
         * Check if island is registered
         */
        has(name) {
            return name in this._components;
        },

        /**
         * Create island instance
         */
        create(name, props, element) {
            if (!this.has(name)) {
                console.warn(`[runts] Unknown island: ${name}`);
                return null;
            }

            const Component = this._components[name];
            const instance = new Component(props, element);
            this._instances[name] = instance;
            return instance;
        },

        /**
         * Mount all pending islands
         */
        mountAll(manifest) {
            if (!manifest || !manifest.islands) {
                return;
            }

            for (const entry of manifest.islands) {
                const element = querySelector(`[data-island="${entry.name}"][data-id="${entry.id}"]`);
                if (!element) {
                    console.warn(`[runts] Island element not found: ${entry.selector}`);
                    continue;
                }

                const props = entry.props || {};
                const instance = this.create(entry.name, props, element);
                if (instance && entry.strategy !== CONFIG.STRATEGY.STATIC) {
                    instance.mount();
                    element.dataset.hydrated = 'true';
                }
            }
        }
    };

    // ============================================================================
    // Hydration Strategies
    // ============================================================================

    /**
     * Hydration manager
     */
    const Hydrator = {
        _observer: null,

        /**
         * Initialize hydration
         */
        init() {
            this._setupVisibilityObserver();
            this._hydrateEager();
            this._scheduleIdle();
        },

        /**
         * Setup IntersectionObserver for visible strategy
         */
        _setupVisibilityObserver() {
            if (!('IntersectionObserver' in window)) {
                // Fallback: hydrate everything visible
                return;
            }

            this._observer = new IntersectionObserver(
                (entries) => {
                    entries.forEach(entry => {
                        if (entry.isIntersecting) {
                            const island = entry.target.closest('[data-island]');
                            if (island) {
                                this._hydrateIsland(island);
                                this._observer.unobserve(entry.target);
                            }
                        }
                    });
                },
                {
                    rootMargin: '100px',
                    threshold: 0.1
                }
            );
        },

        /**
         * Hydrate all eager islands immediately
         */
        _hydrateEager() {
            const islands = querySelectorAll('[data-island]');
            islands.forEach(island => {
                const strategy = island.dataset.strategy;
                if (strategy === CONFIG.STRATEGY.EAGER) {
                    this._hydrateIsland(island);
                }
            });
        },

        /**
         * Schedule idle hydration
         */
        _scheduleIdle() {
            if (!('requestIdleCallback' in window)) {
                // Fallback: use setTimeout
                setTimeout(() => this._hydrateIdle(), CONFIG.IDLE_TIMEOUT);
                return;
            }

            requestIdleCallback(() => this._hydrateIdle(), {
                timeout: CONFIG.IDLE_TIMEOUT
            });
        },

        /**
         * Hydrate idle islands
         */
        _hydrateIdle() {
            const islands = querySelectorAll('[data-island]');
            islands.forEach(island => {
                const strategy = island.dataset.strategy;
                if (strategy === CONFIG.STRATEGY.IDLE) {
                    this._hydrateIsland(island);
                }
            });
        },

        /**
         * Hydrate visible islands when they enter viewport
         */
        hydrateVisible(island) {
            if (this._observer) {
                this._observer.observe(island);
            } else {
                // Fallback: hydrate immediately
                this._hydrateIsland(island);
            }
        },

        /**
         * Hydrate a single island
         */
        _hydrateIsland(island) {
            if (island.dataset.hydrated === 'true') {
                return;
            }

            const name = island.dataset.island;
            const propsJson = island.dataset.props;
            
            if (!Islands.has(name)) {
                console.warn(`[runts] Island not registered: ${name}`);
                return;
            }

            const props = safeJsonParse(propsJson) || {};
            const instance = Islands.create(name, props, island);
            
            if (instance) {
                instance.mount();
                island.dataset.hydrated = 'true';
                console.debug(`[runts] Hydrated island: ${name}`);
            }
        },

        /**
         * Manually hydrate an island (for manual strategy)
         */
        hydrate(name, selector) {
            const element = querySelector(selector);
            if (element) {
                this._hydrateIsland(element);
            }
        }
    };

    // ============================================================================
    // Island Base Class
    // ============================================================================

    /**
     * Base class for island components
     */
    class Island {
        constructor(props, element) {
            this.props = props || {};
            this.element = element;
            this.contentElement = element.querySelector('[data-island-content]') || element;
            this.hydrated = false;
            this._handlers = new Map();
        }

        mount() {
            this.hydrated = true;
            this.attachEvents();
        }

        unmount() {
            this.hydrated = false;
            this.detachEvents();
        }

        setProps(props) {
            this.props = { ...this.props, ...props };
            this.render();
        }

        render() {
            // Override in subclass
        }

        attachEvents() {
            // Override in subclass
        }

        detachEvents() {
            // Remove all attached event listeners
            for (const [key, handler] of this._handlers) {
                const [event, selector] = key.split(':');
                const el = selector ? this.element.querySelector(selector) : this.element;
                if (el) {
                    el.removeEventListener(event, handler);
                }
            }
            this._handlers.clear();
        }

        /**
         * Attach event handler with automatic cleanup
         */
        on(event, selector, handler) {
            const key = `${event}:${selector || ''}`;
            this._handlers.set(key, handler);
            
            const el = selector ? this.element.querySelector(selector) : this.element;
            if (el) {
                el.addEventListener(event, handler);
            }
        }

        /**
         * Attach one-time event handler
         */
        once(event, selector, handler) {
            const wrappedHandler = (e) => {
                handler(e);
                this.off(event, selector, wrappedHandler);
            };
            this.on(event, selector, wrappedHandler);
        }

        /**
         * Remove event handler
         */
        off(event, selector, handler) {
            const key = `${event}:${selector || ''}`;
            this._handlers.delete(key);
            
            const el = selector ? this.element.querySelector(selector) : this.element;
            if (el) {
                el.removeEventListener(event, handler);
            }
        }
    }

    // ============================================================================
    // Event Delegation
    // ============================================================================

    /**
     * Event delegation for island events
     */
    const Events = {
        _handlers: new Map(),
        _delegated: new Map(),

        /**
         * Attach event handler to island
         */
        on(eventType, selector, handler) {
            const key = `${eventType}:${selector}`;
            
            if (!this._delegated.has(key)) {
                document.addEventListener(eventType, (e) => {
                    const target = e.target.closest(selector);
                    if (target) {
                        const handlers = this._delegated.get(key);
                        handlers.forEach(h => h.call(target, e));
                    }
                });
                this._delegated.set(key, new Set());
            }
            
            this._delegated.get(key).add(handler);
        },

        /**
         * Attach one-time event handler
         */
        once(eventType, selector, handler) {
            const wrappedHandler = (e) => {
                handler(e);
                this.off(eventType, selector, wrappedHandler);
            };
            this.on(eventType, selector, wrappedHandler);
        },

        /**
         * Remove event handler
         */
        off(eventType, selector, handler) {
            const key = `${eventType}:${selector}`;
            const handlers = this._delegated.get(key);
            if (handlers) {
                handlers.delete(handler);
            }
        }
    };

    // ============================================================================
    // Preact-Style Signals (Client-side)
    // ============================================================================

    /**
     * Preact-style signals for client
     */
    const Signals = {
        _subscribers: new Map(),
        _values: new Map(),

        create(initialValue) {
            const id = Math.random().toString(36).substr(2);
            this._values.set(id, initialValue);
            this._subscribers.set(id, new Set());
            return {
                id,
                get value() {
                    return Signals._values.get(id);
                },
                set value(newValue) {
                    Signals._values.set(id, newValue);
                    Signals._subscribers.get(id).forEach(fn => fn(newValue));
                }
            };
        },

        effect(fn, ids) {
            ids.forEach(id => {
                this._subscribers.get(id)?.add(fn);
            });
        },

        batch(fn) {
            fn();
        }
    };

    // ============================================================================
    // HMR (Hot Module Replacement)
    // ============================================================================

    const HMR = {
        _connection: null,
        _retryCount: 0,
        _maxRetries: 5,

        /**
         * Connect to HMR server
         */
        connect() {
            if (this._connection) {
                this._connection.close();
            }

            this._connection = new EventSource('/_runts/hmr');

            this._connection.onopen = () => {
                console.log('[runts HMR] Connected');
                this._retryCount = 0;
            };

            this._connection.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    this._handleEvent(data);
                } catch (e) {
                    console.error('[runts HMR] Failed to parse event:', e);
                }
            };

            this._connection.onerror = () => {
                console.warn('[runts HMR] Connection lost, retrying...');
                this._connection.close();
                this._retryCount++;

                if (this._retryCount < this._maxRetries) {
                    setTimeout(() => this.connect(), 1000 * this._retryCount);
                } else {
                    console.error('[runts HMR] Max retries reached');
                }
            };
        },

        /**
         * Handle HMR event
         */
        _handleEvent(data) {
            switch (data.type) {
                case 'change':
                    this._handleChange(data);
                    break;
                case 'reload':
                    window.location.reload();
                    break;
                case 'error':
                    console.error('[runts HMR] Error:', data.message);
                    this._showError(data.message);
                    break;
            }
        },

        /**
         * Handle file change
         */
        _handleChange(data) {
            console.log(`[runts HMR] File changed: ${data.path}`);
            
            // Dispatch custom event for app to handle
            window.dispatchEvent(new CustomEvent('runts:reload', {
                detail: { path: data.path }
            }));

            // Soft reload: re-fetch page data
            if (data.path.includes('/routes/')) {
                this._softReload();
            } else if (data.path.includes('/islands/')) {
                // For islands, just reload the page to get fresh hydration
                window.location.reload();
            }
        },

        /**
         * Soft reload: fetch new page content
         */
        async _softReload() {
            try {
                const response = await fetch(window.location.pathname);
                const html = await response.text();
                
                // Update island content
                const parser = new DOMParser();
                const doc = parser.parseFromString(html, 'text/html');
                
                querySelectorAll('[data-island]').forEach(island => {
                    const name = island.dataset.island;
                    const newIsland = doc.querySelector(`[data-island="${name}"]`);
                    if (newIsland) {
                        const newContent = newIsland.innerHTML;
                        island.innerHTML = newContent;
                    }
                });

                console.log('[runts HMR] Soft reload complete');
            } catch (e) {
                console.error('[runts HMR] Soft reload failed:', e);
                window.location.reload();
            }
        },

        /**
         * Show error in dev toolbar
         */
        _showError(message) {
            const status = document.getElementById('__runts-status');
            if (status) {
                status.textContent = 'Error';
                status.style.color = '#ff6b6b';
                status.title = message;
            }
        }
    };

    // ============================================================================
    // Initialize
    // ============================================================================

    /**
     * Initialize the client runtime
     */
    function init() {
        // Load island manifest
        const manifestEl = document.getElementById('__island-manifest');
        if (manifestEl) {
            try {
                const manifest = JSON.parse(manifestEl.textContent);
                if (manifest) {
                    // Connect to HMR
                    HMR.connect();
                    
                    // Initialize hydration
                    Hydrator.init();
                    Islands.mountAll(manifest);
                }
            } catch (e) {
                console.error('[runts] Failed to parse island manifest:', e);
            }
        }

        // Also check window.__ISLAND_MANIFEST__
        if (window.__ISLAND_MANIFEST__) {
            Hydrator.init();
            Islands.mountAll(window.__ISLAND_MANIFEST__);
        }

        // Mark as ready
        document.body.classList.add('runts-ready');
        console.log('[runts] Client runtime initialized');
    }

    // Export to global
    global.runts = {
        Islands,
        Signals,
        Events,
        Hydrator,
        HMR,
        CONFIG,
        Island,
        init
    };

    // Also expose common APIs at window level for convenience
    global.__registerIsland__ = (name, Component) => Islands.register(name, Component);
    global.__runts_islands__ = Islands._components;

    // Auto-initialize on DOM ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

})(typeof window !== 'undefined' ? window : this);
