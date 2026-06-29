<!-- exo:10086 ulid:01kmzxeffn71814xa71a5zb3nx -->


# RFC 10086: Editing Tools Improvements

## Summary

Investigate improvements to the file editing tools (`replace_string_in_file`, etc.) to reduce "mashing" (repeated failed attempts to edit files).

## Motivation

The current editing tools are brittle. Agents often fail to match the `oldString` exactly due to whitespace or context mismatches, leading to loops of "Read -> Try Edit -> Fail -> Read -> Try Edit". This wastes tokens and time.

## Detailed Design

_Ideas:_

- **Fuzzy Matching**: Allow slight whitespace variations in `oldString`.
- **Unified Diff Tool**: Instead of `replace_string`, allow the agent to provide a unified diff or a search/replace block (like `sed`).
- **Line-Based Editing**: `insert_after_line`, `delete_lines`.
- **AST-Based Editing**: For structured languages (JSON, TOML, TS), use AST transformations?

## Unresolved Questions

- What is the balance between precision and ease of use?
- How to prevent accidental wrong replacements with fuzzy matching?
