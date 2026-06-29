#!/usr/bin/env node

import { readFileSync } from "node:fs";

const workflowPath = ".github/workflows/rust-coverage-audit.yml";
const workflow = readFileSync(workflowPath, "utf8");
const failures = [];

function fail(message) {
  failures.push(message);
}

function job(name) {
  const match = workflow.match(
    new RegExp(
      `^  ${name}:\\n([\\s\\S]*?)(?=^  [a-z0-9][a-z0-9-]*:\\n|(?![\\s\\S]))`,
      "m",
    ),
  );
  if (!match) {
    fail(`missing job: ${name}`);
    return "";
  }
  return match[1];
}

function requireText(subject, text, message) {
  if (!subject.includes(text)) fail(message);
}

function rejectText(subject, text, message) {
  if (subject.includes(text)) fail(message);
}

const executionJobs = [
  "pr-coverage",
  "manual-pr-coverage",
  "main-coverage",
  "manual-ref-coverage",
];
const reporterJobs = [
  "pr-coverage-report",
  "manual-pr-coverage-report",
  "main-coverage-report",
];

for (const name of executionJobs) {
  const body = job(name);
  requireText(body, "contents: read", `${name} must have contents: read`);
  requireText(body, "cargo llvm-cov", `${name} must execute coverage`);
  for (const permission of ["contents", "issues", "pull-requests", "statuses"]) {
    rejectText(body, `${permission}: write`, `${name} must not have ${permission}: write`);
  }
}

for (const name of reporterJobs) {
  rejectText(job(name), "cargo llvm-cov", `${name} must not execute repository coverage code`);
}

for (const [name, result] of [
  ["pr-coverage-report", "needs.pr-coverage.result"],
  ["manual-pr-coverage-report", "needs.manual-pr-coverage.result"],
  ["main-coverage-report", "needs.main-coverage.result"],
]) {
  const body = job(name);
  requireText(body, `${result} == 'success'`, `${name} must report successful execution`);
  requireText(body, `${result} == 'failure'`, `${name} must report failed execution`);
  rejectText(body, `${result} == 'cancelled'`, `${name} must omit cancelled execution`);
  rejectText(body, `${result} == 'skipped'`, `${name} must omit skipped execution`);
}

requireText(
  job("pr-coverage-report"),
  "ref: ${{ github.event.pull_request.base.sha }}",
  "automatic PR reporter must use the trusted base SHA",
);
requireText(
  job("manual-pr-coverage-report"),
  "ref: ${{ github.event.repository.default_branch }}",
  "manual PR reporter must use the trusted default branch",
);
requireText(
  job("main-coverage-report"),
  "ref: ${{ github.event.repository.default_branch }}",
  "main reporter must use the trusted default branch",
);
requireText(
  job("main-coverage-report"),
  "gh auth setup-git",
  "main reporter must configure trusted Git credentials",
);

for (const name of ["manual-pr-coverage", "manual-ref-coverage"]) {
  rejectText(job(name), "actions/cache", `${name} must not use a shared dependency cache`);
}

requireText(
  job("manual-pr-metadata"),
  ".head.sha",
  "manual PR metadata must resolve the exact head SHA",
);
rejectText(
  job("manual-pr-metadata"),
  "actions/checkout",
  "manual PR metadata must not check out repository code",
);
requireText(
  job("manual-pr-coverage"),
  "git rev-parse HEAD",
  "manual PR execution must verify its checked-out SHA",
);
requireText(
  job("manual-pr-coverage"),
  "EXPECTED_HEAD_SHA",
  "manual PR execution must compare against trusted metadata",
);
requireText(
  job("main-coverage"),
  "tested_sha: ${{ steps.tested-sha.outputs.sha }}",
  "main execution must publish its exact tested SHA",
);
requireText(
  job("main-coverage"),
  "ref: ${{ github.event.repository.default_branch }}",
  "main execution must resolve the default branch independently of the dispatch ref",
);
requireText(
  job("main-coverage-report"),
  "COVERAGE_MAIN_SHA: ${{ needs.main-coverage.outputs.tested_sha }}",
  "main reporter must receive the exact tested SHA",
);
requireText(
  job("pr-coverage-report"),
  "COVERAGE_TESTED_SHA: ${{ github.event.pull_request.head.sha }}",
  "automatic PR reporter must receive the tested SHA",
);
requireText(
  job("manual-pr-coverage-report"),
  "COVERAGE_TESTED_SHA: ${{ needs.manual-pr-metadata.outputs.head_sha }}",
  "manual PR reporter must receive the tested SHA",
);
requireText(
  job("pr-coverage-report"),
  "group: rust-coverage-pr-${{ github.event.pull_request.number }}",
  "automatic PR reporter must share the executor concurrency group",
);
requireText(
  job("manual-pr-coverage-report"),
  "group: rust-coverage-pr-${{ needs.manual-pr-metadata.outputs.pr_number }}",
  "manual PR reporter must share the executor concurrency group",
);
requireText(
  job("main-coverage-report"),
  "group: rust-coverage-main-audit",
  "main reporter must share the executor concurrency group",
);

const checkoutSteps = workflow
  .split(/(?=^      - )/m)
  .filter((step) => /^      - /m.test(step))
  .filter((step) => step.includes("uses: actions/checkout@"));
if (checkoutSteps.length === 0) fail("workflow must contain checkout steps");
for (const step of checkoutSteps) {
  const name =
    step.match(/^      - name: (.+)$/m)?.[1] ??
    step.match(/^      - uses: (.+)$/m)?.[1] ??
    "unnamed checkout";
  requireText(
    step,
    "persist-credentials: false",
    `${name} must set persist-credentials: false`,
  );
}

if (failures.length > 0) {
  console.error("Rust coverage workflow security contract failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log("Rust coverage workflow security contract passed.");
