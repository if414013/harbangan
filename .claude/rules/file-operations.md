# File Operations Rules

Applies to all file read/write operations across the codebase.

## Writing Files

- Use Edit for modifying existing files. It sends only the diff, saving tokens and reducing risk.
- Only use Write for creating new files or complete rewrites of small files (<50KB / ~200 lines).
- A PreToolUse hook enforces this: Write calls on existing files >50KB are automatically denied.

## Reading Files

- For files >500 lines, use Read with `offset` and `limit` to read ~200-line chunks at a time.
- Read only the section you need, not the entire file.
- For code navigation, prefer LSP tools (`documentSymbol`, `goToDefinition`, `findReferences`) over reading whole files.
- Use Grep/Glob to locate relevant sections before reading.
