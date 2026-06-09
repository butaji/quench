// Chat UI Example - TuiBridge demo (TypeScript)
// Demonstrates real-time messaging and input

import { render, Box, Text, useState, useInput, useEffect } from 'ink';

interface Message {
  id: number;
  sender: string;
  text: string;
  timestamp: string;
  isMe: boolean;
}

function ChatApp(): JSX.Element {
  const [messages, setMessages] = useState<Message[]>([
    { id: 1, sender: 'Alice', text: 'Hey, how are you?', timestamp: '10:00', isMe: false },
    { id: 2, sender: 'Bob', text: 'Doing great! Working on TuiBridge.', timestamp: '10:01', isMe: false },
    { id: 3, sender: 'You', text: 'That sounds exciting!', timestamp: '10:02', isMe: true },
  ]);
  const [inputValue, setInputValue] = useState('');
  const [nextId, setNextId] = useState(4);
  
  useEffect(() => {
    // Simulate receiving messages
    const timer = setInterval(() => {
      if (Math.random() > 0.7) {
        const now = new Date();
        const timestamp = now.toTimeString().slice(0, 5);
        const replies = [
          'Interesting!',
          'Tell me more.',
          'I see what you mean.',
          'That makes sense.',
          'Cool!',
        ];
        const reply = replies[Math.floor(Math.random() * replies.length)];
        
        setMessages((m: Message[]) => [
          ...m,
          {
            id: nextId,
            sender: 'Bot',
            text: reply,
            timestamp,
            isMe: false,
          },
        ]);
        setNextId((n: number) => n + 1);
      }
    }, 5000);
    
    return () => clearInterval(timer);
  }, [nextId]);
  
  useInput((input: string) => {
    if (input === 'enter') {
      if (inputValue.trim()) {
        const now = new Date();
        const timestamp = now.toTimeString().slice(0, 5);
        
        setMessages((m: Message[]) => [
          ...m,
          {
            id: nextId,
            sender: 'You',
            text: inputValue,
            timestamp,
            isMe: true,
          },
        ]);
        setNextId((n: number) => n + 1);
        setInputValue('');
      }
    }
    if (input === 'backspace' || input === 'delete') {
      setInputValue((v: string) => v.slice(0, -1));
    }
    if (input.length === 1) {
      setInputValue((v: string) => v + input);
    }
    if (input === 'q' || input === 'Q') {
      process.exit(0);
    }
  });
  
  return (
    <Box flexDirection="column" padding={1} height={20}>
      <Box borderStyle="bold">
        <Text color="green" bold>Chat Room</Text>
      </Box>
      <Box flexDirection="column" flexGrow={1} borderStyle="round" padding={1}>
        {messages.map((msg: Message) => (
          <Box key={msg.id} marginY={1}>
            <Text dimColor>[{msg.timestamp}] </Text>
            <Text color={msg.isMe ? 'cyan' : 'yellow'}>
              {msg.sender}: 
            </Text>
            <Text>{msg.text}</Text>
          </Box>
        ))}
      </Box>
      <Box marginTop={1} borderStyle="single" paddingX={1}>
        <Text dimColor>You: </Text>
        <Text>{inputValue}_</Text>
      </Box>
      <Text dimColor marginTop={1}>
        Type message + [enter] to send | [q] quit
      </Text>
    </Box>
  );
}

render(<ChatApp />);
