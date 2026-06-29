import { describe, expect, it } from "vitest";

import {
  formatCallResult,
  formatErrorResponse,
  formatMachineChannelResponse,
} from "../lmtool/exo-run";
import { WORKFLOW_COMPLETION_CONFIRMATION_KIND } from "../types/machineChannel";

describe("exo-run fallback formatting", () => {
  it("distinguishes abandoned goals from pending goals", () => {
    const text = formatCallResult({
      kind: "goal.list",
      ok: true,
      goals: [
        { id: "pending-goal", label: "Pending", status: "pending" },
        { id: "abandoned-goal", label: "Abandoned", status: "abandoned" },
        { id: "completed-goal", label: "Completed", status: "completed" },
      ],
    });

    expect(text).toContain("⏳ pending-goal — Pending");
    expect(text).toContain("⛔ abandoned-goal — Abandoned");
    expect(text).toContain("✅ completed-goal — Completed");
  });

  it("formats completion digests without collapsing claim subjects or bodies", () => {
    const text = formatCallResult({
      steering: {
        completion_digests: [
          {
            entity_type: "goal",
            entity_id: "digest-goal",
            count: 2,
            drill_in:
              "exo inbox list --entity-type goal --entity-id digest-goal",
            claims: [
              {
                id: "claim-1",
                status: "pending",
                source: "user-feedback",
                priority: "next-touch",
                subject: "First outcome subject",
                body: "First outcome body",
                created: "2026-04-27T00:00:00Z",
              },
              {
                id: "claim-2",
                status: "pending",
                source: "user-feedback",
                priority: "next-touch",
                subject: "Second outcome subject",
                body: "Second outcome body",
                created: "2026-04-27T00:01:00Z",
              },
            ],
          },
        ],
      },
    });

    expect(text).toContain("Completed outcomes to review:");
    expect(text).toContain("First outcome subject");
    expect(text).toContain("First outcome body");
    expect(text).toContain("Second outcome subject");
    expect(text).toContain("Second outcome body");
    expect(text).not.toContain("sample_subject");
  });

  it("surfaces workflow confirmation errors without process vocabulary", () => {
    const formatted = formatErrorResponse({
      protocol_version: 1,
      id: "test",
      status: "error",
      error: {
        code: "precondition_failed",
        message:
          "Cannot complete goal 'demo': The outcome still needs human review. Ask the human to review the completed outcome before recording it.",
        details: {
          entity_type: "goal",
          entity_id: "demo",
          blocked_state: "The outcome still needs human review.",
          workflow_confirmation: {
            kind: "workflow_completion_confirmation",
            entity_type: "goal",
            entity_id: "demo",
            completion_input: {
              kind: "workflow_completion_confirmation",
              entity_type: "goal",
              entity_id: "demo",
              decision: "yes_complete",
              outcome: "Done",
            },
            completion_digest: {
              entity_type: "goal",
              entity_id: "demo",
              count: 1,
              drill_in: "exo inbox list --entity-type goal --entity-id demo",
              claims: [
                {
                  id: "claim-1",
                  status: "pending",
                  source: "user-feedback",
                  priority: "next-touch",
                  subject: "Reviewed feature shipped",
                  body: "The user-facing flow now completes successfully.",
                  created: "2026-04-27T00:00:00Z",
                },
              ],
            },
            header: "Review completed outcome",
            question: "Does this outcome look right?",
            message:
              "All child tasks are complete. The remaining step is your review of the outcome.\n\nProposed outcome: Done",
            readiness_rationale:
              "All child tasks are complete. The remaining step is your review of the outcome.",
            proposed_outcome: "Done",
            options: [
              {
                label: "Looks right — record it",
                value: "yes_complete",
                description: "Record this outcome and close the goal.",
              },
              {
                label: "Revise the outcome",
                value: "revise_outcome",
                description:
                  "Edit the outcome summary before completing the goal.",
              },
              {
                label: "Keep working",
                value: "not_complete_yet",
                description: "Leave the goal pending and continue work.",
              },
              {
                label: "Discuss first",
                value: "discuss",
                description: "Pause completion and discuss what is missing.",
              },
            ],
            branch_instructions: {
              yes_complete: "Record this outcome and close the goal.",
              revise_outcome:
                "Ask for the revised outcome summary, then rerun goal complete with the revised --log value.",
              not_complete_yet:
                "Leave the goal pending. If there is remaining work, add or update tasks before trying again.",
              discuss:
                "Stop and discuss what is missing before changing goal state.",
            },
          },
        },
      },
    });

    expect(formatted.workflowConfirmation?.header).toBe(
      "Review completed outcome",
    );
    expect(formatted.text).toContain("Review outcome.");
    expect(formatted.text).not.toContain("Entity: goal demo");
    expect(formatted.text).toContain("Approve recording this outcome?");
    expect(formatted.text).toContain("Outcome:\nDone");
    expect(formatted.text).toContain("Verification / evidence:");
    expect(formatted.text).toContain("Follow-up to record:");
    expect(formatted.text).toContain(
      "State “None” if no follow-up remains, or name the tracked next task.",
    );
    expect(formatted.text).toContain("Ask: “Record this goal outcome?”");
    expect(formatted.text).toContain(
      "Options: Looks right — record it — Record this outcome and close the goal. / Revise the outcome — Edit the outcome summary before completing the goal.",
    );
    expect(formatted.text).toContain(
      "Keep working — Leave the goal pending and continue work.",
    );
    expect(formatted.text).toContain(
      "Discuss first — Pause completion and discuss what is missing.",
    );
    expect(formatted.text).toContain("Once approved, finish the goal with the approved outcome.");
    expect(formatted.workflowConfirmation?.completion_input?.decision).toBe(
      "yes_complete",
    );
    expect(formatted.text).toContain("Reviewed feature shipped");
    expect(formatted.text).toContain(
      "The user-facing flow now completes successfully.",
    );
    expect(formatted.text).not.toContain("Error: Cannot complete goal");
    expect(formatted.text).not.toContain("## Review completed outcome");
    expect(formatted.text).not.toContain("Does this outcome look right?");
    expect(formatted.text).not.toContain(
      "All child tasks are complete. The remaining step is your review of the outcome.",
    );
    expect(formatted.text).not.toContain("Proposed outcome:");
    expect(formatted.text).not.toContain("- Looks right — record it");
    expect(formatted.text).not.toContain("- Revise the outcome");
    expect(formatted.text).not.toContain("- Keep working");
    expect(formatted.text).not.toContain("- Discuss first");
    expect(formatted.text).not.toContain("Completed outcomes to review:");
    expect(formatted.text).not.toContain("claim-1");
    expect(formatted.text).not.toContain("exo inbox list");
    expect(formatted.text).not.toContain("Branch handling:");
    expect(formatted.text).not.toContain("workflowConfirmation");
    expect(formatted.text).not.toContain('"decision":"yes_complete"');
    expect(formatted.text).not.toContain('"entityType":"goal"');
    expect(formatted.text).not.toContain("workflow_confirmation");
    expect(formatted.text).not.toContain("completion_input");
    expect(formatted.text).not.toContain("blocked_state");
    expect(formatted.text).not.toContain("entity_type");
    expect(formatted.text).not.toContain("entity_id");
    expect(formatted.text).not.toContain("inbox");
    expect(formatted.text).not.toContain("acknowledged");
  });

  it("keeps workflow confirmation retry data hidden in the data part", () => {
    const result = formatMachineChannelResponse(
      {
        protocol_version: 1,
        id: "test",
        status: "error",
        error: {
          code: "precondition_failed",
          message: "Cannot complete task 'demo-task': needs review.",
          details: {
            workflow_confirmation: {
              kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
              entity_type: "task",
              entity_id: "demo-task",
              completion_input: {
                kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
                entity_type: "task",
                entity_id: "demo-task",
                decision: "yes_complete",
                outcome: "Task done",
              },
              header: "Review completed outcome",
              question: "Does this outcome look right?",
              message: "Proposed outcome: Task done",
              readiness_rationale: "Ready for review.",
              proposed_outcome: "Task done",
              options: [
                {
                  label: "Looks right — record it",
                  value: "yes_complete",
                },
              ],
            },
          },
        },
      },
      false,
    );

    const [textPart, dataPart] = result.content as Array<{ value: unknown }>;
    expect(textPart?.value).toContain("Review outcome.");
    expect(textPart?.value).toContain("Options: Looks right — record it");
    expect(textPart?.value).not.toContain("Revise outcome");
    expect(textPart?.value).not.toContain("yes_complete");
    expect(textPart?.value).not.toContain("workflowConfirmation");
    expect(textPart?.value).not.toContain("workflow_confirmation");
    expect(dataPart?.value).toEqual({
      workflow_confirmation: {
        kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
        entity_type: "task",
        entity_id: "demo-task",
        completion_input: {
          kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
          entity_type: "task",
          entity_id: "demo-task",
          decision: "yes_complete",
          outcome: "Task done",
        },
        header: "Review completed outcome",
        question: "Does this outcome look right?",
        message: "Proposed outcome: Task done",
        readiness_rationale: "Ready for review.",
        proposed_outcome: "Task done",
        options: [
          {
            label: "Looks right — record it",
            value: "yes_complete",
          },
        ],
      },
      workflowConfirmation: {
        kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
        entityType: "task",
        entityId: "demo-task",
        decision: "yes_complete",
        outcome: "Task done",
      },
    });
  });
});
