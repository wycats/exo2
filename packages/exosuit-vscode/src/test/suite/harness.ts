type TestHandler = () => unknown | Promise<unknown>;
type SuiteHandler = () => void;

interface TestCase {
  name: string;
  handler: TestHandler;
  timeoutMs?: number;
  skipped: boolean;
}

interface SuiteCase {
  name: string;
  parent?: SuiteCase;
  suites: SuiteCase[];
  tests: TestCase[];
  beforeEachHandlers: TestHandler[];
  afterEachHandlers: TestHandler[];
  skipped: boolean;
}

export interface HarnessSummary {
  passed: number;
  failed: number;
  skipped: number;
}

export interface DescribeFn {
  (name: string, handler: SuiteHandler): void;
  skip(name: string, handler: SuiteHandler): void;
}

export interface ItFn {
  (name: string, handler: TestHandler, timeoutMs?: number): void;
  skip(name: string, handler: TestHandler, timeoutMs?: number): void;
}

const rootSuite: SuiteCase = {
  name: "",
  suites: [],
  tests: [],
  beforeEachHandlers: [],
  afterEachHandlers: [],
  skipped: false,
};

let currentSuite = rootSuite;

function createSuite(
  name: string,
  handler: SuiteHandler,
  skipped: boolean,
): void {
  const suite: SuiteCase = {
    name,
    parent: currentSuite,
    suites: [],
    tests: [],
    beforeEachHandlers: [],
    afterEachHandlers: [],
    skipped,
  };
  currentSuite.suites.push(suite);

  const previous = currentSuite;
  currentSuite = suite;
  try {
    handler();
  } finally {
    currentSuite = previous;
  }
}

function createTest(
  name: string,
  handler: TestHandler,
  timeoutMs: number | undefined,
  skipped: boolean,
): void {
  currentSuite.tests.push({ name, handler, timeoutMs, skipped });
}

async function withTimeout<T>(
  label: string,
  timeoutMs: number | undefined,
  callback: () => Promise<T>,
): Promise<T> {
  if (!timeoutMs) {
    return callback();
  }

  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      callback(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          reject(new Error(`${label} timed out after ${timeoutMs}ms`));
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}

function testPath(suite: SuiteCase, test: TestCase): string {
  const names: string[] = [test.name];
  let current: SuiteCase | undefined = suite;
  while (current && current.name.length > 0) {
    names.unshift(current.name);
    current = current.parent;
  }
  return names.join(" > ");
}

async function runSuite(
  suite: SuiteCase,
  inheritedBeforeEach: TestHandler[],
  inheritedAfterEach: TestHandler[],
  inheritedSkipped: boolean,
): Promise<HarnessSummary> {
  const summary: HarnessSummary = { passed: 0, failed: 0, skipped: 0 };
  const skipped = inheritedSkipped || suite.skipped;
  const beforeEachHandlers = [
    ...inheritedBeforeEach,
    ...suite.beforeEachHandlers,
  ];
  const afterEachHandlers = [...suite.afterEachHandlers, ...inheritedAfterEach];

  for (const child of suite.suites) {
    const childSummary = await runSuite(
      child,
      beforeEachHandlers,
      afterEachHandlers,
      skipped,
    );
    summary.passed += childSummary.passed;
    summary.failed += childSummary.failed;
    summary.skipped += childSummary.skipped;
  }

  for (const test of suite.tests) {
    const name = testPath(suite, test);
    if (skipped || test.skipped) {
      summary.skipped += 1;
      process.stdout.write(`- ${name}\n`);
      continue;
    }

    try {
      await withTimeout(name, test.timeoutMs, async () => {
        for (const handler of beforeEachHandlers) {
          await handler();
        }
        try {
          await test.handler();
        } finally {
          for (const handler of afterEachHandlers) {
            await handler();
          }
        }
      });
      summary.passed += 1;
      process.stdout.write(`✓ ${name}\n`);
    } catch (error) {
      summary.failed += 1;
      process.stderr.write(`✗ ${name}\n`);
      process.stderr.write(
        error instanceof Error
          ? `${error.stack ?? error.message}\n`
          : `${String(error)}\n`,
      );
    }
  }

  return summary;
}

export const describe: DescribeFn = Object.assign(
  (name: string, handler: SuiteHandler) => {
    createSuite(name, handler, false);
  },
  {
    skip: (name: string, handler: SuiteHandler) => {
      createSuite(name, handler, true);
    },
  },
);

export const it: ItFn = Object.assign(
  (name: string, handler: TestHandler, timeoutMs?: number) => {
    createTest(name, handler, timeoutMs, false);
  },
  {
    skip: (name: string, handler: TestHandler, timeoutMs?: number) => {
      createTest(name, handler, timeoutMs, true);
    },
  },
);

export function beforeEach(handler: TestHandler): void {
  currentSuite.beforeEachHandlers.push(handler);
}

export function afterEach(handler: TestHandler): void {
  currentSuite.afterEachHandlers.push(handler);
}

export function installHarnessGlobals(): void {
  globalThis.describe = describe;
  globalThis.it = it;
  globalThis.beforeEach = beforeEach;
  globalThis.afterEach = afterEach;
}

export async function runHarness(): Promise<HarnessSummary> {
  return runSuite(rootSuite, [], [], false);
}
