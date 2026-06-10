// Text Styles Example — TuiBridge
// Demonstrates all Ink text styling props
// Covers: italic, strikethrough, underline, inverse, transform, textWrap

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface Style {
  label: string;
  props: {
    bold?: boolean;
    dimColor?: boolean;
    italic?: boolean;
    underline?: boolean;
    strikethrough?: boolean;
    inverse?: boolean;
    transform?: 'uppercase' | 'lowercase';
    backgroundColor?: string;
    color?: string;
  };
}

const STYLES: Style[] = [
  { label: 'Normal', props: {} },
  { label: 'Bold', props: { bold: true } },
  { label: 'Dim', props: { dimColor: true } },
  { label: 'Italic', props: { italic: true } },
  { label: 'Underline', props: { underline: true } },
  { label: 'Strikethrough', props: { strikethrough: true } },
  { label: 'Inverse', props: { inverse: true } },
  { label: 'Uppercase', props: { transform: 'uppercase' } },
  { label: 'Lowercase', props: { transform: 'lowercase' } },
  { label: 'Red bg', props: { backgroundColor: 'red', color: 'white' } },
];

function StyleItem({ style, isActive, onSelect }: { style: Style; isActive: boolean; onSelect: () => void }) {
  return (
    <Box flexDirection="row" onSelect={onSelect}>
      <Text color={isActive ? 'yellow' : 'gray'}>
        {isActive ? '> ' : '  '}
      </Text>
      <Text
        bold={style.props.bold}
        dimColor={style.props.dimColor}
        italic={style.props.italic}
        underline={style.props.underline}
        strikethrough={style.props.strikethrough}
        inverse={style.props.inverse}
        transform={style.props.transform}
        backgroundColor={style.props.backgroundColor}
        color={style.props.color}
      >
        {style.label}
      </Text>
    </Box>
  );
}

function TextStyles() {
  const [activeIdx, setActiveIdx] = useState(0);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'j' || input === 'downArrow') {
      setActiveIdx((i: number) => Math.min(i + 1, STYLES.length - 1));
    }
    if (input === 'k' || input === 'upArrow') {
      setActiveIdx((i: number) => Math.max(i - 1, 0));
    }
  });

  const activeStyle = STYLES[activeIdx];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Text Styles Demo</Text>
      <Text dimColor>[j/k] navigate | [q] quit</Text>
      <Text> </Text>
      
      {/* Active style preview */}
      <Box borderStyle="single" padding={1} borderColor="yellow">
        <Text dimColor>Preview: </Text>
        <Text
          bold={activeStyle.props.bold}
          dimColor={activeStyle.props.dimColor}
          italic={activeStyle.props.italic}
          underline={activeStyle.props.underline}
          strikethrough={activeStyle.props.strikethrough}
          inverse={activeStyle.props.inverse}
          transform={activeStyle.props.transform}
          backgroundColor={activeStyle.props.backgroundColor}
          color={activeStyle.props.color}
        >
          {activeStyle.label} text
        </Text>
      </Box>
      
      <Text> </Text>
      
      {/* Style list */}
      <Box flexDirection="column">
        {STYLES.map((style, i) => (
          <StyleItem
            key={style.label}
            style={style}
            isActive={i === activeIdx}
            onSelect={() => setActiveIdx(i)}
          />
        ))}
      </Box>
    </Box>
  );
}

render(<TextStyles />);
