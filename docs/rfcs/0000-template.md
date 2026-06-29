# Do Not Create RFC Files Manually

This directory is managed by the `exo` CLI. **Do not use `create_file` or manual editing.**

## To Create an RFC

```bash
exo rfc create --title "Your RFC Title" --feature "feature-name"
```

This will:

1. Assign the next available RFC number
2. Create the file with correct frontmatter
3. Populate the template structure

## To View/Edit RFCs

```bash
exo rfc list              # List all RFCs by stage
exo rfc show <id>         # Show RFC details
exo rfc promote <id>      # Promote to next stage
```

## Why?

RFCs have sequential IDs managed by the system. Manual file creation causes:

- ID collisions
- Missing metadata
- Verification failures that auto-repair (renumbering your RFC)

**Treat `docs/rfcs/` as a database, not a file tree.**

## Drawbacks

Why should we _not_ do this?

## Alternatives

What other designs were considered?

## Unresolved Questions

What parts of the design do you expect to resolve through the RFC process before this gets merged?

## Future Possibilities

Think about what the natural extension and evolution of this proposal would be.
