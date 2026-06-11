// Context Demo — Quench
// Demonstrates useContext / createContext for theme sharing

var useState = ink.useState;
var useContext = ink.useContext;
var createContext = ink.createContext;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

var ThemeContext = createContext({ name: 'dark', fg: 'white', bg: '#1a1a1a', accent: 'cyan' });

var THEMES = {
  dark:  { name: 'dark',  fg: 'white', bg: '#1a1a1a', accent: 'cyan' },
  light: { name: 'light', fg: 'black', bg: 'white', accent: 'blue' },
  retro: { name: 'retro', fg: 'green', bg: 'black', accent: 'yellow' },
};

function ThemedBox(props) {
  var theme = useContext(ThemeContext);
  return {
    type: Box,
    props: {
      borderStyle: 'round',
      borderColor: theme.accent,
      padding: 1,
      backgroundColor: theme.bg,
      children: props.children
    }
  };
}

function ThemedText(props) {
  var theme = useContext(ThemeContext);
  return {
    type: Text,
    props: Object.assign({ color: theme.fg }, props)
  };
}

function ContextDemo() {
  var _useState = useState('dark');
  var themeName = _useState[0];
  var setThemeName = _useState[1];

  var theme = THEMES[themeName];

  useInput(function(input) {
    if (input === 'q') useApp().exit();
    if (input === '1') setThemeName('dark');
    if (input === '2') setThemeName('light');
    if (input === '3') setThemeName('retro');
  });

  return {
    type: ThemeContext.Provider,
    props: {
      value: theme,
      children: {
        type: Box,
        props: {
          flexDirection: 'column',
          padding: 1,
          children: [
            { type: ThemedText, props: { bold: true, children: 'Context Theme Demo' } },
            { type: ThemedText, props: { dimColor: true, children: '[1-3] switch theme | [q] quit' } },
            { type: Text, props: { children: '' } },
            {
              type: ThemedBox,
              props: {
                children: [
                  { type: ThemedText, props: { children: 'Current theme: ' + theme.name } },
                  { type: ThemedText, props: { children: 'Foreground: ' + theme.fg } },
                  { type: ThemedText, props: { children: 'Background: ' + theme.bg } },
                  { type: ThemedText, props: { children: 'Accent: ' + theme.accent } }
                ]
              }
            },
            { type: Text, props: { children: '' } },
            {
              type: Box,
              props: {
                flexDirection: 'row',
                gap: 1,
                children: [
                  { type: Text, props: { color: themeName === 'dark' ? 'cyan' : 'gray', children: '[1] Dark' } },
                  { type: Text, props: { color: themeName === 'light' ? 'blue' : 'gray', children: '[2] Light' } },
                  { type: Text, props: { color: themeName === 'retro' ? 'yellow' : 'gray', children: '[3] Retro' } }
                ]
              }
            }
          ]
        }
      }
    }
  };
}

render({ type: ContextDemo, props: {} });
