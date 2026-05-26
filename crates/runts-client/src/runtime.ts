/**
 * runts Client Runtime
 * 
 * This is the client-side runtime for hydrating islands.
 * It's a minimal Preact-compatible runtime for use in the browser.
 * 
 * In production, this would be bundled with each island.
 */

// ============================================================================
// Signals
// ============================================================================

type SignalValue<T> = {
  value: T;
  subscribers: Set<() => void>;
};

export function signal<T>(initialValue: T): Signal<T> {
  const state: SignalValue<T> = {
    value: initialValue,
    subscribers: new Set(),
  };

  return {
    get value(): T {
      // Track subscription when accessed in effect
      if (currentEffect) {
        state.subscribers.add(currentEffect);
        effectCleanup.add(() => state.subscribers.delete(currentEffect));
      }
      return state.value;
    },
    set value(newValue: T) {
      if (state.value !== newValue) {
        state.value = newValue;
        // Notify all subscribers
        state.subscribers.forEach(fn => fn());
      }
    },
    peek: () => state.value,
  };
}

export function computed<T>(fn: () => T): Signal<T> {
  const sig = signal<T>(undefined as any);
  let currentValue: T;
  
  effect(() => {
    currentValue = fn();
    sig.value = currentValue;
  });
  
  return sig;
}

let currentEffect: (() => void) | null = null;
const effectCleanup = new Set<() => void>();

export function effect(fn: () => void | (() => void)): () => void {
  const cleanupFns: (() => void)[] = [];
  
  const wrappedFn = () => {
    currentEffect = wrappedFn;
    effectCleanup.delete(wrappedFn);
    const cleanup = fn();
    if (typeof cleanup === 'function') {
      cleanupFns.push(cleanup);
    }
  };
  
  wrappedFn();
  
  return () => {
    effectCleanup.delete(wrappedFn);
    cleanupFns.forEach(fn => fn());
  };
}

export function batch(fn: () => void): void {
  fn();
}

// ============================================================================
// Hooks (simplified client-side versions)
// ============================================================================

type StateSetter<T> = (value: T | ((prev: T) => T)) => void;

export function useState<T>(initialValue: T | (() => T)): [T, StateSetter<T>] {
  const sig = typeof initialValue === 'function'
    ? signal((initialValue as () => T)())
    : signal(initialValue);
  
  const setValue: StateSetter<T> = (value) => {
    if (typeof value === 'function') {
      sig.value = (value as (prev: T) => T)(sig.peek());
    } else {
      sig.value = value;
    }
  };
  
  return [sig.value, setValue];
}

export function useRef<T>(initialValue: T): { current: T } {
  return { current: initialValue };
}

export function useEffect(fn: () => void | (() => void), deps?: any[]): void {
  if (typeof window === 'undefined') return; // SSR guard
  
  let cleanup: (() => void) | void;
  let oldDeps: any[] | undefined;
  
  const effect = () => {
    if (deps !== undefined && oldDeps !== undefined) {
      // Check if deps changed
      if (deps.every((d, i) => d === oldDeps[i])) {
        return;
      }
    }
    
    if (cleanup) {
      (cleanup as () => void)();
    }
    
    cleanup = fn();
    oldDeps = deps ? [...deps] : undefined;
  };
  
  // Schedule effect
  requestAnimationFrame(effect);
}

export function useMemo<T>(fn: () => T, deps: any[]): T {
  const [value, setValue] = useState<T>(() => fn());
  
  useEffect(() => {
    setValue(() => fn());
  }, deps);
  
  return value;
}

export function useCallback<T extends (...args: any[]) => any>(
  fn: T,
  deps: any[]
): T {
  return useMemo(() => fn, deps);
}

// ============================================================================
// Island Hydration
// ============================================================================

export interface IslandInfo {
  name: string;
  id: string;
  hash: string;
  props: Record<string, any>;
}

export interface HydrationResult {
  element: HTMLElement;
  unmount: () => void;
}

// Find all islands in the DOM
export function findIslands(): IslandInfo[] {
  const elements = document.querySelectorAll('[data-island]');
  const islands: IslandInfo[] = [];
  
  elements.forEach((el) => {
    const name = el.getAttribute('data-island');
    const id = el.getAttribute('data-id');
    const hash = el.getAttribute('data-hash');
    const propsJson = el.getAttribute('data-props');
    
    if (name && id && hash) {
      let props: Record<string, any> = {};
      
      try {
        props = propsJson ? JSON.parse(propsJson) : {};
      } catch (e) {
        console.error(`[runts] Failed to parse props for island ${name}:`, e);
      }
      
      islands.push({ name, id, hash, props });
    }
  });
  
  return islands;
}

// Hydrate a single island
export async function hydrateIsland(
  info: IslandInfo,
  container: HTMLElement
): Promise<HydrationResult> {
  console.log(`[runts] Hydrating island: ${info.name}`, info.props);
  
  // Load island bundle
  const modulePath = `/_runts/islands/${info.name}.${info.hash}.js`;
  
  try {
    // Dynamic import of island bundle
    const module = await import(/* @vite-ignore */ modulePath);
    
    if (typeof module.hydrate === 'function') {
      const cleanup = module.hydrate(info.props);
      
      return {
        element: container,
        unmount: typeof cleanup === 'function' ? cleanup : () => {},
      };
    } else {
      console.warn(`[runts] Island ${info.name} has no hydrate function`);
    }
  } catch (e) {
    console.error(`[runts] Failed to hydrate island ${info.name}:`, e);
  }
  
  return {
    element: container,
    unmount: () => {},
  };
}

// Hydrate all islands
export async function hydrateAll(): Promise<HydrationResult[]> {
  const islands = findIslands();
  const results: HydrationResult[] = [];
  
  for (const info of islands) {
    const container = document.querySelector(`[data-id="${info.id}"]`) as HTMLElement;
    
    if (container) {
      const result = await hydrateIsland(info, container);
      results.push(result);
    }
  }
  
  console.log(`[runts] Hydrated ${results.length} islands`);
  return results;
}

// Lazy hydration with IntersectionObserver
export function hydrateOnVisible(
  info: IslandInfo,
  container: HTMLElement
): void {
  if (typeof IntersectionObserver === 'undefined') {
    // Fallback: hydrate immediately
    hydrateIsland(info, container);
    return;
  }
  
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          observer.disconnect();
          hydrateIsland(info, container);
        }
      });
    },
    { rootMargin: '100px' }
  );
  
  observer.observe(container);
}

// Hydration on interaction (click, focus, hover)
export function hydrateOnInteraction(
  info: IslandInfo,
  container: HTMLElement,
  events: string[] = ['click', 'focus', 'hover']
): void {
  let hydrated = false;
  
  const hydrate = () => {
    if (!hydrated) {
      hydrated = true;
      events.forEach((event) => {
        container.removeEventListener(event, hydrate);
      });
      hydrateIsland(info, container);
    }
  };
  
  events.forEach((event) => {
    container.addEventListener(event, hydrate, { once: true, passive: true });
  });
}

// ============================================================================
// Initialization
// ============================================================================

// Auto-hydrate on DOMContentLoaded
if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      hydrateAll();
    });
  } else {
    // DOM already loaded, hydrate immediately
    hydrateAll();
  }
}

// Export for manual control
export default {
  signal,
  computed,
  effect,
  batch,
  useState,
  useRef,
  useEffect,
  useMemo,
  useCallback,
  findIslands,
  hydrateIsland,
  hydrateAll,
  hydrateOnVisible,
  hydrateOnInteraction,
};
