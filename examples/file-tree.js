// File Tree Example - TuiBridge demo
// Demonstrates recursive rendering, tree structure, and selection
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

var FILE_ICONS = {
  folder: '📁',
  file: '📄',
  js: '📜',
  ts: '📘',
  json: '📋',
  md: '📝'
};

var SAMPLE_TREE = {
  name: 'project',
  type: 'folder',
  children: [
    { name: 'src', type: 'folder', children: [
      { name: 'main.ts', type: 'ts' },
      { name: 'utils.ts', type: 'ts' },
      { name: 'components', type: 'folder', children: [
        { name: 'App.tsx', type: 'tsx' },
        { name: 'Button.tsx', type: 'tsx' }
      ]},
      { name: 'lib', type: 'folder', children: [
        { name: 'api.js', type: 'js' }
      ]}
    ]},
    { name: 'package.json', type: 'json' },
    { name: 'README.md', type: 'md' },
    { name: '.gitignore', type: 'file' },
    { name: 'tsconfig.json', type: 'json' }
  ]
};

function getIcon(node) {
  if (node.type === 'folder') return FILE_ICONS.folder;
  if (node.type === 'tsx' || node.type === 'ts') return FILE_ICONS.ts;
  if (node.type === 'jsx' || node.type === 'js') return FILE_ICONS.js;
  if (node.type === 'json') return FILE_ICONS.json;
  if (node.type === 'md') return FILE_ICONS.md;
  return FILE_ICONS.file;
}

function flattenTree(node, path, depth, expanded, result) {
  result.push({ node: node, path: path, depth: depth });
  
  if (node.children && expanded[path]) {
    for (var i = 0; i < node.children.length; i++) {
      flattenTree(node.children[i], path + '/' + node.children[i].name, depth + 1, expanded, result);
    }
  }
}

function App() {
  var _useState = useState({ '/project': true });
  var expanded = _useState[0];
  var setExpanded = _useState[1];
  
  var _useState2 = useState(0);
  var selectedIndex = _useState2[0];
  var setSelectedIndex = _useState2[1];
  
  // Build flat list for navigation
  var flatNodes = [];
  flattenTree(SAMPLE_TREE, '/project', 0, expanded, flatNodes);
  
  var selectedNode = flatNodes[selectedIndex];
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === 'j' || input === 'downArrow') {
      setSelectedIndex(function(i) { return Math.min(i + 1, flatNodes.length - 1); });
    }
    if (input === 'k' || input === 'upArrow') {
      setSelectedIndex(function(i) { return Math.max(i - 1, 0); });
    }
    if (input === ' ' || input === 'return') {
      if (selectedNode && selectedNode.node.type === 'folder') {
        var path = selectedNode.path;
        setExpanded(function(e) {
          var next = {};
          for (var k in e) next[k] = e[k];
          next[path] = !next[path];
          return next;
        });
      }
    }
    if (input === 'l' || input === 'rightArrow') {
      // Expand current
      if (selectedNode && selectedNode.node.type === 'folder') {
        var path = selectedNode.path;
        setExpanded(function(e) {
          var next = {};
          for (var k in e) next[k] = e[k];
          next[path] = true;
          return next;
        });
      }
    }
  });
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'File Tree' }},
        { type: Text, props: { dimColor: true, children: '[' + flatNodes.length + ' items] [j/k] navigate | [space] expand | [q] quit' }},
        { type: Text, props: { children: '' }},
        {
          type: Box,
          props: {
            width: 40,
            borderStyle: 'round',
            children: flatNodes.map(function(item, i) {
              var isSelected = (i === selectedIndex);
              var indent = '  '.repeat(item.depth);
              var prefix = item.node.type === 'folder' ? ((expanded[item.path] || false) ? '▼ ' : '▶ ') : '  ';
              var icon = getIcon(item.node);
              
              return {
                type: Text,
                props: {
                  color: isSelected ? 'black' : (item.node.type === 'folder' ? 'yellow' : 'white'),
                  backgroundColor: isSelected ? 'yellow' : undefined,
                  children: indent + prefix + icon + ' ' + item.node.name
                }
              };
            })
          }
        },
        { type: Text, props: { children: '' }},
        selectedNode ? { 
          type: Box, 
          props: { 
            borderStyle: 'single',
            children: [
              { type: Text, props: { dimColor: true, children: 'Selected: ' }},
              { type: Text, props: { bold: true, children: getIcon(selectedNode.node) + ' ' + selectedNode.node.name }},
              { type: Text, props: { dimColor: true, children: ' | Type: ' + selectedNode.node.type }}
            ]
          }
        } : null
      ]
    }
  };
}

render({ type: App, props: {} });
