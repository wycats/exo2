# Exo Codex Plugin

This plugin registers Exo's MCP stdio server for Codex:

```json
{
  "mcpServers": {
    "exo": {
      "command": "exo-mcp",
      "args": []
    }
  }
}
```

The server exposes a single `exo-run` tool. Commands use Exo CLI syntax without
the leading `exo`, for example `status`, `task list`, or
`task complete <id> --log $1`.

## Local Setup

From the repository root, install or update the Exo binaries before loading the
plugin:

```sh
cargo install-exo
```

The `cargo install-exo` alias is defined by this repository's Cargo
configuration. To avoid the alias, run the equivalent command from the
repository root:

```sh
cargo install --path tools/exo --locked
```

Both commands install the `exo` CLI and the `exo-mcp` durable MCP proxy. The
plugin expects `exo-mcp` and `exo` to be on `PATH` for the Codex process. The
MCP proxy resolves the active workspace through Exo's normal project and
sidecar policy.

When running the full local dogfood lifecycle, `cargo dogfood-exo` installs the
workspace binaries and updates any installed local Codex Exo plugin cache to
launch the installed `exo-mcp` by absolute path. That pinned launch also records
the workspace build it supervises, so the durable proxy routes tool calls through
the current `target/debug/exo` worker and replaces that worker after a rebuild.
The source plugin package keeps the portable `exo-mcp` command so published
installs can continue to use PATH.

After a merge to `main`, GitHub Actions also builds downloadable `exo` and
`exo-mcp` artifacts for currently supported runner platforms. If you have GitHub
CLI access to this repository, you can install the latest successful `main`
artifact for your host with:

```sh
scripts/dev/install-latest-exo-binaries.sh
```

The source install remains the canonical fallback when no matching artifact is
available for your platform.

After changing the plugin package or installed binaries, start a fresh Codex
thread so the host reloads the plugin and MCP server definition.
