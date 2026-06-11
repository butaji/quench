// File Tree Example - Quench demo (TypeScript)
// Demonstrates recursive rendering and selection

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface FileNode {
  name: string;
  type: 'file' | 'folder';
  children?: FileNode[];
}

interface FileTreeProps {
  nodes: FileNode[];
  depth: number;
  selectedPath: string[];
  onSelect: (path: string[]) => void;
  currentPath: string[];
}

function FileTree(props: FileTreeProps): JSX.Element {
  const { nodes, depth, selectedPath, onSelect, currentPath } = props;
  
  return (
    <Box flexDirection="column">
      {nodes.map((node: FileNode, index: number) => {
        const path = [...currentPath, node.name];
        const isSelected = JSON.stringify(path) === JSON.stringify(selectedPath);
        const icon = node.type === 'folder' ? '📁' : '📄';
        
        return (
          <Box key={index}>
            <Text dimColor>{'  '.repeat(depth)}</Text>
            <Box backgroundColor={isSelected ? 'gray' : undefined}>
              <Text>
                {icon} {node.name}
              </Text>
            </Box>
            {node.type === 'folder' && node.children && (
              <FileTree
                nodes={node.children}
                depth={depth + 1}
                selectedPath={selectedPath}
                onSelect={onSelect}
                currentPath={path}
              />
            )}
          </Box>
        );
      })}
    </Box>
  );
}

function FileTreeApp(): JSX.Element {
  const treeData: FileNode[] = [
    {
      name: 'src',
      type: 'folder',
      children: [
        { name: 'index.ts', type: 'file' },
        { name: 'App.tsx', type: 'file' },
        {
          name: 'components',
          type: 'folder',
          children: [
            { name: 'Button.tsx', type: 'file' },
            { name: 'Modal.tsx', type: 'file' },
          ],
        },
      ],
    },
    {
      name: 'package.json',
      type: 'file',
    },
    {
      name: 'README.md',
      type: 'file',
    },
  ];
  
  const [selectedPath, setSelectedPath] = useState<string[]>([]);
  
  useInput((input: string) => {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    // Simplified: just toggle selection on first item
    if (input === 'return' || input === ' ') {
      setSelectedPath([]);
    }
  });
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="green">File Explorer</Text>
      <Box marginTop={1} borderStyle="round" padding={1}>
        <FileTree
          nodes={treeData}
          depth={0}
          selectedPath={selectedPath}
          onSelect={setSelectedPath}
          currentPath={[]}
        />
      </Box>
      <Text dimColor marginTop={1}>
        Use arrow keys to navigate | [q] quit
      </Text>
    </Box>
  );
}

render(<FileTreeApp />);
