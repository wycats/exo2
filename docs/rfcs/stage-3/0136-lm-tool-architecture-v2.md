<!-- exo:136 ulid:01kg5kp2hr3b7dneekkkekyvh4 -->

# RFC 0136: LM Tool Architecture v2

- **Supersedes**: RFC 0083

## Summary

Exo exposes a deliberately small language-model tool surface over its complete
command system. `exo-run` is the shared, CLI-shaped project-tool surface backed
by CommandSpec and the machine channel. MCP reaches the complete current command
inventory through that surface. VS Code presents the same tool shape and
execution contract while its extension-local command-text adapter retains known
coverage gaps. A small set of curated extension tools remains separate for
capabilities that benefit from dedicated model visibility.

The VS Code extension contributes exactly five public tools:
`exo-ai-chat-history`, `exo-diagnostics`, `exo-logs`, `exo-ping`, and
`exo-run`. The MCP server contributes `exo-run`. The VS Code manifest
contains zero language-model toolsets. The complete operation inventory remains
discoverable through `exo-run` help and the shared command language defined by
RFC 0132.

This architecture supersedes RFC 0083's collection of orientation,
method-dispatch, and convenience tools. It incorporates the implemented
surface-reduction direction from RFC 10163 and uses the machine-channel
capability model from RFC 0125. RFC 10200 develops the CLI-shaped MCP transport
as a public client contract.

## Motivation

A language model pays for every visible tool schema before choosing a tool.
Publishing the full Exo command inventory as individual tools would make the
model compare a large collection of closely related names and argument
schemas. It would also create a second public command taxonomy alongside the
CLI.

Exo already has a command language with namespaces, help, typed arguments,
effects, diagnostics, confirmation, and recovery. The model-facing architecture
uses that language directly. The MCP project tool reaches the complete operation
set while preserving the same semantics available to CLI users. VS Code uses the
same project-tool surface and machine execution model while its local parser
converges on equivalent command coverage.

Some useful capabilities deserve dedicated extension tools. Diagnostics,
extension logs, and extension identity come directly from the VS Code process.
Chat history is an Exo command, and the extension provides a curated wrapper
that routes to `exo ai chat-history` through the machine channel. Keeping these
capabilities explicit gives models focused schemas for common environmental and
context-recovery work while `exo-run` remains the primary project surface.

## The Public Tool Surface

### Project operations

`exo-run` is the public entry point for Exo project work. It accepts a command
string without the leading `exo` and optional placeholder values. Both
transports accept workflow-completion confirmation data. MCP also accepts the
hidden execution-approval ticket used by confirm-required external operations;
the VS Code tool schema does not currently expose that execution-confirmation
field.

Examples include:

```text
status
task list
task complete build-release --log $1
rfc show 0136
```

The MCP tool tokenizes this text with Exo's Rust command-text compiler, removes
global presentation options before operation validation, rejects shell syntax,
and compiles the result against the current `CommandSpec`. The VS Code tool uses
an extension-local tokenizer and router to construct the structured machine
request. Its request still enters the same typed invocation and execution
machinery, while its parsing errors and treatment of global options currently
differ from MCP.

The adapter difference also affects command coverage. The current VS Code root
command table recognizes `status` and `version`, but omits root `write <path>`;
that command is therefore misrouted as a namespaced operation. This is an
implemented adapter gap within the shared architecture, rather than a separate
command contract.

The two transports therefore share command paths, placeholder substitution,
machine-channel validation, effect classification, and structured responses.
Their registration, process topology, and command-text adapters remain distinct.
Help and diagnostics make the operation inventory discoverable while the public
tool surface stays compact.

### Curated extension tools

The extension publishes four focused tools alongside `exo-run`.

`exo-diagnostics` reads editor diagnostics, and `exo-logs` exposes extension
logs. `exo-ai-chat-history` is a curated wrapper over the machine-channel
operation `exo ai chat-history`; it provides a focused schema and presentation
for recovering conversation context without creating a second source of truth.
`exo-ping` reports the loaded extension's build and process identity and
confirms that its language-model tool registration is functioning. It is
intentionally independent of Exo and the daemon, so a successful ping does not
establish that `exo-run` can reach the project runtime.

These tools are curated capabilities whose authority remains either in the
editor environment or in an existing Exo operation. The boundary keeps
additional extension features subject to deliberate product evaluation. A new
curated tool needs a clear source contract and a capability that benefits from
direct model visibility.

## Command Discovery

The model discovers project operations through the same hierarchy as a CLI
user. General help describes root operations and namespaces. Namespace and
operation help reveal accepted arguments and effects. `CommandSpec` compilation
can return structured diagnostics and suggestions; tokenizer-level failures keep
the transport-specific shape described by RFC 0132.

This help ladder provides progressive disclosure without introducing a second
grouping system. Names such as `task`, `goal`, `phase`, and `rfc` remain
the public vocabulary across human and model interaction.

