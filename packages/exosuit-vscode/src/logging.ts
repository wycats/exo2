import * as vscode from "vscode";
import { format } from "node:util";
import {
  Logger,
  type LogComponent,
  type LogLevel,
  type LogSink,
  isLogLevel,
} from "@exosuit/core";

type LogLevelConfig = Partial<Record<"default" | LogComponent, LogLevel>>;

let outputChannel: vscode.OutputChannel | null = null;

/**
 * Structured log entry for the in-memory buffer.
 */
export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  component: LogComponent;
  message: string;
}

/**
 * Circular buffer for recent log entries, accessible via LM tool.
 */
class LogBuffer {
  private entries: LogEntry[] = [];
  private maxSize: number;

  constructor(maxSize = 1000) {
    this.maxSize = maxSize;
  }

  push(entry: LogEntry): void {
    this.entries.push(entry);
    if (this.entries.length > this.maxSize) {
      this.entries.shift();
    }
  }

  getRecent(
    count: number,
    filter?: { level?: LogLevel; component?: LogComponent },
  ): LogEntry[] {
    let filtered = this.entries;

    if (filter?.level) {
      const levelPriority: Record<LogLevel, number> = {
        silent: 0,
        error: 1,
        warn: 2,
        info: 3,
        debug: 4,
        trace: 5,
      };
      const minPriority = levelPriority[filter.level];
      filtered = filtered.filter((e) => levelPriority[e.level] <= minPriority);
    }

    if (filter?.component) {
      filtered = filtered.filter((e) => e.component === filter.component);
    }

    return filtered.slice(-count);
  }

  clear(): void {
    this.entries = [];
  }
}

const logBuffer = new LogBuffer();

/**
 * Get recent log entries from the in-memory buffer.
 * Used by the exo-logs LM tool.
 */
export function getRecentLogs(
  count = 50,
  filter?: { level?: LogLevel; component?: LogComponent },
): LogEntry[] {
  return logBuffer.getRecent(Math.min(count, 500), filter);
}

export function initializeLogging(channel: vscode.OutputChannel): void {
  outputChannel = channel;
}

function formatLogLine(
  level: LogLevel,
  component: LogComponent,
  message: string,
  args: unknown[],
): string {
  const rendered = args.length ? format(message, ...args) : String(message);
  return `[exosuit:${component}] [${level}] ${rendered}`;
}

function getConfiguredLevel(component: LogComponent): LogLevel {
  const config = vscode.workspace.getConfiguration("exosuit");
  const logLevelConfig = (config.get("logLevel") ?? {}) as LogLevelConfig;
  const componentLevel = logLevelConfig[component];
  if (isLogLevel(componentLevel)) {
    return componentLevel;
  }

  const defaultLevel = logLevelConfig.default;
  if (isLogLevel(defaultLevel)) {
    return defaultLevel;
  }

  return "error";
}

const delegatingSink: LogSink = {
  log(level, component, message, ...args) {
    const rendered = args.length ? format(message, ...args) : String(message);

    // Push to in-memory buffer for LM tool access
    logBuffer.push({
      timestamp: new Date().toISOString(),
      level,
      component,
      message: rendered,
    });

    // Write to output channel
    const channel = outputChannel;
    if (!channel) {
      return;
    }

    channel.appendLine(formatLogLine(level, component, message, args));
  },
};

export function getLogger(component: LogComponent): Logger {
  return new Logger(component, delegatingSink, () =>
    getConfiguredLevel(component),
  );
}

export type WebviewLogMessage = {
  type: "log";
  level: LogLevel;
  component?: LogComponent;
  message: string;
  args?: unknown[];
};

export function logWebviewMessage(message: WebviewLogMessage): void {
  const component = message.component ?? "webview";
  const logger = getLogger(component);
  const args = message.args ?? [];

  switch (message.level) {
    case "error":
      logger.error(message.message, ...args);
      break;
    case "warn":
      logger.warn(message.message, ...args);
      break;
    case "info":
      logger.info(message.message, ...args);
      break;
    case "debug":
      logger.debug(message.message, ...args);
      break;
    case "trace":
      logger.trace(message.message, ...args);
      break;
    case "silent":
    default:
      break;
  }
}
