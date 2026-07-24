# RFC Reconstruction Final Disposition

RFC corpus reconstruction is complete.

This disposition records what reconstruction established, the evidence that
supports closure, and the boundary between the repaired corpus and future RFC
work. It supersedes the execution queue in the historical
[RFC Reconstruction Plan](./rfc-reconstruction-plan.md) and the pre-audit
checkpoint formerly published as the RFC reconstruction current-state recon.

Verified baseline: clean public `main` at
`74d7c23bee5fad6b6a987aac8c44870b3daf316b`. The RFC corpus last changed in
PR #49, merged as `ca1ee8bfdc54cbe2cd0e83f84ea71670db9b8e40`.

## What Reconstruction Means

Reconstruction makes `docs/rfcs/**` usable as durable project reference without
requiring access to private Git history or a previously seeded sidecar.
Implementation, tests, command behavior, Exo state, durable design notes, and
accepted design packages determine an RFC's role. Checked-in prose remains
evidence, but it is not treated as current law merely because it exists.

The work is complete because the repository now satisfies the reconstruction
charter:

- Stage 3 and Stage 4 contain no known stale statement of implemented law.
- Lifecycle placement, retained stage, and portable metadata agree.
- Every managed RFC has one unambiguous numeric identity.
- Every normalized duplicate-title family has a reviewed, convergent
  disposition.
- Supersession edges resolve to real, semantically appropriate records and are
  reciprocal where the relationship model requires symmetry.
- Canonical and workspace views reconstruct the same corpus across linked
  worktrees.
- A fresh checkout can rebuild the corpus without inheriting hidden canonical
  rows from an older sidecar.

This is a coherence claim, not a claim that every proposal has been implemented
or that the RFC corpus is frozen. Stage 0, Stage 1, and Stage 2 continue to
contain future work at their stated maturity.

## Final Corpus

The public tree contains 338 Markdown files under `docs/rfcs/**`. Two are
support files (`README.md` and `0000-template.md`), leaving 336 managed RFC
records.

### Directory Placement

| Directory | Managed Records |
| --- | ---: |
| `stage-0` | 67 |
| `stage-1` | 68 |
| `stage-2` | 12 |
| `stage-3` | 17 |
| `stage-4` | 25 |
| `archive` | 3 |
| `withdrawn` | 144 |
| **Total** | **336** |

### Effective Lifecycle Status

| Status | Records |
| --- | ---: |
| `active` | 145 |
| `superseded` | 44 |
| `withdrawn` | 144 |
| `archived` | 3 |
| **Total** | **336** |

### Retained Metadata Stage

| Stage | Records |
| ---: | ---: |
| 0 | 154 |
| 1 | 95 |
| 2 | 17 |
| 3 | 37 |
| 4 | 33 |
| **Total** | **336** |

The active current-law surface contains 17 Stage 3 records and 20 Stage 4
records. The remaining Stage 3 and Stage 4 metadata belongs to withdrawn,
archived, or superseded history.

All 336 records are present in both the canonical Git view and the current
workspace view. No effective workspace record differs from canonical, and the
effective read surface reports no metadata diagnostics.

## Landed Reconstruction

### High-Value Current Law

PRs #31 through #33 reconstructed the high-value Stage 3 cluster:

| Scope | Landed Outcome |
| --- | --- |
| RFC 10179 binary re-exec | PR #31, `fd3cb3b3c78f39fc0b13d0ea58798d8dc9e331a8` |
| RFCs 0132/0136 command and tool surfaces | PR #32, `7dde72a6fafb9c98e2f1c8a1353ce9e58ba709cf` |
| RFC 10170 feedback-loop boundaries | PR #33, `64670f2e000e12d5b0a21d9db0c5f16886e47fb3` |

These RFCs remain active Stage 3 Candidates and describe the implemented
boundaries they govern.

### Duplicate-Title Families

PRs #34 through #42 resolved all 96 normalized duplicate-title families,
affecting 193 records:

| Scope | Pull Request | Landed Commit |
| --- | ---: | --- |
| Agent workflow and planning | #34 | `792b8288` |
| UI, workbench, and rich content | #35 | `6f5d8873` |
| RFC, context, and specification authoring | #36 | `8c54f368` |
| Command, tool, and execution architecture | #37 | `a39799a7` |
| Validation, quality, and platform experiments | #38 | `7c6653f5` |
| Higher-stage duplicate history | #39 | `ad117e99` |
| Retired-only history | #40 | `308bf763` |
| Portable successor clearing | #41 | `bd2a539be2f43588818e726facfa75aad23a2824` |
| Semantic Merge Driver linkage | #42 | `5b0e585b567ebecda8b7699abe524dc7c9777475` |

The reviewed dispositions preserve materially distinct authorities, withdraw
only approved historical duplicates, and make each family converge without
numeric ambiguity or internal relationship asymmetry.

### Corpus Audit And Repair

PR #43 published the deterministic pre-repair inventory and isolated the final
clean-bootstrap gaps. PR #49 then repaired those gaps:

- RFC 00178 now has one Stage 1 identity and no metadata conflict.
- Historical RFCs 0048, 00239, and 10071 are restored with portable identity
  and relationship metadata.
- The affected supersession chains now lead through the nearest meaningful
  historical records to their current authorities.
- Stale or phantom relationship endpoints were removed or replaced.
- Older RFC 0111 workspace diagnostics were confirmed as stale observation
  history rather than canonical document debt.

The repair is durable in the Markdown corpus. It does not depend on canonical
rows retained by a previously initialized sidecar.

## Verification

A disposable clean-bootstrap reconstruction produced 336 canonical records and
336 workspace records with:

- zero metadata diagnostics;
- zero duplicate numeric identities;
- zero missing relationship targets;
- zero asymmetric reciprocal edges; and
- zero workspace records differing from canonical.

The final document-link audit scanned all 338 RFC Markdown files without an
error or warning. Focused RFC identity and repair tests, Cargo checks, required
platform checks, and exact-head automated reviews passed for the repair. The
landed PR #49 tree exactly matches its reviewed signed head.

The regenerated [RFC Reconstruction Inventory](./rfc-reconstruction-inventory.md)
is the machine-oriented companion to this disposition.

## Continuing Work

Reconstruction closes the recovery and coherence campaign. It does not decide
later RFC lifecycle changes on their merits.

RFC 10202 is the active Stage 2 contract for the lane-centered workbench
direction. Its implementation remains a separate stream. Future RFC authoring,
promotion, withdrawal, and stabilization continue through the ordinary staged
RFC process, using this coherent corpus as their starting point.
