import {
  Logger,
  type LogComponent,
  type LogSink,
} from "@exosuit/core/Logger.ts";

type WebviewApi = {
  postMessage: (message: unknown) => void;
};

const noopSink: LogSink = {
  log: () => {},
};

let sink: LogSink = noopSink;

const delegatingSink: LogSink = {
  log(level, component, message, ...args) {
    sink.log(level, component, message, ...args);
  },
};

export function initializeWebviewLogger(vscodeApi: WebviewApi): void {
  sink = {
    log(level, component, message, ...args) {
      vscodeApi.postMessage({
        type: "log",
        level,
        component,
        message,
        args,
      });
    },
  };
}

export function getWebviewLogger(component: LogComponent = "webview"): Logger {
  return new Logger(component, delegatingSink, () => "trace");
}
