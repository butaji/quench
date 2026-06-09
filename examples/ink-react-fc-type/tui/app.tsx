// React.FC and React.FunctionComponent type annotation example.
// Exercises common React type patterns that include implicit children prop typing.

import React, { FC, FunctionComponent } from 'react';
import { Box, Text } from 'ink';

interface Props {
  title: string;
}

const Header: FC<Props> = ({ title }) => (
  <Text bold color="cyan">{title}</Text>
);

const SubHeader: FunctionComponent<Props> = ({ title }) => (
  <Text dimColor>{title}</Text>
);

export default function App() {
  return (
    <Box flexDirection="column">
      <Header title="Main Title" />
      <SubHeader title="Subtitle" />
    </Box>
  );
}
