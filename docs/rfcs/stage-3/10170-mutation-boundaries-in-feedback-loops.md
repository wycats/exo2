<!-- exo:10170 ulid:01kmzxbcy5rxzf5523p5j8scpy -->

# RFC 10170: Mutation Boundaries in Feedback Loops

## Summary

Exohook distinguishes checks that observe a workspace from checks that mutate
it. That distinction lets each execution context choose an appropriate safety
boundary: continuous validation runs observation checks, explicit validation
may run the complete lane, and staged mutation can restage its lane-scoped
results without absorbing pre-existing unstaged edits.

This RFC defines that contract. It also places Exohook validation in Exo's
larger feedback model without making validation responsible for steering,
diagnostic presentation, or workflow review. Those systems meet at stable
results rather than sharing one execution mechanism.

## Motivation

A validation result describes a particular workspace state. A formatter,
codemod, or generator creates a new state. Treating both operations as generic
"checks" obscures when a result remains trustworthy and when an execution may
change files.

This matters most in feedback loops. An editor can run validation every time a
file is saved, a Git hook can validate the staged snapshot, and an explicit
command can apply repairs before a commit. Each loop needs a different answer
to the same question: is this execution observing the current state, or is it
producing the next one?

Exohook answers that question in configuration. A check declares its category,
and the invocation context decides which categories to run and whether a
mutating result should be restaged. The declaration is durable enough for the
CLI, JSONL protocol, and VS Code integration to agree on the same behavior.

## Guide-Level Explanation

### Declaring Check Behavior

Version 3 Exohook checks declare a `category`:

```toml
[check.lint]
command = "eslint --max-warnings 0"
category = "observe"

[check.format]
command = "prettier --write ."
category = "mutate"
fix_command = "prettier --write ."
```

An `observe` check reads project state and reports a result. Linters, type
checkers, and tests normally belong in this category. `observe` is the default,
so existing checks retain observation semantics when they omit the field.

A `mutate` check may change project files. Formatters, codemods, and generators
belong in this category. A `fix_command` is valid only for a mutate check,
which keeps the repair path attached to an explicit mutation declaration.

The category describes behavior rather than scheduling. A workflow may include
both categories. The caller chooses whether it wants an observation pass or an
explicit full-lane execution.

### Continuous And Explicit Validation

Continuous validation is an observation loop. VS Code invokes:

```text
exohook validate <lane> --format=jsonl --category observe
```

for both the initial continuous pass and save-triggered reruns. Exohook applies
the category selector before fileset discovery and command planning, so mutate
checks do not acquire work or execute during that pass.

Manual Test Explorer runs and ordinary `exohook validate <lane>` invocations
omit the selector and execute the lane as configured. This gives users and
agents an explicit path for workflows that intentionally include mutation.
The JSONL discovery stream carries each check's category, allowing VS Code to
use observe checks when deciding which lanes a saved file affects.

### Hook And Workflow Context

The category also informs command selection. Mutation checks may use their fix
command when the execution context permits repair. Interactive pre-commit
execution and manual workflows select the fix command by default when one is
available; pre-push and other non-interactive hook contexts select the primary
command unless policy explicitly requests repair. Category continues to
describe the check's behavior whichever command the context selects.

Workflow `fix_policy` remains independent from category. Category answers what
a check can do. Policy and invocation context answer which command Exohook
chooses for this run.

### Staged Mutation

Pre-commit validation may apply a mutation to the staged snapshot and restage
the result. Exohook gives this path a narrower boundary than ordinary lane
parallelism.

When a parallel lane uses staged scope, checks with both `category = "mutate"`
and automatic restaging execute sequentially after the lane's parallel-safe
checks. Before restaging, Exohook requires lane-scoped files to be free of
unstaged changes. This prevents the restage step from folding unrelated local
edits into the index.

After the command runs, Exohook compares the worktree with its pre-command
state and restages changed lane-scoped files. Version 3 plans currently use
containment-off semantics: changes outside the lane remain visible in the
worktree, and Exohook neither restages them nor promotes them to a warning or
failure. The earlier configuration model supports warn and fail containment
overrides; the V3 schema does not yet expose that policy.

This is a staged-restage guarantee, not a global transaction over every
validation command. Ordinary observation checks retain the lane's configured
parallelism, while explicit mutations outside this path remain visible
filesystem operations.

## Reference-Level Explanation

### Category Selection

`exohook validate` accepts an optional V3 category selector:

```text
exohook validate <lane> --category <observe|mutate>
```

The selector filters resolved checks before Exohook computes filesets, invokes
tools, or creates command plans. Without the selector, validation includes all
checks in workflow order. Category selection requires a version 3
configuration because earlier schemas do not carry this distinction.

