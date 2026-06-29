---
applyTo: "**/docs/rfcs/**/*.md"
---

# RFCs (Request for Comments)

RFCs are the governance mechanism for decisions—capturing rationale, alternatives considered, and outcomes.

**Content:** Edit RFC prose directly. RFCs are living documents meant to be written and refined.

**The "Write It Today" Test:** An RFC should read as if written from scratch with full current knowledge. Remove journey residue (superseded notes, "we discovered X" language, recon scaffolding). The test: _if I were writing this today, knowing what I know now, how would I write it?_ This may require significant revision or restructuring — that's expected. RFCs are authoritative specifications, not historical transcripts.

**Lifecycle:** Manage stage transitions with `exo-run("rfc list")`, `exo-run("rfc create --title '...'")`, and `exo-run("rfc promote <id>")`.

**Stage gates:**

- Stage 0→1: User approval required (idea → proposal)
- Stage 1→2: User approval required (proposal → draft spec)
- Stage 2→3: Implementation complete + Manual updated (draft → candidate)
- Stage 3→4: Shipped (candidate → stable)

**Superseded/Duplicate RFCs:**

- **Superseded**: If RFC A is superseded by RFC B, delete RFC A. First check if any content from A should be incorporated into B (avoiding historical transcripts — RFCs are living documents, not journals).
- **Duplicates**: Literal duplicates should be deleted. Keep the higher-stage version.
- **Withdrawal**: Use `exo-run("rfc withdraw <id>")` only when implementation started but the approach was abandoned. Withdrawn RFCs are archived for historical context but not listed in active RFCs.

Use `docs/rfcs/0000-template.md` as a starting point for new RFCs.