CommandSpec-generated tool metadata is still useful. It supports schema
inspection, artifact checks, and experiments with alternate projections. It is
informational unless a tool is explicitly selected for the curated manifest.
Publication as a VS Code tool requires explicit selection for the curated
manifest.

## Execution and Safety

MCP compiles a requested command before dispatch. VS Code constructs a
structured call with its local adapter, after which the machine channel resolves
the operation and validates its arguments. In both paths, Exo derives effect and
recovery class from the command specification and built command rather than from
client-supplied recovery metadata.

Pure operations can be repeated. Project-state mutations use the daemon's
writer lane and durable outcome contract. External operations retain
at-most-once behavior unless Exo has durable completion proof. Workflow
completion commands can return confirmation data that both transports resubmit.
Confirm-required external operations return an execution ticket that MCP can
resubmit through its `auth` input; the current VS Code schema stops at the
confirmation response.

A stable request identity follows a call through proxying and reconnects so
completed outcomes can be replayed without repeating the mutation. Workspace
identity is also request-scoped, allowing one daemon to validate and serve
commands from linked worktrees without treating its startup checkout as the
caller's workspace.

The model-facing layer preserves the classification and response supplied by
the command and daemon layers.

## VS Code Registration

The extension manifest is the declarative public inventory, and runtime
activation registers implementations only for tools present there.

The repository's synchronization check enforces the curated five-tool list and
removes `languageModelToolSets` declarations. Generated package-tool output
remains available for auditing, separate from the public manifest.

This distinction resolves an older source of drift. Declarative metadata and
runtime registration still have different jobs, but they agree on which tools
exist. Shared factories may remain available as implementation infrastructure
without defining the active product surface.

## MCP Registration

The Exo MCP server advertises one tool named `exo-run`. Its instructions state
that calls use Exo CLI syntax without the leading executable name and that the
tool is not a shell runner.

MCP tool calls compile into the same machine request model used by other
clients. Worker classification determines effects and confirmation needs before
execution, and the response preserves Exo's structured result, steering,
command-compilation diagnostics when present, and recovery identity.

An MCP host therefore learns one transport schema while retaining access to the
full current CommandSpec. Adding an Exo operation makes it available through
`exo-run` without a new MCP tool declaration.

## Relationship to the Command Architecture

RFC 0132 owns the shared command grammar, typed invocation, and diagnostic
model. This RFC owns the decision about which of those capabilities appear as
language-model tools.

RFC 0085 owns executable Command implementations. RFC 0125 owns capability and
machine-channel structure. RFC 00233 owns the inline ExoSpec authoring model.
RFC 10163 proposed reducing the model-facing surface to CLI delegation; the
architecture here records the implemented result.

These boundaries allow the command inventory to grow without expanding the
tool picker. They also allow curated wrappers and editor-sourced tools to evolve
without changing the primary command surface.

## Drawbacks

A single project tool gives the model less schema-level guidance for an
individual operation than a dedicated tool would. Exo compensates with a
familiar command hierarchy, help, examples, and structured diagnostics. This is
a deliberate exchange: command discovery happens when needed instead of
charging every request for the full inventory.

The VS Code inventory extends MCP's single project tool with focused schemas for
editor-sourced capabilities and selected Exo operations. Documentation
distinguishes the shared project transport from this curated extension layer
rather than referring to a single undifferentiated tool count.

Curation also requires judgment. Command metadata can be generated
mechanically, while deciding that a capability deserves permanent visibility
to models remains a product decision.

## Alternatives

Declaring one tool per Exo operation would maximize schema-specific
discoverability. It would also duplicate the CLI taxonomy, increase prompt
cost, and make every command addition a manifest change.

Language-model toolsets could group a large generated inventory, but grouping
would preserve the inventory and its schemas. It also makes the public
architecture depend on a second hierarchy and on host support for toolsets.

A single complex tool with a large tagged-union schema would keep the tool count
small while rebuilding the command tree inside one JSON schema. `exo-run`
instead reuses the existing command language and its help system.

Shell execution would make arbitrary composition available, but it would bypass
CommandSpec validation, effect classification, confirmation, and portable
parsing. Exo keeps execution inside the machine channel.

## Current Status

The curated architecture is implemented. The VS Code manifest contains the five
named tools and no language-model toolsets. Extension activation registers that
same declared set. The Exo MCP server advertises only `exo-run`.
CommandSpec-generated tool metadata remains an informational projection, and
tests enforce the curated manifest and MCP transport behavior.

Stage 3 reflects the deployed contract while leaving room to converge the MCP
and VS Code command-text adapters and improve help, descriptions, and client
presentation. Additional public tools or toolsets would require new evidence
that their persistent visibility improves model behavior enough to justify
expanding the surface.
