<!-- exo:132 ulid:01kmzxbcxn3jzfdsy1bf14z4dm -->

# RFC 0132: CLI Patterns: Command Spec, Router, and Tool-Safe DSL

## Summary

Exo presents one command language across its CLI, machine channel, MCP server,
and VS Code extension. Commands are described by a machine-readable
`CommandSpec`, compiled into typed `Invocation` values, and dispatched through
the same command implementations regardless of the frontend that supplied the
input.

The command language is CLI-shaped because verb-first paths, options, and
positionals provide a familiar interface for people and language models. The
language evaluates one Exo invocation as data. Its grammar covers Exo command
structure while leaving shell operators, expansion, pipelines, and ambient
process behavior to dedicated process tools. Structured
frontends can provide argument values directly and reach the same invocation
model without reconstructing command text.

This RFC records the implemented command-surface contract. RFC 0085 defines the
runtime Command trait architecture. RFC 0125 defines the capability and machine
channel beneath external clients. RFC 0136 defines how the command surface is
presented to language models. RFC 00233 develops the inline ExoSpec authoring
model that supplies command metadata.

## Motivation

A project command is more than a function call. It has a public name, an
argument grammar, an effect, a recovery policy, help text, diagnostics, and a
structured result. When each frontend defines those properties independently,
the CLI, editor, and agent surfaces become different products with similar
spelling.

Exo instead treats command semantics as shared project infrastructure. A command
added to the Rust implementation enters a common inventory. The CLI can compile
argv against that inventory, machine clients can submit structured values, help
can describe the accepted shape, and model-facing transports can classify the
operation before execution. The result is one behavioral contract with several
presentations.

The same structure also provides a safety boundary. Agents benefit from concise
CLI-shaped commands, and Exo provides them through tokenization and typed
compilation. Keeping command text inside a small grammar makes effects visible,
errors local, and retries governable.

## The Command Model

### Command identity

An operation has a stable address consisting of an optional namespace and an
operation name. Namespaced operations use paths such as `task complete` or
`rfc show`; root operations use a single name such as `status`.

The address identifies both executable behavior and its specification. The
specification records the operation's description, arguments, effect,
recovery class, upgrade requirements, and language-model guidance. Every frontend reads those properties from the shared specification and uses
the same operation catalog.

### Arguments

The command specification distinguishes flags, options, and positional
arguments. Each argument has a stable name, accepted value type, optional short
name, required or optional status, repeatability, and any default or enumerated
values.

Compilation produces typed values rather than leaving every input as an
uninterpreted string. The common value model includes booleans, integers,
floats, strings, paths, JSON values, enums, and arrays. Type errors therefore
belong to command compilation, where Exo can report the offending argument and
the expected form.

### Effects and recovery

Every operation declares whether it is pure, writes project state, or performs
an external effect. Recovery metadata further distinguishes replayable reads,
atomic project-state mutations, and external at-most-once work.

These declarations are part of the command contract. Daemon routing,
confirmation, outcome recovery, post-write persistence, and client guidance
all consume them. A frontend may change how an operation is presented while preserving the
operation's declared effect.

## From Specification to Invocation

The implemented pipeline has three semantic steps:

```text
command definition -> CommandSpec -> Invocation -> Command
```

Inline `ExoSpec` definitions and the command registry produce the
`CommandSpec`. The specification is an ordered, serializable description of
root operations, namespaces, operations, arguments, and metadata.

A frontend compiles its input against that specification. Successful
compilation produces an `Invocation` containing the resolved command address,
typed arguments, occurrence counts, and source information. Dispatch then
constructs the corresponding command and runs it through the shared execution
and response machinery.

This separation matters. Parsing answers what the input means. Command
execution applies that meaning to a project. Help, schema generation, and
classification can inspect the first two layers without executing the third.

## Frontends

### CLI argv

The CLI compiler accepts ordinary argv tokens. It resolves the command address,
parses long and short options, assigns positional values, applies defaults, and
validates required arguments and value types.

