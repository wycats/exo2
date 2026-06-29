<!-- exo:6 ulid:01kg5kp2b59jb3j2de7ev4875f -->

# RFC 6: Workspace Cache


# RFC 0006: Workspace Cache

## Summary

Implement a live, in-memory cache of all files and directories in the workspace to support synchronous O(1) lookups for the "Smart Kernel".

## Motivation

The "Smart Kernel" needs to linkify file paths in the chat stream in real-time. Querying the file system for every token is too slow.

## Detailed Design

### Data Structure

- `files`: `Set<string>` of all file paths.
- `directories`: `Set<string>` of all directory paths.

### Lifecycle

1.  **Initialization**: Asynchronous `findFiles` on startup.
2.  **Maintenance**: `FileSystemWatcher` updates the Sets on create/delete.

### API

- `hasFile(path)`: Exact match.
- `hasDirectory(path)`: Directory existence check.

## Performance

- Memory: ~5-10MB for 50k files.
- Latency: Sub-microsecond lookups.


