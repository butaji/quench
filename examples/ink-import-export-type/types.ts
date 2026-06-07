// Type definitions (types.ts)
export type Status = 'idle' | 'loading' | 'success' | 'error';
export type Theme = 'light' | 'dark';

export interface Config {
  status: Status;
  theme: Theme;
  title: string;
}

// Type-only re-export
export type { Status as AppStatus, Theme as AppTheme };
