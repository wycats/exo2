<!-- exo:10194 ulid:01ktwrvep5dqv31t38ys4sf11k -->

# RFC 10194: Command Surface Coherence and the Shared Exo Command Language

**Status**: Proposal
**Feature**: mcp

## Summary

Exo should keep one shared command language across the terminal `exo` CLI, VS
Code language-model tools, MCP `exo-run`, near-term wrapper tools such as
`exo-help`, Exo-authored steering suggestions, recovery text, plugin skills,
cockpit actions, and future host adapters.

That command language is intentionally CLI-like, but it must not become a
near-copy maintained by spot fixes. The CLI-like text agents see should be a
rendered projection of the same `ExoSpec`/`CommandSpec` model that dispatches
terminal commands, MCP calls, help, effects, and confirmations.

The first implementation boundary remains a syntax frontend. The frontend
accepts either terminal argv tokens or MCP command text plus placeholder
arguments, and returns command tokens plus intent:

- `call`: run the parsed command tokens through normal Exo dispatch;
- `help`: render help for the parsed target;
- `syntax error`: reject non-Exo command text before semantic dispatch.

The frontend owns syntax only. `ExoSpec` and `CommandSpec` remain the semantic
authority for command paths, argument schemas, effects, machine-channel
addresses, help metadata, typed command references, and dispatch.

The broader design goal is command surface coherence: every Exo-authored
command reference should be structured or spec-derived internally, validated
against `CommandSpec`, and rendered into CLI-like text only at the boundary
where a human or agent needs to read or invoke it.

The current recommendation is to proceed with a small shared custom
`command_text` frontend and a typed command-reference layer. A parser library
may still be useful later, but only as implementation machinery behind that
boundary. It must not become the command model or a second source of truth.

## Motivation

Exo deliberately presents a CLI-like command surface to agents:

```text
exo-run("status")
exo-run("help task")
exo-run("task complete parser-frontend-recon-and-spike --log $1", ["..."])
```

That surface works because agents already understand CLI shapes. It builds
trust when `exo-run("task complete foo --log $1")` means the same thing as
`exo task complete foo --log ...` in a terminal. It undermines trust when MCP,
CLI, steering text, recovery hints, plugin skills, and cockpit actions are only
approximately aligned.

The command shape mismatches that keep recurring are not isolated typos. For
example, an Exo-authored or Exo-taught suggestion such as
`task complete <id> --message ...` can drift from the actual supported
`--log` flag when suggestions are hand-coded as strings instead of projected
from `CommandSpec`. Agents then learn an invalid Exo language from Exo itself,
and every surface becomes less credible.

The shared command language still avoids the worst parts of a raw shell runner.
It can reject pipes, redirects, environment assignment prefixes, glob
expansion, and command substitution before execution. It can also preserve
Exo-specific affordances such as placeholder arguments, help intent, workflow
confirmations, effect budgets, and steering.

The implementation has drifted in two related ways:

- terminal `exo` argv routing strips global flags and handles help forms;
- MCP `exo-run` tokenizes command strings and substitutes `$1`, `$2`, etc.;
- machine-channel help and call paths route through `CommandSpec`;
- infrastructure commands such as `init`, `mcp serve`, `daemon ensure`,
  `json server`, `merge-driver`, and `validate` are special-cased outside the
  generated registry;
- steering suggestions, completion prompts, recovery hints, skills, docs, and
  cockpit affordances can still contain hand-authored command strings.

That drift produced real behavior gaps. For example, `exo init --help` could
fall back to root help because global `--help` handling ran before special
command dispatch, while `init` was not represented in the generated
`CommandSpec` registry. Separately, status and recovery text can suggest
command shapes that are no longer accepted by the current parser.

The goal is not to make Exo more shell-compatible. The goal is to make Exo's
CLI-like command language explicit enough that CLI, MCP, VS Code, steering,
help, recovery, cockpit, plugin, and future wrapper surfaces share the same
syntax and semantics while delegating command meaning to
`ExoSpec`/`CommandSpec`.

## Design Principle

All Exo command surfaces must serve `ExoSpec`, not compete with it.

This RFC inherits the central lesson from RFC 00233: command semantics drift
when Exo maintains multiple independent command-definition systems. RFC 10200
applies that lesson to `exo-run`: the MCP tool is a transport for the Exo
command language, not a second API. RFC 0200 shows the same pressure from the
agent side: inconsistent command shapes make agents guess wrong.

