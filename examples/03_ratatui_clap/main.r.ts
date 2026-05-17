import { Command, Arg } from "clap";
import { Terminal, CrosstermBackend } from "ratatui";

export type Args = {
  file: string;
  interval: number;
};

export function parseArgs(): Args {
  return Command.new("todox")
    .about("Task tracker TUI")
    .arg(Arg.new("file").required(true).help("Tasks JSON file"))
    .arg(Arg.new("interval").defaultValue("5").help("Refresh seconds"))
    .parse();
}

export function runTerminal(args: Args): void {
  const terminal = Terminal.new(CrosstermBackend.new());
  const app = AppState.new(args.file);

  while (app.running) {
    terminal.draw((frame) => {
      frame.renderWidget(rootView(app), frame.area());
    });

    if (pollEvent(args.interval * 1000)) {
      handleInput(app);
    }
  }
}