Global presentation options such as `--format` are handled without becoming
operation arguments. The compiled invocation then enters the same dispatcher
used by structured clients.

### Structured machine requests

Machine-channel clients identify an operation and provide structured arguments.
Those arguments are validated against the same `CommandSpec` and converted
into the same typed invocation model.

Structured requests carry protocol identity, request identity, workspace
context, confirmation data, and recovery metadata outside the command
arguments. This keeps transport and lifecycle concerns explicit while
preserving the command's public argument contract.

### Tool-safe command text

`exo-run` accepts a compact command string because CLI-shaped text is an
effective interface for language models. Its tokenizer supports command words,
options, positionals, quoting, escaping, and explicit placeholder values for
content supplied independently from command text.

The tokenizer treats all input as data. Environment assignments, command
substitution, pipelines, redirects, and control operators produce explicit
unsupported-input diagnostics. Placeholder values are inserted as argument
tokens rather than evaluated as program text.

The command-text frontend is deliberately smaller than a shell language. Its
scope is one safe Exo invocation; project workflows and process composition use
their dedicated Exo surfaces.

## Diagnostics

Compilation failures produce structured diagnostics. A diagnostic has a stable
code, a message, source location when available, contextual values, and
concrete suggestions.

Diagnostics answer the likely mistake. Unknown namespaces and operations can
offer nearby names. Unknown flags can show the accepted flags for the resolved
operation. Missing or invalid values can identify the argument and expected
type. Shell operators can explain that `exo-run` is a command transport rather
than a shell.

Human CLI output and machine responses may render these diagnostics
differently, but both preserve the same underlying failure and repair
information.

## Projections

The command specification supports several derived views: help and command
references, JSON artifacts for editor clients, machine-channel classification,
and language-model metadata. These are projections of the command language and inherit its authority.

A projection may intentionally expose only part of the inventory. In
particular, RFC 0136 keeps the public language-model tool list small even though
the generated command artifact describes the full Exo operation set. `exo-run` provides complete command coverage while the public tool manifest
stays deliberately curated.

Generated artifacts are checked against the Rust command inventory so that
clients can detect drift. Command authority and effect classification remain
with the Rust command definitions.

## Compatibility and Evolution

Command paths and argument names are user-facing compatibility surfaces.
Renaming or removing them affects CLI users, model prompts, recorded command
text, and machine clients. Additive metadata and new projections can evolve
without changing command behavior, while semantic changes require the command
implementation and its specification to move together.

The inline ExoSpec model continues to reduce authoring duplication. That
migration preserves the architectural boundary in this RFC: however a command
is authored, the effective `CommandSpec` remains the contract consumed by
compilation and projection.

## Drawbacks

A shared specification concentrates correctness requirements. Incorrect
argument or effect metadata can influence every frontend at once, so parity and
artifact checks are part of the implementation rather than optional tooling.

The tool-safe grammar expresses one Exo operation at a time. Process
composition uses dedicated execution surfaces, while project workflow lives in
Exo's commands and state model.

Generated inventory and interface curation solve different problems. Public
surfaces still need deliberate product judgment, which is why RFC 0136 owns
language-model presentation separately.

## Alternatives

Keeping independent CLI, editor, and agent command definitions would let each
surface optimize locally, but it would preserve the drift this architecture is
designed to remove.

Accepting shell command strings would provide familiar composition at the cost
of hidden effects, platform-dependent parsing, quoting hazards, and a much
larger security boundary.

Exposing only structured JSON would be safe but would discard the concise,
discoverable CLI language that people and models already understand. The
current design keeps structured execution while allowing CLI-shaped input.

## Current Status

The shared command model is implemented. Exo generates a command specification
from its registered command definitions, compiles argv and structured inputs
into typed invocations, rejects shell operators, emits structured diagnostics,
and projects the inventory into help, machine-channel, and editor artifacts.

Stage 3 reflects that implemented contract. Further work may improve individual
diagnostics, artifact ergonomics, or command authoring, and those refinements preserve the
command-language boundary recorded here.