Parser libraries, rendered command strings, plugin skill examples, cockpit
actions, and steering suggestions are acceptable only if they are projections
of the command model. They must not become hidden command-definition systems.

It is not acceptable for any command surface to become:

- the source of command paths;
- the source of argument metadata;
- the source of effect metadata;
- the source of help text;
- the authority for machine-channel addresses;
- the place where agent-facing command semantics are hidden;
- the place where Exo-authored command suggestions are hand-maintained as raw
  strings.

## Stage 1 Proposal

The committed direction is that every Exo command surface is a projection of
one command language:

- `exo`, MCP `exo-run`, wrapper tools, VS Code tools, steering suggestions,
  recovery hints, plugin guidance, and cockpit actions must use the same
  command syntax and command semantics.
- CLI-like command text remains the user-facing and agent-facing affordance,
  but it is rendered from structured command data rather than maintained as the
  source of truth.
- Exo-authored command guidance must be typed or validated against
  `CommandSpec` before it is emitted.
- Raw command strings may remain only for human-authored prose, temporary
  legacy compatibility, or external commands that are not Exo invocations.

This RFC therefore has two implementation tracks:

1. `command_text`: the shared syntax frontend for CLI-like invocation text.
2. `CommandReference`: the authoring and rendering model for Exo-authored
   command guidance.

`CommandReference` should be smaller than a fully constructed command object.
It names an Exo command address, supplies argument values or placeholders, and
records presentation intent. It compiles to `Invocation` for validation and
dispatch compatibility, then renders to terminal CLI text or `exo-run` command
text at the edge.

For this RFC, "Exo-authored command guidance" includes any command string
emitted by the Exo runtime, generated Exo help or docs, packaged plugin skills,
cockpit APIs, machine-channel steering, recovery hints, or workflow
continuation surfaces. Hand-written RFC prose and examples are exempt unless
Exo packages or generates them as operational guidance.

Infrastructure commands remain in scope. If a command cannot yet be represented
by project `CommandSpec` because it runs before normal context loading, it
should be represented by a small infrastructure spec/table with the same
validation and rendering expectations until it can converge with the main spec.

## Proposed Architecture

```text
Terminal argv ─┐
               ├─ command_text frontend ── tokens + intent ─┐
MCP text + args┘                                             │
                                                             ▼
                                                ExoSpec / CommandSpec
                                                             ▲
                                                             │
Exo-authored suggestions ── typed command references ─────────┘
recovery text / cockpit actions / plugin skills

                                                             │
                                                             ▼
                                  dispatch / help / effects / rendered text
```

The frontend should expose a small stable API:

```rust
enum CommandTextIntent {
    Call,
    Help { target: Vec<String> },
}

struct ParsedCommandText {
    tokens: Vec<String>,
    intent: CommandTextIntent,
}
```

The frontend is responsible for:

- tokenizing MCP command text with Exo quoting rules;
- substituting placeholder arguments such as `$1`;
- preserving terminal argv tokens without pretending they are shell text;
- rejecting unsupported shell syntax before dispatch;
- recognizing equivalent help forms such as `task --help`, `task help`, and
  `help task`;
- recognizing explicit JSON help requests such as
  `rfc promote --help --format json`;
- stripping global flags only when the frontend is interpreting help intent.

The frontend is not responsible for:

- deciding whether `task complete` is a real command;
- knowing whether a flag exists for an operation;
- classifying effects;
- choosing a machine-channel address;
- constructing typed command structs;
- deciding command namespace aliases beyond syntax-only help forms.

Those remain `CommandSpec`/`ExoSpec` responsibilities.

## Typed Command References

Exo-authored command suggestions should be represented internally as structured
command references or invocations, not raw strings. A command reference names a
command address, supplies argument values or placeholders, and records the
presentation context that will render it.

Examples of surfaces that should use typed command references:

- `→ Next:` steering suggestions in command output;
- error recovery hints;
- workflow confirmation follow-up commands;
- dogfood and status repair actions;
- plugin skill examples that Exo packages or generates;
- cockpit buttons or menu actions that map to Exo commands;
- docs or help snippets generated from Exo state.

The rendered CLI-like text remains important. Agents and humans should still
see familiar forms such as:

