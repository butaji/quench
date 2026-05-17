import { Block, Borders, List, ListItem, Gauge, Layout, Constraint } from "ratatui";
import { Task } from "./models.r.ts";

interface Props {
  tasks: Task[];
  selected: number;
  progress: number;
}

export function rootView(props: Props): Widget {
  return (
    <Layout direction="vertical" constraints={[Constraint.Percentage(80), Constraint.Percentage(20)]}>
      <List
        block={<Block title="Tasks" borders={Borders.ALL} />}
        items={props.tasks.map((task, i) => (
          <ListItem style={i === props.selected ? "bold" : "default"}>
            {task.done ? "[x] " : "[ ] "}{task.title}
          </ListItem>
        ))}
        highlightSymbol=">"
      />
      <Gauge
        block={<Block title="Progress" borders={Borders.ALL} />}
        percent={props.progress}
        label={`${props.progress}%`}
      />
    </Layout>
  );
}
