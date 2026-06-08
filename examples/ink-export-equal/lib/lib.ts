// Standalone utility functions that can be exported
export function createGreeting(name: string): string {
  return `Hello, ${name}!`;
}

export function createFarewell(name: string): string {
  return `Goodbye, ${name}!`;
}

// Export an object - demonstrates module export pattern
export const utils = {
  formatName: (name: string) => name.toUpperCase(),
  getYear: () => new Date().getFullYear(),
};
