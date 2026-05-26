/**
 * Island Hydration
 * 
 * This module handles hydrating islands from server-rendered HTML.
 */

// Re-export from runtime
export {
  hydrateIsland,
  hydrateAll,
  hydrateOnVisible,
  hydrateOnInteraction,
  findIslands,
  type IslandInfo,
  type HydrationResult,
} from './runtime';

import { hydrateAll } from './runtime';

// Get hydration mode from data attribute
function getHydrationMode(el: Element): 'eager' | 'lazy' | 'visible' | 'interaction' {
  const mode = el.getAttribute('data-mode');
  switch (mode) {
    case 'visible':
      return 'visible';
    case 'interaction':
      return 'interaction';
    case 'eager':
      return 'eager';
    case 'lazy':
    default:
      return 'lazy';
  }
}

// Get events for interaction mode
function getInteractionEvents(el: Element): string[] {
  const eventsAttr = el.getAttribute('data-events');
  if (eventsAttr) {
    return eventsAttr.split(',').map(e => e.trim());
  }
  return ['click', 'focus'];
}

// Initialize hydration based on mode
export function initHydration(): void {
  const islands = document.querySelectorAll('[data-island]');
  
  islands.forEach((el) => {
    const mode = getHydrationMode(el);
    
    switch (mode) {
      case 'eager':
        // Hydrate immediately
        import('./runtime').then(({ hydrateIsland, findIslands }) => {
          const info = findIslands().find(i => i.id === el.getAttribute('data-id'));
          if (info) {
            hydrateIsland(info, el as HTMLElement);
          }
        });
        break;
        
      case 'lazy':
        // Hydrate after 100ms idle
        let hydrated = false;
        const hydrate = () => {
          if (!hydrated) {
            hydrated = true;
            import('./runtime').then(({ hydrateIsland, findIslands }) => {
              const info = findIslands().find(i => i.id === el.getAttribute('data-id'));
              if (info) {
                hydrateIsland(info, el as HTMLElement);
              }
            });
          }
        };
        requestIdleCallback ? requestIdleCallback(hydrate) : setTimeout(hydrate, 100);
        break;
        
      case 'visible':
        // Hydrate when visible (IntersectionObserver)
        if (typeof IntersectionObserver !== 'undefined') {
          const observer = new IntersectionObserver(
            (entries) => {
              entries.forEach((entry) => {
                if (entry.isIntersecting) {
                  observer.disconnect();
                  hydrateAll();
                }
              });
            },
            { rootMargin: '100px' }
          );
          observer.observe(el);
        } else {
          hydrateAll();
        }
        break;
        
      case 'interaction':
        // Hydrate on first interaction
        const events = getInteractionEvents(el);
        const handleInteraction = () => {
          events.forEach((event) => {
            el.removeEventListener(event, handleInteraction);
          });
          hydrateAll();
        };
        events.forEach((event) => {
          el.addEventListener(event, handleInteraction, { once: true, passive: true });
        });
        break;
    }
  });
}

// Auto-init
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initHydration);
  } else {
    initHydration();
  }
}