```text
exo-run("task complete parser-frontend-recon-and-spike --log $1", ["..."])
exo task complete parser-frontend-recon-and-spike --log "..."
```

But those strings should be produced by a renderer after the reference has
been validated against `CommandSpec`. They should not be the source of truth.

This is the direct answer to recurring `--message`/`--log` style drift. The
problem is not that an agent guessed a flag once. The problem is that Exo still
has enough hand-coded command prose that invalid command shapes can escape into
trusted guidance. A typed command-reference layer turns those mistakes into
compile-time, test-time, or fixture-time failures instead of user-facing tool
failures.

The minimum implementation target is:

1. inventory Exo-authored command strings across steering, status, recovery,
   skills, docs, tests, and cockpit code;
2. introduce a typed builder for common suggested actions;
3. validate or compile each suggestion against `CommandSpec`;
4. render CLI and `exo-run` strings only from validated references;
5. add drift tests that fail when rendered suggestions name unknown commands,
   flags, or argument shapes.

Free-form prose can still mention commands manually when it is truly
human-authored documentation. Exo-authored, Exo-packaged, or Exo-generated
guidance should use the typed path.

## Implementation Roadmap

The Stage 1 implementation should proceed in three slices.

### Slice 1: Inventory And Validation Harness

Inventory existing Exo-authored command guidance and classify it as:

- Exo command guidance that must become typed or validated;
- infrastructure command guidance that needs the interim infrastructure table;
- human-authored prose that can remain raw text;
- non-Exo shell or external-tool examples outside this RFC's authority.

A first repository scan found roughly 154 likely command-guidance sites across
Rust, VS Code, cockpit, and plugin surfaces. The first harness should validate
rendered Exo suggestions against `CommandSpec` where possible and report the
remaining infrastructure or external-command exceptions explicitly.

### Slice 2: Typed Builders And Renderers

Introduce `CommandReference` builders for the highest-churn guidance surfaces:

- `SuggestedAction`;
- router suggestions;
- failure and recovery hints;
- workflow confirmation continuations;
- dogfood and status repair actions.

Each builder should render both terminal CLI text and MCP `exo-run` text from
the same reference. Placeholder arguments such as `$1` remain part of the
rendering layer, not independent command semantics.

### Slice 3: Drift Tests

Add tests that fail when Exo-generated command guidance names unknown commands,
unknown flags, or invalid argument shapes. The tests should cover both rendered
CLI strings and rendered `exo-run` strings, and should include a regression for
the `task complete <id> --message ...` class of mismatch by proving generated
guidance uses the supported `--log` shape.

This roadmap intentionally stops short of a complete migration plan. Exact Rust
type layout, migration order for every call site, and compatibility policy for
legacy serialized actions belong in the Stage 2 draft.

## Special Commands

Infrastructure commands are currently unevenly represented. Some are normal
`CommandSpec` operations; others are entrypoint-level commands that run before
normal project context or registry dispatch.

The command text frontend should not learn semantic facts about special
commands. It may detect `init --help` as help intent, but the actual help
content for `init` must come from either:

- `CommandSpec`, if the command becomes spec-described; or
- a small infrastructure-command help registry owned by the CLI entrypoint.

This distinction matters. If the parser frontend learns "init is a command",
it becomes another command registry. If the help renderer learns how to render
special-entrypoint help after the frontend says "this is help intent", the
syntax boundary stays clean.

Longer term, Exo should consider a unified "infrastructure spec" for commands
that must run before normal context loading. That spec can be projected into
help and tool metadata without making them ordinary project commands.

## Parser Library Evaluation

The parser-library question is worth taking seriously. The right answer may be
"no", but it should be no for architectural reasons, not because Exo has slowly
normalized hand-written parsing.

### Evaluation Criteria

A parser library is a fit only if it can:

- parse Exo's intentionally small CLI-like grammar without importing POSIX
  shell semantics;
- work for both terminal argv input and MCP command-text input;
- preserve Exo placeholder substitution semantics;
- keep help-intent recognition equivalent across CLI and MCP;
- let `--format json` behavior stay aligned with Exo help output;
- preserve or improve Exo diagnostics and steering hints;
- avoid becoming a second source of command schemas or help metadata;
- stay small enough that its behavior can be tested and explained as Exo
  command-language behavior.

### Full CLI Parser Libraries

Examples: `clap`, `bpaf`.

