export const LOG_LEVELS = [
  "silent",
  "error",
  "warn",
  "info",
  "debug",
  "trace",
] as const;

export type LogLevel = (typeof LOG_LEVELS)[number];

export type LogComponent =
  | "extension"
  | "webview"
  | "bridge"
  | "lmtool"
  | "rust"
  | "core";

export interface LogSink {
  log(
    level: LogLevel,
    component: LogComponent,
    message: string,
    ...args: unknown[]
  ): void;
}

const NOOP_SINK: LogSink = {
  log: () => {},
};

const LOG_LEVEL_ORDER: Record<LogLevel, number> = {
  silent: 0,
  error: 1,
  warn: 2,
  info: 3,
  debug: 4,
  trace: 5,
};

export function isLogLevel(value: unknown): value is LogLevel {
  return (
    typeof value === "string" &&
    (LOG_LEVELS as readonly string[]).includes(value)
  );
}

export function shouldLog(level: LogLevel, configured: LogLevel): boolean {
  return LOG_LEVEL_ORDER[level] <= LOG_LEVEL_ORDER[configured];
}

export class Logger {
  constructor(
    private component: LogComponent,
    private sink: LogSink,
    private getLevel: () => LogLevel,
  ) {}

  error(message: string, ...args: unknown[]) {
    this.emit("error", message, args);
  }

  warn(message: string, ...args: unknown[]) {
    this.emit("warn", message, args);
  }

  info(message: string, ...args: unknown[]) {
    this.emit("info", message, args);
  }

  debug(message: string, ...args: unknown[]) {
    this.emit("debug", message, args);
  }

  trace(message: string, ...args: unknown[]) {
    this.emit("trace", message, args);
  }

  private emit(level: LogLevel, message: string, args: unknown[]) {
    const configuredLevel = this.getLevel();
    if (!shouldLog(level, configuredLevel)) {
      return;
    }

    this.sink.log(level, this.component, message, ...args);
  }
}

export function createNoopLogger(component: LogComponent): Logger {
  return new Logger(component, NOOP_SINK, () => "silent");
}
