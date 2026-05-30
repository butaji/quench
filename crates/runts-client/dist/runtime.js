// Runts Client Runtime v0.1.0
/**
 * Runts Client-Side Runtime — Preact Edition
 *
 * Thin hydration layer. Islands are standard Preact components.
 * This script discovers data-island elements and hydrates them
 * using Preact from esm.sh (or any CDN).
 *
 * @version 2.0.0
 */

(function(global) {
  'use strict';

  const log = (...args) => {
    if (global.__RUNTS_CONFIG__?.debug) {
      console.log('[runts]', ...args);
    }
  };

  // ── Import map for Preact ──────────────────────────────────────
  // In dev mode we load from esm.sh; in production the bundler
  // can replace these with local files.
  const PREACT_URL = 'https://esm.sh/preact@10.19.3';
  const PREACT_HOOKS_URL = 'https://esm.sh/preact@10.19.3/hooks';

  // ── Island registry ────────────────────────────────────────────
  const ISLANDS = new Map();

  function registerIsland(name, component) {
    ISLANDS.set(name, component);
    log('Registered island:', name);
  }

  // ── Hydration ──────────────────────────────────────────────────
  async function hydrateIsland(el, name, props, strategy) {
    const Component = ISLANDS.get(name);
    if (!Component) {
      console.error('[runts] Unknown island:', name);
      return;
    }

    const doHydrate = () => {
      try {
        // Use Preact's render() — it diffs against existing SSR HTML
        preact.render(preact.h(Component, props), el);
        log('Hydrated:', name, el.dataset.id);
      } catch (e) {
        console.error('[runts] Hydration error for', name, e);
      }
    };

    switch (strategy || 'visible') {
      case 'load':
      case 'eager':
        doHydrate();
        break;
      case 'visible': {
        const observer = new IntersectionObserver((entries) => {
          if (entries[0]?.isIntersecting) {
            observer.disconnect();
            doHydrate();
          }
        }, { threshold: 0.1 });
        observer.observe(el);
        break;
      }
      case 'idle': {
        if (typeof requestIdleCallback !== 'undefined') {
          requestIdleCallback(doHydrate);
        } else {
          setTimeout(doHydrate, 1);
        }
        break;
      }
      case 'interaction': {
        const handler = () => {
          el.removeEventListener('click', handler);
          el.removeEventListener('mouseenter', handler);
          doHydrate();
        };
        el.addEventListener('click', handler, { once: true });
        el.addEventListener('mouseenter', handler, { once: true });
        break;
      }
      default:
        doHydrate();
    }
  }

  // ── Bootstrap ──────────────────────────────────────────────────
  async function bootstrap() {
    // Wait for Preact to be available
    if (!global.preact) {
      console.error('[runts] Preact not loaded. Include Preact before island bundles.');
      return;
    }

    const islands = document.querySelectorAll('[data-island]');
    log('Discovered islands:', islands.length);

    islands.forEach(el => {
      const name = el.dataset.island;
      let props = {};
      try {
        props = JSON.parse(el.dataset.props || '{}');
      } catch (e) {
        console.error('[runts] Bad props for', name, e);
      }
      hydrateIsland(el, name, props, el.dataset.hydrate);
    });

    log('Bootstrap complete');
  }

  // ── Global API ─────────────────────────────────────────────────
  global.Runts = {
    registerIsland,
    bootstrap,
    ISLANDS,
  };

  // Auto-bootstrap once Preact + island bundles have loaded
  if (typeof window !== 'undefined') {
    if (document.readyState === 'complete') {
      bootstrap();
    } else {
      window.addEventListener('load', bootstrap);
    }
  }
})(typeof window !== 'undefined' ? window : (typeof global !== 'undefined' ? global : this));