These libraries are strong at defining complete CLI applications. They can own
subcommands, flags, help rendering, validation, and typed command construction.
That is exactly why they are risky here.

`clap` is the best-known option and remains useful in many Rust CLIs, but Exo
has already moved away from Clap-owned dispatch. RFC 00233 records the reason:
Clap-derived parsing, legacy command traits, and `CommandSpec` became competing
sources of truth. Reintroducing Clap as the parser layer would be acceptable
only if Exo generated the Clap shape from `ExoSpec` and treated Clap's parse
result as a mechanical projection. That is a bigger design than this spike.

`bpaf` has a composable parser style that could express pieces of Exo syntax,
but the same warning applies. If Exo hand-writes a second `bpaf` grammar for
the command tree, the project has recreated the dual-source problem in a nicer
syntax.

Conclusion: full CLI parser libraries are not the right first increment. They
are candidates only for a generated projection from `ExoSpec`, not for a
hand-authored replacement parser.

### Lightweight Argv Parsers

Examples: `lexopt`, `pico-args`.

These libraries help scan argv-style flags and values. They are smaller than a
full CLI framework, which makes them attractive at first glance.

The limitation is that Exo's current pain is not just argv scanning. The MCP
path starts with command text, placeholder substitution, quote handling, shell
syntax rejection, and help-intent normalization. A lightweight argv parser
could help after tokenization, but the hard architectural boundary would still
be Exo's own command text frontend and `CommandSpec` compiler.

Conclusion: lightweight argv parsers may be useful for isolated option-scanner
cleanup, but they do not replace the shared frontend and should not become the
command router.

### Text Parser Combinators

Examples: `winnow`, `nom`, `chumsky`.

These libraries are a better conceptual fit if Exo's command language grows
beyond a tiny tokenizer. They can express a domain-specific text grammar
without pretending to be a full CLI framework.

The tradeoff is weight and complexity. The current grammar is small:

- words;
- single and double quoted strings;
- `$N` placeholder substitution;
- a short unsupported-shell-syntax rejection list;
- a small set of help-intent forms.

For that grammar, a custom parser is easy to test and easier for agents and
maintainers to understand. A combinator library becomes more attractive if Exo
adds nested syntax, richer escaping, command macros, or structured inline data.

Conclusion: text parser combinators are the most plausible future library
category, but the current spike does not justify introducing one yet.

### Shell-Style Tokenizers

Examples: `shlex`, `shell-words`.

Shell-style tokenizers are tempting because they solve quote splitting. They
are also easy to misuse. Exo command text is explicitly not shell text. The
grammar should reject shell features rather than accidentally inherit edge
cases, escape rules, or compatibility expectations from a shell lexer.

These crates may still be useful as reference implementations or narrow
tokenizer dependencies if their accepted syntax is audited against Exo's
contract. But Exo should not adopt one merely because the command string "looks
shell-ish".

Conclusion: shell-style tokenizers are acceptable only after an explicit
compatibility audit. They should not define Exo's language by accident.

## Recommendation

Proceed with command surface coherence in two coupled increments:

1. keep the shared custom `command_text` frontend as the syntax boundary for
   terminal argv, MCP command text, and wrapper tools;
2. introduce typed command references for Exo-authored suggestions, recovery
   hints, generated docs, plugin guidance, and cockpit actions.

These increments should land together conceptually even if implemented across
several PRs. The parser frontend prevents CLI/MCP syntax drift. Typed command
references prevent Exo from teaching agents command shapes that the parser and
`CommandSpec` do not accept.

This is a "no for now" on parser-library adoption, not a permanent rejection.
The principled reason is that parser libraries solve only one part of the
problem. Adding a library before the command-surface boundary stabilizes risks
bending Exo's command language toward library semantics, or worse, rebuilding a
second command model beside `ExoSpec`.

The future adoption path should be:

1. keep the frontend API small and heavily tested;
2. make CLI, MCP, and wrapper help behavior depend on the same frontend
   contract;
3. move Exo-authored suggestions onto typed command references;
4. validate rendered command strings against `CommandSpec` in tests;
5. move special-command help behind a spec-like registry or explicit
   infrastructure help table;
6. only then evaluate whether a parser library can replace the frontend
   internals without changing its public behavior.

If a library is adopted later, the preferred candidates are:

