export interface PlanItem {
  id: string;
  title: string;
  status: "todo" | "in-progress" | "done" | "skipped";
  type?: "epoch" | "phase" | "section" | "task";
  children: PlanItem[];
}

export interface Task {
  id: string;
  title: string;
  status: "todo" | "in-progress" | "done";
  relatesTo?: string;
}

export interface PlanTree {
  items: PlanItem[];
}

export interface TaskList {
  tasks: Task[];
}
