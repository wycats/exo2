<!-- exo:72 ulid:01kg5kp2ehv6nbm7wmccwncd1k -->

# RFC 72: Semantic Merge Driver for Structured Context Files



# RFC 0072: Semantic Merge Driver for Structured Context Files

## Summary

Introduce a Git merge driver for Exosuit “structured context” files (initially TOML) that performs conservative semantic 3-way merges and, when conflicts remain, records them in a *parseable* conflict format (option 2) suitable for tooling.

This RFC also proposes a Studio conflict-resolution view that can present these structured conflicts and help the user resolve them inside the existing Studio editors.

## Motivation

We frequently hit merge conflicts in “structured” workspace files that are:

- mechanically edited by `exo` (or agents),
- often updated in parallel across branches,
- semantically mergeable (e.g. union-by-`id`),
- but *textually* conflict-prone.

Examples:

- `docs/agent-context/ideas.toml`: independent additions should merge cleanly.
- `docs/agent-context/plan.toml`: reordering and parallel edits should not produce noisy conflicts.
- `docs/agent-context/current/*`: these are projections/snapshots; merging them is usually counterproductive.

## Goals

- Provide semantic 3-way merges for a narrow set of Exosuit TOML files.
- Avoid noisy conflicts for “additions-only” changes (especially arrays-of-tables keyed by `id`).
- When conflicts remain, produce a *machine-readable* conflict representation while still returning a conflict to Git.
- Enable a Studio UX for resolving these conflicts using the existing structured editors.

## Non-Goals

- A universal TOML merge for arbitrary schemas.
- Solving all Git conflicts (only the high-value structured files).
- Silently “picking a side” when both branches change the same scalar value.

## Detailed Design

### Terminology

- **Semantic merge driver**: a Git merge driver that interprets file structure (TOML AST) rather than doing purely line-based merging.
- **Parseable conflict**: the driver exits with conflict (`1`) but writes a syntactically valid TOML file containing structured conflict payloads (instead of `<<<<<<<` markers).
- **Studio**: the Exosuit “structured file” UI surface (the existing Studio views/editors) where users view and edit context files.

### 1) A Git merge driver backed by `exo`

Add a CLI entrypoint suitable for use as a Git merge driver:

```bash
exo merge-driver toml <base> <current> <other> [--path <path>]
```

Where:

- `<current>` is the file that Git expects to be updated in-place.
- Exit code `0` means “merged cleanly”.
- Exit code `1` means “merge contains conflicts”.

### 2) Merge semantics (TOML)

We define a conservative 3-way merge on TOML values:

- If `current == other`: take it.
- If `current == base`: take `other`.
- If `other == base`: take `current`.
- Tables: recursively merge keys.
- Arrays-of-tables with `id`: merge by `id`.
    - For matching `id`, recursively merge table fields.
    - Pure additions of distinct `id`s should not conflict.
- Anything else: treat as a conflict.

### 3) Conflict representation (option 2: parseable TOML)

When conflicts remain, the driver SHOULD:

- exit with code `1` (so Git correctly marks the path as conflicted), and
- write a syntactically valid TOML document that includes a conflict payload.

A concrete shape (subject to iteration) is:

```toml
[exo.merge]
conflict = true
format = 1

[[exo.merge.conflicts]]
path = ["tasks", "id=10036", "title"]
base = "... TOML snippet ..."
current = "... TOML snippet ..."
other = "... TOML snippet ..."

# (repeat for each conflict)
```

Notes:

- The conflict payload is deliberately “out of band” under an `exo.*` namespace so schema-specific parsers can either ignore it or explicitly error with a clear diagnostic.
- The file remaining *parseable* is the key property; tools can detect `exo.merge.conflict = true` and present a dedicated resolution flow.
- We should strongly consider adding guard rails (e.g. `exo verify` / Studio editors refusing to treat the file as “valid context”) while `exo.merge.conflict=true` is present.

### 4) Special-case: `docs/agent-context/current/*`

For snapshot/projection files under `docs/agent-context/current/`, default behavior should be “ours wins” (keep `<current>` as-is) and exit `0`.

Rationale:

- These files are derived artifacts.
- They can be regenerated and should not block merges.

### 5) Studio conflict-resolution view (custom diff/conflict UX)

Studio should provide a structured conflict-resolution UI for files that:

- are supported by Studio (e.g. `plan.toml`, `ideas.toml`), and
- contain `exo.merge.conflict=true`.

Proposed UX (minimal, consistent with existing Studio views):

- When a supported file contains structured conflicts, Studio opens the normal editor/view, but shows a “Resolve Conflicts” state.
- Conflicted entities/fields are highlighted in the structured view (e.g. a task row, a field within a task, an idea entry).
- Selecting a conflict shows the three candidates (**base**, **ours/current**, **theirs/other**) and allows the user to pick one (or edit the final value).
- Applying a resolution writes the chosen value into the canonical location and removes the corresponding `[[exo.merge.conflicts]]` entry.
- When all conflicts are resolved, Studio removes `[exo.merge]` (or sets `conflict=false`) and the file becomes a normal, schema-valid document again.

This UI intentionally does not need Git strategy sensitivity; it is driven entirely by the conflict payload produced by the merge driver.

## Repository Integration

### `.gitattributes`

Mark a narrow initial file set to use the driver:

- `docs/agent-context/ideas.toml`
- `docs/agent-context/plan.toml`
- `docs/agent-context/current/*.toml`

### Installation

Provide a helper script (repo-local) that installs the driver mapping:

```bash
git config merge.exo-toml.name "Exosuit TOML semantic merge"
git config merge.exo-toml.driver "exo merge-driver toml %O %A %B %P"
```

## Drawbacks

- Adding structured conflict payloads requires consumers to either ignore `exo.*` keys or to fail loudly with a clear diagnostic.
- A parseable conflict file can be “misleadingly usable” unless we add explicit guard rails (verification/UI refusal).

## Alternatives

- Plain textual conflict markers (`<<<<<<<`): works everywhere, but cannot be used to build a structured conflict-resolution UX.
- Sidecar conflict files: keeps the primary schema clean, but complicates workflows and requires additional plumbing.
- Rely entirely on external merge tools (`git mergetool`): less integrated and harder to make consistent for Exosuit-specific schemas.

## Unresolved Questions

- What path notation should we standardize for `[[exo.merge.conflicts]]` (string paths, arrays of segments, JSON Pointer-like)?
- Should `exo` treat `exo.merge.conflict=true` as a hard error for reads (plan/ideas), or only for writes/verification?
- For which file classes should we prefer “ours wins” vs “conflict payload required”?

## Future Possibilities

- Add a semantic diff view for these files (not only conflict resolution).
- Support additional structured formats (JSON, YAML) using the same “parseable conflict payload” contract.
- Provide multiple merge drivers (conservative vs union vs ours-wins) selected by path via `.gitattributes`.

