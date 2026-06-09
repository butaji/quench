// Simple test script for TuiBridge
// Creates a basic Box with Text and measures it

// Test basic FFI functions
console.log('TuiBridge FFI Test');
console.log('==================');

// Test 1: Create root
var rootId = __ink_create_root();
console.log('Created root:', rootId);

// Test 2: Create a Box
var boxId = __ink_create_node('ink-box', '{"flexDirection":"column","padding":2}');
console.log('Created Box:', boxId);

// Test 3: Create Text
var textId = __ink_create_text_node('Hello, TuiBridge!');
console.log('Created Text:', textId);

// Test 4: Append text to box
__ink_append_child(String(boxId), String(textId));
console.log('Appended text to box');

// Test 5: Append box to root
__ink_append_child(String(rootId), String(boxId));
console.log('Appended box to root');

// Test 6: Commit changes
__ink_commit();
console.log('Committed changes');

// Test 7: Measure text
var size = __ink_measure_text('Hello, World!', 80);
console.log('Text measure:', size);

// Test 8: Get layout
__ink_set_terminal_size(80, 24);
__ink_calculate_layout();
var layout = __ink_get_layout(boxId);
console.log('Box layout:', layout);

// Test 9: Get node info
var tag = __ink_get_node_tag(boxId);
var text = __ink_get_node_text(textId);
var children = __ink_get_node_children(boxId);
console.log('Box tag:', tag);
console.log('Text content:', text);
console.log('Box children:', children);

// Test 10: Update a node
__ink_commit_update(String(textId), '{"color":"green"}');
__ink_commit();
console.log('Updated text color');

// Test 11: Terminal size
var termSize = __ink_get_terminal_size();
console.log('Terminal size:', termSize);

// Test 12: Measure element
var elemSize = __ink_measure_element(String(textId));
console.log('Element size:', elemSize);

console.log('\\nAll FFI tests passed!');
