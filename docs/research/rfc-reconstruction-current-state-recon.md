# RFC Reconstruction Current-State Recon

This checkpoint records the state of RFC reconstruction after the high-value
Stage 3 pass and the seven reviewed duplicate-title batches. It is the current
status source for the final corpus audit.

Scanned baseline: clean public `main` at
`5b0e585b567ebecda8b7699abe524dc7c9777475`.

## Reconstruction Charter

RFC reconstruction makes `docs/rfcs/**` usable as durable project reference.
Stage 3 and Stage 4 RFCs should describe implemented behavior or explicitly
record implemented history. Earlier-stage RFCs should preserve future plans
only when they still map to current architecture and a plausible implementation
path.

The evidence-driven method remains unchanged: implementation, tests, command
behavior, Exo state, durable design notes, and accepted design packages
determine whether an RFC is current, historical, superseded, archived,
withdrawn, or future work. Existing prose is evidence, not authority merely
because it is checked in.

Reconstruction is complete when:

- Stage 3 and Stage 4 contain no known stale law;
- lifecycle placement and portable metadata agree;
- numeric identity is unambiguous;
- duplicate-title families have reviewed dispositions;
- supersession and related-RFC links lead to real, semantically appropriate
  records;
- canonical and workspace views remain stable across linked worktrees; and
- the remaining future-work corpus can be read without reconstructing private
  history first.

## Live Corpus Inventory

The public tree contains 335 Markdown files under `docs/rfcs/**`. Two are
support files (`README.md` and `0000-template.md`), leaving 333 managed RFC
records.

### Directory Placement

| Directory | Managed Records |
| --- | ---: |
| `stage-0` | 65 |
| `stage-1` | 67 |
| `stage-2` | 12 |
| `stage-3` | 17 |
| `stage-4` | 25 |
| `archive` | 3 |
| `withdrawn` | 144 |
| **Total** | **333** |

Directory placement is not the same as retained design maturity. Withdrawn
and archived documents keep the stage they reached so historical readers can
distinguish an abandoned proposal from implemented history.

### Effective Lifecycle Status

| Status | Records |
| --- | ---: |
| `active` | 145 |
| `superseded` | 41 |
| `withdrawn` | 144 |
| `archived` | 3 |
| **Total** | **333** |

### Retained Metadata Stage

| Stage | Records |
| ---: | ---: |
| 0 | 152 |
| 1 | 94 |
| 2 | 17 |
| 3 | 37 |
| 4 | 33 |
| **Total** | **333** |

The effective current-law surface contains 17 active Stage 3 records and 20
active Stage 4 records. The remaining Stage 3/4 metadata belongs to withdrawn,
archived, or superseded history.

## Identity, Family, And Overlay State

The identity and duplicate-family work is complete at the corpus level:

- all 333 managed documents have unique numeric `exo:` anchors;
- all 96 normalized duplicate-title families, affecting 193 records, have a
  reviewed survivor or explicit historical relation;
- no reviewed family is nonconvergent or internally asymmetric;
- every record is present in the canonical Git tree;
- canonical quarantine is empty;
- no effective workspace record differs from canonical; and
- the sidecar repository is clean and synchronized.

The current workspace observes 332 of 333 records. RFC 00178 is absent from the
workspace observation set because its Markdown declares two RFC headings and
Stage 0 body metadata while living in the Stage 1 directory. Exo records that
as a `metadata_conflict`, then retains the canonical record in the effective
view with `workspace_presence: absent`. This is explicit final-audit debt, not
canonical data loss or an effective-read failure.

Older linked-worktree observations also retain an RFC 0111 metadata conflict.
The cross-worktree audit must distinguish stale observations from a defect in
the canonical document before changing either record.

## Completed Execution Ledger

### High-Value Stage 3 Reconstruction

| Scope | Landed Outcome |
| --- | --- |
| RFC 10179 binary re-exec | PR #31, `fd3cb3b3c78f39fc0b13d0ea58798d8dc9e331a8` |
| RFCs 0132/0136 command and tool surfaces | PR #32, `7dde72a6fafb9c98e2f1c8a1353ce9e58ba709cf` |
| RFC 10170 feedback-loop boundaries | PR #33, `64670f2e000e12d5b0a21d9db0c5f16886e47fb3` |

