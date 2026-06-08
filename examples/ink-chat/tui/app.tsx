// ink-chat example — real-world chat interface demonstrating useState, useEffect, useInput
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

/** @jsxImportSource react */
import React, { useState, useEffect, useRef } from "react";
import { render, Text, Box } from "ink";

interface Message {
  id: number;
  role: "user" | "claude";
  content: string;
  timestamp: string;
}

const C = {
  bg: "#0c0c0c",
  fg: "#4a4a4a",
  fgMid: "#6a6a6a",
  fgBright: "#909090",
  accent: "#8b7cf4",
  success: "#3ebd6a",
  warning: "#eab84a",
  dim: "#282828",
};

const App: React.FC = () => {
  const [messages, setMessages] = useState<Message[]>([
    { id: 0, role: "user", content: "implement JWT auth", timestamp: "10:00" },
    { id: 1, role: "claude", content: "I'll check the project first", timestamp: "10:00" },
  ]);
  
  const [input, setInput] = useState("");
  const [cursorPos, setCursorPos] = useState(0);
  const [isThinking, setIsThinking] = useState(false);
  const [isWorking, setIsWorking] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  
  const messageIdRef = useRef(2);
  const inputRef = useRef("");
  const cursorRef = useRef(0);
  const initializedRef = useRef(false);

  useEffect(() => {
    if (initializedRef.current) return;
    initializedRef.current = true;
    
    if (typeof process === "undefined") return;
    process.on("SIGINT", () => process.exit(0));
    if (!process.stdin?.isTTY) return;
    process.stdin.setRawMode?.(true);
    process.stdin.resume?.();
    
    const inputBuffer: string[] = [];
    
    import("node:readline").then(({ createInterface }) => {
      const rl = createInterface({ input: process.stdin, output: process.stdout });
      
      const updateInput = (buffer: string[], cursor: number) => {
        inputRef.current = buffer.join("");
        cursorRef.current = cursor;
        setInput([...buffer].join(""));
        setCursorPos(cursor);
      };
      
      const simulateResponse = (userInput: string) => {
        const timestamp = new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
        
        setMessages(prev => [...prev, { id: messageIdRef.current++, role: "user", content: userInput, timestamp }]);
        
        setIsThinking(true);
        
        setTimeout(() => {
          setIsThinking(false);
          setIsWorking(true);
          
          setTimeout(() => {
            setIsWorking(false);
            const responses = [`Echo: "${userInput}"`, `Got: "${userInput}"`, `You: "${userInput}"`];
            setMessages(prev => [...prev, {
              id: messageIdRef.current++,
              role: "claude",
              content: responses[Math.floor(Math.random() * responses.length)],
              timestamp: new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false })
            }]);
          }, 1000);
        }, 600);
      };
      
      const handleCommand = (cmd: string) => {
        const timestamp = new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
        
        switch (cmd.toLowerCase()) {
          case "clear":
            setMessages([]);
            break;
          case "status":
            setMessages(prev => [...prev, { id: messageIdRef.current++, role: "claude", content: "ready · 12k/64k · 3 files", timestamp }]);
            break;
          case "files":
            setMessages(prev => [...prev, { id: messageIdRef.current++, role: "claude", content: "app.ts · auth.ts · middleware.ts", timestamp }]);
            break;
          default:
            if (cmd.includes("implement") || cmd.includes("create") || cmd.includes("add") || cmd.includes("build")) {
              const timestamp = new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
              setMessages(prev => [...prev, { id: messageIdRef.current++, role: "user", content: cmd, timestamp }]);
              
              // Thinking
              setIsThinking(true);
              setElapsed(0);
              const startTime = Date.now();
              const timer = setInterval(() => {
                setElapsed(((Date.now() - startTime) / 1000).toFixed(1));
              }, 100);
              
              setTimeout(() => {
                clearInterval(timer);
                setIsThinking(false);
                setIsWorking(true);
                setElapsed(0);
                
                // Simulate tool calls in feed
                const toolCalls = [
                  { tool: "read", target: "src/app.ts", status: "✓" },
                  { tool: "read", target: "src/auth.ts", status: "✓" },
                  { tool: "edit", target: "src/auth.ts", status: "✓" },
                  { tool: "bash", target: "npm test", status: "✓" },
                ];
                
                toolCalls.forEach((tc, i) => {
                  setTimeout(() => {
                    setMessages(prev => [...prev, {
                      id: messageIdRef.current++,
                      role: "claude",
                      content: `${tc.status} ${tc.tool} ${tc.target}`,
                      timestamp: new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false })
                    }]);
                  }, i * 600);
                });
                
                const workStartTime = Date.now();
                const workTimer = setInterval(() => {
                  setElapsed(((Date.now() - workStartTime) / 1000).toFixed(1));
                }, 100);
                
                setTimeout(() => {
                  clearInterval(workTimer);
                  setIsWorking(false);
                  setMessages(prev => [...prev, {
                    id: messageIdRef.current++,
                    role: "claude",
                    content: "Done. JWT authentication implemented. All tests passing.",
                    timestamp: new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false })
                  }]);
                }, toolCalls.length * 600 + 500);
              }, 1000);
            } else {
              simulateResponse(cmd);
            }
        }
      };
      
      rl.input.on("keypress", (_str: string, key: any) => {
        if (key.ctrl && key.name === "c") {
          process.exit(0);
          return;
        }
        
        if (key.name === "enter" || key.name === "return") {
          const text = inputRef.current.trim();
          if (text) {
            handleCommand(text);
            inputBuffer.length = 0;
            updateInput(inputBuffer, 0);
          }
        } else if (key.name === "backspace") {
          if (inputBuffer.length > 0) {
            inputBuffer.pop();
            updateInput(inputBuffer, inputBuffer.length);
          }
        } else if (_str && _str.length === 1) {
          const pos = cursorRef.current;
          inputBuffer.splice(pos, 0, _str);
          updateInput(inputBuffer, pos + 1);
        }
      });
    });
  }, []);

  const statusColor = isThinking ? C.accent : isWorking ? C.warning : C.fg;

  const renderInput = () => {
    const text = input;
    const pos = cursorPos;
    if (pos >= text.length) {
      return (
        <>
          <Text color={C.fgMid}>{text}</Text>
          <Text backgroundColor={C.fgBright} color={C.bg}> </Text>
        </>
      );
    }
    const before = text.slice(0, pos);
    const char = text[pos] || " ";
    const after = text.slice(pos + 1);
    return (
      <>
        <Text color={C.fgMid}>{before}</Text>
        <Text backgroundColor={C.fgBright} color={C.bg}>{char}</Text>
        <Text color={C.fgMid}>{after}</Text>
      </>
    );
  };

  return (
    <Box flexDirection="column" height={typeof process !== "undefined" && process.stdout?.rows ? process.stdout.rows : 100}>
      
      {/* CONTEXT - minimal */}
      <Box backgroundColor={C.dim} paddingX={2} paddingY={0}>
        <Text color={C.dim}>12k/64k</Text>
        <Text color={C.dim}> · </Text>
        <Text color={C.dim}>3 files</Text>
        <Text color={C.dim}> · </Text>
        <Text color={C.dim}>{isThinking ? "thinking" : isWorking ? "working" : "ready"}</Text>
      </Box>
      
      {/* FEED - conversation */}
      <Box flexDirection="column" flexGrow={1} backgroundColor={C.bg} paddingX={3} paddingY={1}>
        
        {messages.map((msg) => (
          <Box key={msg.id}>
            <Text color={msg.role === "user" ? C.fgMid : C.dim}>
              {msg.role === "user" ? "$" : "→"}
            </Text>
            <Text color={C.dim}> </Text>
            <Box flexGrow={1}>
              <Text color={C.fgMid}>{msg.content}</Text>
            </Box>
            <Text color={C.dim}>{msg.timestamp}</Text>
          </Box>
        ))}
        
        {isThinking && (
          <Box>
            <Text color={C.dim}>→</Text>
            <Text color={C.dim}> </Text>
            <Text color={C.accent}>◐</Text>
            <Text color={C.dim}> </Text>
            <Text color={C.accent}>{elapsed}s</Text>
          </Box>
        )}
        
        {isWorking && (
          <Box>
            <Text color={C.dim}>→</Text>
            <Text color={C.dim}> </Text>
            <Text color={C.success}>◐</Text>
            <Text color={C.dim}> </Text>
            <Text color={C.success}>{elapsed}s</Text>
          </Box>
        )}
        
        <Box flexGrow={1} />
      </Box>
      
      {/* SEPARATOR */}
      <Box backgroundColor={C.dim} height={1} />
      
      {/* PROMPT */}
      <Box backgroundColor={C.bg} paddingX={2} paddingY={1}>
        <Text color={C.dim}>$</Text>
        <Text color={C.dim}> </Text>
        {renderInput()}
        <Text color={C.dim}>  ?</Text>
      </Box>

    </Box>
  );
};

export default App;

render(React.createElement(App));