- a text parser combinator if Exo's command-text grammar becomes richer;
- a generated full-CLI parser projection if Exo wants library-provided help or
  validation while keeping `ExoSpec` as the source of truth;
- a narrowly audited shell-style tokenizer only if Exo explicitly accepts its
  quote and escape behavior.

## Stage 1 Acceptance Bar

This RFC is ready for Stage 1 when it clearly establishes:

- one shared Exo command language as the proposal direction;
- `command_text` as the shared syntax frontend for CLI-like invocation text;
- `CommandReference` as the typed/spec-validated authoring model for
  Exo-authored command guidance;
- parser libraries as subordinate implementation machinery, not the command
  model;
- a concrete first implementation path through inventory, typed builders, and
  drift tests;
- Stage 2 boundaries for unresolved details such as exact type layout, full
  migration mechanics, and compatibility treatment of legacy serialized
  command strings.

## Relationship To Wrapper Tools

`exo-help`, `exo-read`, and `exo-write` should share this frontend with
`exo-run`.

The tools differ by intent and effect budget, not by command language:

- `exo-help` accepts only help intent and rejects ordinary calls;
- `exo-read` accepts calls only when `CommandSpec` classifies them as pure;
- `exo-write` accepts pure and write calls but rejects exec calls.

This keeps the agent-facing surface CLI-like without multiplying parser
implementations.

The same rule applies to any future cockpit or plugin affordance that presents
an Exo action. A button, menu item, skill example, or recovery card may render
as CLI-like text, but its source should be a typed reference validated against
the same command model used by `exo-run`.

## Implementation Acceptance Criteria

- CLI and MCP help-intent tests cover `task --help`, `task help`, `help task`,
  and `rfc promote --help --format json`.
- MCP command-text tests cover quoted strings, placeholder substitution, and
  unsupported shell syntax rejection.
- Exo-authored `→ Next:` and `→ Try:` suggestions are generated from typed
  command references or pass a `CommandSpec` validation harness.
- Recovery hints, workflow confirmation continuations, dogfood repair actions,
  and status steering cannot name unknown commands, flags, or argument shapes.
- Drift tests cover rendered CLI strings and rendered `exo-run` strings for the
  same command reference.
- A regression fixture covers the `task complete <id> --message ...` class of
  bug by proving Exo-generated suggestions use the supported `--log` shape.
- Special-entrypoint help covers at least `init --help` and
  `daemon ensure --workspace ... --help`.
- Actual command resolution still flows through `CommandSpec`/`ExoSpec`.
- The parser frontend contains no command schema or effect metadata.
- Parser-library adoption remains explicitly deferred unless a follow-up RFC
  shows that the library is a projection or implementation detail, not a new
  authority.

## Stage 2 Design Questions

- What exact Rust structs and traits should implement `CommandReference`,
  rendering, and validation?
- Which existing raw command strings should migrate first after
  `SuggestedAction` and router suggestions?
- How should validation report infrastructure-command exceptions while the
  infrastructure spec/table exists?
- Should namespace-only MCP help remain a semantic fallback, or should the CLI
  expose the same convenience explicitly?
- Should `exo-help` accept command text only, or should it also accept
  structured target tokens for hosts that prefer forms over strings?
- Where should command-text syntax diagnostics attach to steering so agents get
  repair suggestions without needing JSON mode?
- What compatibility policy should apply to any persisted or serialized
  suggested actions that currently contain raw command strings?

## Prior Art And References

- RFC 00233: ExoSpec: Unified Command Definition and the End of Dual-Source
  Drift.
- RFC 10200: CLI-Shaped `exo-run` MCP Transport.
- RFC 10193: Codex Integration and Cockpit Adapter.
- RFC 0200: CLI Argument Consistency.
- Spike note: `docs/research/command-text-parser-frontend-spike.md`.
- Rust parser/library docs reviewed for this RFC:
  - <https://docs.rs/clap/latest/clap/>
  - <https://docs.rs/bpaf/latest/bpaf/>
  - <https://docs.rs/lexopt/latest/lexopt/>
  - <https://docs.rs/pico-args/latest/pico_args/>
  - <https://docs.rs/winnow/latest/winnow/>
  - <https://docs.rs/nom/latest/nom/>
  - <https://docs.rs/chumsky/latest/chumsky/>
  - <https://docs.rs/shell-words/latest/shell_words/>
  - <https://docs.rs/shlex/latest/shlex/>
