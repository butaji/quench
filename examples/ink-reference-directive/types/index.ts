// Re-export types from declaration file

export interface Config {
  version: string;
  enabled: boolean;
}

export type Status = "active" | "inactive" | "pending";