These RFCs remain active Stage 3 Candidates and now describe their implemented
boundaries. Their supporting code and guidance corrections landed with clean
exact-head review and validation.

### Duplicate-Title Dispositions

| Batch | Pull Request | Landed Commit |
| --- | ---: | --- |
| Agent workflow and planning | #34 | `792b8288` |
| UI, workbench, and rich content | #35 | `6f5d8873` |
| RFC, context, and specification authoring | #36 | `8c54f368` |
| Command, tool, and execution architecture | #37 | `a39799a7` |
| Validation, quality, and platform experiments | #38 | `7c6653f5` |
| Higher-stage duplicate history | #39 | `ad117e99` |
| Retired-only history | #40 | `308bf763` |
| Portable successor clearing | #41 | `bd2a539be2f43588818e726facfa75aad23a2824` |
| Final Semantic Merge Driver linkage | #42 | `5b0e585b567ebecda8b7699abe524dc7c9777475` |

The batches established reviewed symmetric family chains, withdrew only
approved historical duplicates, retained materially distinct authorities, and
removed stale or unrelated family links. PR #42 closed the sole gap found by
the post-batch family validator.

### Foundation Already In Place

The preceding stabilization tranche also delivered durable RFC identity and
lifecycle reads, portable lifecycle metadata, worktree-aware RFC overlays,
request-scoped daemon workspace identity, and RFC 10196 as a Stage 3 Candidate.
RFC 10202 is the active Stage 1 adoption record for the lane-centered workbench
direction. Its Stage 2 development remains a separate implementation stream.

## Final Audit Candidates

The inventory exposes a small set of candidates for the coherence audit. This
checkpoint records them without deciding their disposition:

| Candidate | Observed State | Audit Question |
| --- | --- | --- |
| RFC 00178 | Current workspace `metadata_conflict`; canonical file has duplicate headings and Stage 0 body metadata in a Stage 1 path. | What single Stage 1 document shape preserves its proposal while restoring a valid workspace observation? |
| RFCs 0030 and 0040 | Both point to RFC 0080, whose reciprocal `Supersedes` set names RFCs 0019, 0055, and 10072. | Are 0030/0040 true predecessors of RFC 0080, or stale links that should be removed? |
| RFC 0082 | Points to RFC 0122, while RFC 0122 names only RFC 0141. | Does streaming progress replace the signed-capability proposal, or is the successor semantically wrong? |
| RFC 0103 | Points to RFC 00225 without reciprocal target metadata. | Is this a legitimate replacement chain requiring symmetry? |
| RFC 10116 | Points to missing RFC 10014. | Should the declared successor be cleared, relinked, or represented as unavailable history? |
| RFC 0111 linked-worktree diagnostics | Present in older workspace observations, absent from the current 155b workspace diagnostic set. | Is this stale observation state or canonical metadata debt? |

The audit must also re-read every active Stage 3/4 record against implementation
evidence, check all lifecycle paths and portable markers, verify relationship
endpoints and semantic scope, and exercise canonical/workspace stability across
linked worktrees.

## Remaining Execution Queue

The old reconstruction queue is complete. The remaining phase work is exactly:

1. Land this refreshed ledger and deterministic corpus inventory.
2. Audit corpus coherence and cross-worktree integrity, resolving only reviewed
   findings through focused RFC or product changes.
3. Publish the final reconstruction disposition and phase outcome.

RFC 10202 Stage 2 refinement and lane implementation remain outside this phase.

## Relationship To Historical Research

The [RFC Reconstruction Plan](./rfc-reconstruction-plan.md) and
[initial Stage 3/4 classification](./rfc-stage-3-4-current-state.md) preserve
the evidence and priorities that began this work. Their old counts and next-step
lists are historical. This checkpoint and the regenerated
[RFC Reconstruction Inventory](./rfc-reconstruction-inventory.md) are the
current execution sources for the final audit.
