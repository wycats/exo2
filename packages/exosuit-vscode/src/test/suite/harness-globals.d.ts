type ExosuitHarnessTestHandler = () => unknown | Promise<unknown>;
type ExosuitHarnessSuiteHandler = () => void;

declare var describe: {
  (name: string, handler: ExosuitHarnessSuiteHandler): void;
  skip(name: string, handler: ExosuitHarnessSuiteHandler): void;
};

declare var it: {
  (name: string, handler: ExosuitHarnessTestHandler, timeoutMs?: number): void;
  skip(
    name: string,
    handler: ExosuitHarnessTestHandler,
    timeoutMs?: number,
  ): void;
};

declare function beforeEach(handler: ExosuitHarnessTestHandler): void;
declare function afterEach(handler: ExosuitHarnessTestHandler): void;