Hook execution uses the same resolved category metadata. A category filter can
therefore describe a coherent pass whether the lane is reached as a named V3
workflow or through its configured hook name.

### Machine-Readable Feedback

Exohook discovery emits suite and check records over JSONL. Check records carry
the lane, command metadata, filters, and category. Validation emits lane and
check lifecycle events, output, completion status, matched-file information,
and restage failures.

VS Code consumes these events to build Test Explorer items and settle their
results. It retains category and filter metadata from discovery, then uses the
saved file's path to select affected lanes for continuous execution. Exohook
remains authoritative for the final category filter when the selected lane
runs.

The Test Explorer is a presentation of Exohook results. Exohook failures do
not currently become entries in VS Code's diagnostic collection, and the
controller does not publish a shared validation snapshot into Exo state.

### Relationship To Exo Steering

Exo commands provide contextual steering and completion review around durable
project work. Those command boundaries complement Exohook's validation
boundary: validation produces evidence about a stable workspace state, while
Exo records progress and decides when a task or goal has a sufficient outcome.

The systems remain independently useful. Running an Exohook lane does not
implicitly complete Exo work, and an Exo command does not acquire control of a
running mutation. Callers compose them by validating a stable state and then
recording the resulting evidence through the Exo workflow.

## Drawbacks

Category is a behavioral promise made by configuration. Exohook cannot prove
that an observe command leaves the filesystem unchanged, so an incorrectly
classified command can still violate the continuous-run expectation.

Filtering at check granularity also means a continuous pass may represent only
part of a lane. The Test Explorer preserves the lane-shaped presentation, but
mutate checks settle only when an explicit run includes them. Interfaces that
want to display this distinction more prominently can build on the discovery
category.

Finally, staged restaging protects the index from pre-existing unstaged lane
edits, but V3 does not contain or roll back a command's filesystem effects
outside the lane.

## Alternatives

One alternative is to maintain separate continuous and explicit workflows.
That can express the same behavior, but it duplicates lane membership and lets
the two definitions drift. Category selection keeps one workflow authoritative
while allowing callers to request a safe observational subset.

Another alternative is to infer mutation from command names or the presence of
a fix command. Command names are conventions rather than contracts, and some
mutating checks use their primary command. An explicit category is readable by
people and stable across adapters.

A global validation lock would provide a broader exclusion boundary. The
current design instead serializes the staged mutations whose restage behavior
requires ordering and lets observation checks retain useful parallelism.

## Future Extensions

The category model provides a foundation for richer interaction without making
those features part of the current Candidate contract. VS Code can present
mutate checks as explicit actions, validation failures can participate in a
unified diagnostic model, and a repair action can request confirmation before
it changes files. A future observation service can also publish validation
freshness into Exo's shared perception model and schedule re-observation after
an accepted mutation.

Those extensions will need to define their own authority, freshness, and
failure semantics. This RFC supplies the distinction they can rely on:
observation reports a state, mutation creates a state, and callers choose the
boundary appropriate to their feedback loop.

The V3 schema can also grow an explicit containment policy for automatic
restaging. That extension would let projects promote outside-lane mutations to
warnings or failures while preserving today's containment-off default.

## Implementation Evidence

The Stage 3 contract is implemented across Exohook and the VS Code extension:

- `CheckCategory`, `CheckV3`, and `ExecutionContext` define category and command
  selection.
- Validation filters resolved V3 checks by category and gives staged
  mutate-and-restage checks sequential execution with index safeguards.
- Discovery and validation JSONL events carry the metadata used by Test
  Explorer.
- VS Code uses observe-only validation for continuous initial and save-triggered
  runs while preserving complete manual lane execution.
- Focused tests cover category parsing and defaults, execution-context command
  selection, staged restaging, category-filtered execution, and the VS Code
  invocation and finalization boundaries.

## Related RFCs

- RFC 0081, *Exohook: File Expansion Worked Examples*, describes the fileset
  behavior used to build check plans.
- RFC 0113, *Exohook Machine Channel Protocol*, introduces machine-readable
  discovery and validation events.
- RFC 0122, *Exohook Streaming Progress Reporting*, defines the streaming
  progress model consumed by interactive clients.
- RFC 00224, *The SOAR Loop*, describes the larger project workflow in which
  validation evidence informs orientation and review.
- RFC 00225, *Problems Pane Integration with SOAR Loop*, explores the future
  diagnostic presentation boundary.
- RFCs 10181, 10182, and 10183 develop shared perception, contextual steering,
  and activity evidence as adjacent Exo contracts.
