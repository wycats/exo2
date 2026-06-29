type TestLogLevel = "debug" | "info" | "warn" | "error";

type LogLine = {
  level: TestLogLevel;
  message: string;
  ts: number;
};

const STREAM_LIVE = process.env.EXOSUIT_TEST_LOGS === "true";
const MAX_BUFFERED_LINES = Number.parseInt(
  process.env.EXOSUIT_TEST_LOG_BUFFER_SIZE ?? "2000",
  10,
);

const buffer: LogLine[] = [];

const formatLine = (line: LogLine) => {
  const iso = new Date(line.ts).toISOString();
  return `[${iso}] [${line.level}] ${line.message}`;
};

const record = (level: TestLogLevel, message: string) => {
  const line: LogLine = { level, message, ts: Date.now() };
  buffer.push(line);

  const max = Number.isFinite(MAX_BUFFERED_LINES) ? MAX_BUFFERED_LINES : 2000;
  if (buffer.length > max) {
    buffer.splice(0, buffer.length - max);
  }

  if (STREAM_LIVE) {
    const stream =
      level === "warn" || level === "error" ? process.stderr : process.stdout;
    stream.write(`${formatLine(line)}\n`);
  }
};

export const testLogger = {
  debug: (message: string) => record("debug", message),
  info: (message: string) => record("info", message),
  warn: (message: string) => record("warn", message),
  error: (message: string) => record("error", message),
  // Back-compat helpers for existing call sites
  log: (message: string) => record("info", message),
};

export const dumpTestLogs = (header?: string) => {
  const title = header ?? "Test logs";
  process.stderr.write(`\n--- ${title} (buffered) ---\n`);
  for (const line of buffer) {
    process.stderr.write(`${formatLine(line)}\n`);
  }
  process.stderr.write(`--- end ${title} ---\n\n`);
};

export const clearTestLogs = () => {
  buffer.length = 0;
};
