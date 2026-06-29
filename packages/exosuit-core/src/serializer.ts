import type { TaskList } from "./types.ts";

export class ExosuitSerializer {
  // TODO: This is a placeholder. Real serialization needs to reconstruct the AST from the objects.
  // For now, we might just rely on remark-stringify if we were manipulating the AST directly.
  // But since we are converting to Objects (PlanItem, Task), we need a way to convert back.

  // However, for the "Markdown ORM" approach, we might want to keep the AST around
  // and only modify the nodes we need, to preserve formatting of untouched parts.
  // But the design doc implies "Data Models" are the primary interface.

  // Let's start with a simple "Object -> Markdown" generation for new files,
  // but for editing, we might need a smarter approach (AST manipulation).

  // For this phase, let's just verify we can parse correctly.
  // I will implement a simple "stringify" that takes our objects and produces Markdown.

  serializeTasks(taskList: TaskList): string {
    let output = "# Tasks\n\n";
    for (const task of taskList.tasks) {
      const check = task.status === "done" ? "x" : " ";
      let line = `- [${check}] ${task.title}`;
      if (task.id) {
        line += ` <!-- id: "${task.id}" -->`;
      }
      if (task.relatesTo) {
        line += ` <!-- relates-to: "${task.relatesTo}" -->`;
      }
      output += line + "\n";
    }
    return output;
  }
}
