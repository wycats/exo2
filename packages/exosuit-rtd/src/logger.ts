export type RtdLogger = {
  info: (message: string, ...args: unknown[]) => void;
  warn: (message: string, ...args: unknown[]) => void;
  error: (message: string, ...args: unknown[]) => void;
};

const noopLogger: RtdLogger = {
  info: () => {},
  warn: () => {},
  error: () => {},
};

let activeLogger: RtdLogger = noopLogger;

export function setLogger(logger: Partial<RtdLogger>): void {
  activeLogger = { ...noopLogger, ...logger };
}

export function getLogger(): RtdLogger {
  return activeLogger;
}
