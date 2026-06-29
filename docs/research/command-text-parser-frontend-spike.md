# Command Text Parser Frontend Spike

## Prepare Hypothesis

Exo already has the right semantic authority in `CommandSpec`/`ExoSpec`, but
syntax is split across entrypoints:

- terminal `exo` argv handling strips globals and detects help in `main.rs`;
- MCP `exo-run` tokenizes command text, substitutes placeholders, detects help,
  and strips global format flags in `mcp.rs`;
- machine-channel calls already converge on `CommandSpec` addresses;
- special entrypoints (`init`, `mcp`, `daemon`, `json server`, `merge-driver`,
  `validate`) bypass normal command dispatch because they run before or outside
  an ordinary project context.

Prediction: a small shared `command_text` frontend can reduce duplicate syntax
handling without becoming a competing command model.

## Execute Summary

The spike added a `command_text` frontend that owns syntax only:

- command-text tokenization for MCP input;
- `$1` placeholder substitution;
- explicit help intent detection for `help task`, `task help`, and
  `task --help`;
- global `--format` stripping for help target detection;
- JSON-output detection shared with MCP response profiling.

CLI help routing and MCP `exo-run` request construction now use the shared
frontend. Command resolution still flows through `CommandSpec`/`ExoSpec`.

Special command help remains outside the parser frontend. The frontend can
identify `daemon ensure --workspace PATH --help` as help for `daemon ensure`,
but rendering that help is a CLI help-layer concern until infrastructure
commands become spec-described.

## Review

The main prediction held: syntax duplication dropped without moving command
semantics out of `CommandSpec`.

The main friction point also matched the hypothesis: "namespace-only means
help" is semantic, not syntactic. MCP keeps that behavior as a small
CommandSpec-aware fallback rather than teaching the frontend about namespaces.

No parser library was introduced. This spike shows the first step is an
internal frontend boundary; a library can still be evaluated behind that
boundary later.

Recommendation: proceed with the shared custom parser module for the next
increment, then evaluate a parser-library-backed implementation only after the
frontend contract stabilizes.
