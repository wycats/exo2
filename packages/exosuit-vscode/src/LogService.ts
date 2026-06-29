import * as vscode from "vscode";
import { getLogger } from "./logging";

const logger = getLogger("extension");

export interface ActivityItemData {
  label: string;
  description?: string;
  icon?: string;
  tooltip?: string;
  file?: string;
}

export interface ActivityEvent {
  id: string;
  timestamp: number;
  type: "system" | "context" | "axiom" | "llm";
  label: string;
  details?: string;
  items?: ActivityItemData[];
  icon?: string;
  file?: string;
}

export class LogService {
  private static _instance: LogService;
  private _logs: ActivityEvent[] = [];
  private _onLog = new vscode.EventEmitter<void>();

  private constructor() {}

  public static get instance(): LogService {
    if (!LogService._instance) {
      LogService._instance = new LogService();
    }
    return LogService._instance;
  }

  public get onLog(): vscode.Event<void> {
    return this._onLog.event;
  }

  public log(context: string, message: string) {
    // Legacy support: convert string log to event
    this.logActivity({
      type: "system",
      label: message,
      details: context,
      icon: "info",
    });
  }

  public logActivity(event: Omit<ActivityEvent, "id" | "timestamp">) {
    const fullEvent: ActivityEvent = {
      id: Math.random().toString(36).substring(7),
      timestamp: Date.now(),
      ...event,
    };

    this._logs.push(fullEvent);
    logger.info(`[Exosuit] [${fullEvent.type}] ${fullEvent.label}`);
    this._onLog.fire();
  }

  public getLogs(): ActivityEvent[] {
    return [...this._logs];
  }

  public getLogsHtml(): string {
    return this._logs
      .map((l) => {
        const time = new Date(l.timestamp).toLocaleTimeString();
        const iconClass = l.icon ? `codicon codicon-${l.icon}` : "";
        return `
        <div class="log-entry type-${l.type}">
          <span class="timestamp">[${time}]</span>
          <span class="icon ${iconClass}"></span>
          <span class="label">${l.label}</span>
          ${l.details ? `<div class="details">${l.details}</div>` : ""}
        </div>
      `;
      })
      .join("");
  }
}
