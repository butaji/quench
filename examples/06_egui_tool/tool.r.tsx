import { Ctx, Window, CentralPanel, SidePanel, TopBottomPanel } from "egui";
import { Task } from "./models.r.ts";

interface Props {
  tasks: Task[];
  selected: number | null;
}

export function taskEditor(ctx: Ctx, props: Props): void {
  return (
    <>
    <TopBottomPanel top="48px">
      <div style={{ layout: "horizontal", spacing: "8px" }}>
        <button onclick={() => newTask(ctx)}>+ New</button>
        <button onclick={() => saveTasks(ctx)}>Save</button>
      </div>
    </TopBottomPanel>
    <SidePanel left="200px">
      <Window title="Task List">
        {props.tasks.map((task, i) => (
          <selectable
            selected={props.selected === task.id}
            onclick={() => selectTask(ctx, task.id)}
          >
            {task.done ? "☑" : "☐"} {task.title}
          </selectable>
        ))}
      </Window>
    </SidePanel>
    <CentralPanel>
      {props.selected !== null && (
        <Window title="Editor" scroll={true}>
          <input label="Title" value={currentTitle(ctx)} />
          <checkbox label="Done" checked={currentDone(ctx)} />
          <colorPicker label="Tag" color={currentColor(ctx)} />
        </Window>
      )}
    </CentralPanel>
    </>
  );
}
