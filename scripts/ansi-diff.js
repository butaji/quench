#!/usr/bin/env node
/**
 * ANSI Diff Tool — Compare ANSI terminal outputs cell-by-cell
 * 
 * Usage: node scripts/ansi-diff.js <file1.ansi> <file2.ansi>
 * 
 * Output shows:
 * - Side-by-side comparison of lines that differ
 * - Visual markers for added/removed characters
 * - Exit code 0 if identical, 1 if different
 */

const fs = require('fs');
const readline = require('readline');

// Strip ANSI escape sequences from a string
function stripAnsi(str) {
    return str.replace(/\x1b\[[0-9;]*[A-Za-z]/g, '').replace(/\x1b\[[0-9;]*m/g, '');
}

// Parse a file into lines with ANSI preserved
function parseFile(filepath) {
    if (!fs.existsSync(filepath)) {
        console.error(`File not found: ${filepath}`);
        process.exit(1);
    }
    
    const content = fs.readFileSync(filepath, 'utf-8');
    return content.split('\n').map(line => ({
        raw: line,
        stripped: stripAnsi(line)
    }));
}

// Compare two files
function compareFiles(file1, file2) {
    const lines1 = parseFile(file1);
    const lines2 = parseFile(file2);
    
    const maxLines = Math.max(lines1.length, lines2.length);
    const differences = [];
    
    for (let i = 0; i < maxLines; i++) {
        const line1 = lines1[i] || { raw: '', stripped: '' };
        const line2 = lines2[i] || { raw: '', stripped: '' };
        
        if (line1.stripped !== line2.stripped) {
            differences.push({
                line: i + 1,
                file1: line1.raw,
                file2: line2.raw,
                stripped1: line1.stripped,
                stripped2: line2.stripped
            });
        }
    }
    
    return differences;
}

// Main
const args = process.argv.slice(2);

if (args.length < 2) {
    console.log('Usage: ansi-diff.js <file1.ansi> <file2.ansi>');
    console.log('');
    console.log('Compares two ANSI terminal output files and shows differences.');
    console.log('Exit code 0 = identical, 1 = different');
    process.exit(1);
}

const file1 = args[0];
const file2 = args[1];

const differences = compareFiles(file1, file2);

if (differences.length === 0) {
    console.log(`✓ Files are identical: ${file1} vs ${file2}`);
    process.exit(0);
} else {
    console.log(`✗ Found ${differences.length} line(s) with differences:`);
    console.log('');
    
    for (const diff of differences.slice(0, 20)) { // Show max 20 differences
        console.log(`Line ${diff.line}:`);
        console.log(`  File1: ${diff.stripped1 || '(empty)'}`);
        console.log(`  File2: ${diff.stripped2 || '(empty)'}`);
        
        // Show character-level diff
        const chars1 = diff.stripped1.split('');
        const chars2 = diff.stripped2.split('');
        let mark1 = '';
        let mark2 = '';
        
        for (let i = 0; i < Math.max(chars1.length, chars2.length); i++) {
            if (chars1[i] !== chars2[i]) {
                mark1 += (chars1[i] || ' ');
                mark2 += (chars2[i] || ' ');
            }
        }
        
        if (mark1.trim()) console.log(`  Mark1:  ${mark1}`);
        if (mark2.trim()) console.log(`  Mark2:  ${mark2}`);
        console.log('');
    }
    
    if (differences.length > 20) {
        console.log(`... and ${differences.length - 20} more differences`);
    }
    
    process.exit(1);
}
