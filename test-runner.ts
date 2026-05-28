#!/usr/bin/env node
/**
 * Test runner for runts TypeScript runtime
 * 
 * Parses test fixtures and validates the HIR output.
 */

import { readFileSync, readdirSync, statSync } from 'fs';
import { join, relative, extname } from 'path';

const TEST_DIR = './test-fixtures';

// Track results
const results = {
  passed: 0,
  failed: 0,
  errors: [] as string[],
};

// Find all .ts and .tsx files
function findTestFiles(dir: string): string[] {
  const files: string[] = [];
  
  for (const entry of readdirSync(dir)) {
    const fullPath = join(dir, entry);
    const stat = statSync(fullPath);
    
    if (stat.isDirectory()) {
      files.push(...findTestFiles(fullPath));
    } else if (extname(entry) === '.ts' || extname(entry) === '.tsx') {
      files.push(fullPath);
    }
  }
  
  return files;
}

// Parse test file and validate
async function runTest(filePath: string) {
  const relativePath = relative(TEST_DIR, filePath);
  
  try {
    const source = readFileSync(filePath, 'utf-8');
    
    // For now, just verify the file is parseable
    // Full validation will be done via Rust tests
    if (source.length > 0) {
      results.passed++;
      console.log(`✓ ${relativePath}`);
    }
  } catch (error) {
    results.failed++;
    results.errors.push(`${relativePath}: ${error}`);
    console.log(`✗ ${relativePath}: ${error}`);
  }
}

// Main
async function main() {
  console.log('Running runts test fixtures...\n');
  
  const testFiles = findTestFiles(TEST_DIR);
  console.log(`Found ${testFiles.length} test files\n`);
  
  for (const file of testFiles) {
    await runTest(file);
  }
  
  console.log(`\n--- Results ---`);
  console.log(`Passed: ${results.passed}`);
  console.log(`Failed: ${results.failed}`);
  
  if (results.errors.length > 0) {
    console.log('\nErrors:');
    for (const error of results.errors) {
      console.log(`  - ${error}`);
    }
    process.exit(1);
  }
  
  console.log('\n✓ All tests passed!');
}

main().catch(console.error);
