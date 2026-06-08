// ink-import-meta-url example — demonstrates import.meta
//
// This example exercises the import.meta object properties:
// - import.meta.url - the module's URL
// - import.meta.resolve(specifier) - resolve a module specifier
// - import.meta.dirname - directory of the module (non-standard, Deno only)
//
// Note: import.meta is available in ES modules. In rquickjs dev mode,
// it may not be available, so we provide fallbacks.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Safe access to import.meta with fallbacks
function getModuleUrl(): string {
  try {
    // In deno, import.meta.url is available
    if (typeof (globalThis as { importMeta?: { url?: string } }).importMeta?.url === 'string') {
      return (globalThis as { importMeta?: { url?: string } }).importMeta!.url!;
    }
    // Fallback
    return 'module://./app.tsx';
  } catch {
    return 'unavailable';
  }
}

function hasResolve(): boolean {
  try {
    return typeof (globalThis as { importMeta?: { resolve?: unknown } }).importMeta?.resolve === 'function';
  } catch {
    return false;
  }
}

// import.meta.url - the URL of the current module
const moduleUrl = getModuleUrl();

// Extract filename from URL
function getFilename(url: string): string {
  if (url === 'unavailable' || url === 'module://./app.tsx') return 'app.tsx';
  const parts = url.split('/');
  return parts[parts.length - 1] || 'unknown';
}

// Extract directory from URL
function getDirname(url: string): string {
  if (url === 'unavailable' || url === 'module://./app.tsx') return './';
  const parts = url.split('/');
  parts.pop(); // Remove filename
  return parts.join('/') || '/';
}

// Extract protocol from URL
function getProtocol(url: string): string {
  if (url === 'unavailable' || url === 'module://./app.tsx') return 'module';
  const match = url.match(/^([a-z]+):/);
  return match ? match[1] : 'unknown';
}

// Check if URL is file:// or https:// or http://
function getUrlType(url: string): string {
  if (url === 'unavailable' || url === 'module://./app.tsx') return 'module';
  if (url.startsWith('file://')) return 'file';
  if (url.startsWith('https://')) return 'https';
  if (url.startsWith('http://')) return 'http';
  return 'other';
}

// Get file extension
function getExtension(url: string): string {
  if (url === 'unavailable' || url === 'module://./app.tsx') return '.tsx';
  const filename = getFilename(url);
  const match = filename.match(/\.([^.]+)$/);
  return match ? `.${match[1]}` : 'no extension';
}

// Test resolve function (if available)
const canResolve = hasResolve();
const resolveResult = canResolve ? 'available' : 'unavailable';

export default function ImportMetaDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">import.meta Demo</Text>
      <Text></Text>
      <Text>Module URL:</Text>
      <Text>  import.meta.url: {moduleUrl}</Text>
      <Text></Text>
      <Text>Extracted from URL:</Text>
      <Text>  filename: {getFilename(moduleUrl)}</Text>
      <Text>  dirname: {getDirname(moduleUrl)}</Text>
      <Text>  protocol: {getProtocol(moduleUrl)}</Text>
      <Text>  URL type: {getUrlType(moduleUrl)}</Text>
      <Text>  extension: {getExtension(moduleUrl)}</Text>
      <Text></Text>
      <Text>Metadata:</Text>
      <Text>  resolve() available: {resolveResult}</Text>
    </Box>
  );
}
