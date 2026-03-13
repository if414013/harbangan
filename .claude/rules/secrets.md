# Secret Handling Rules

Applies to all files in the repository.

## Never Write Real Credentials

- Never write real API keys, passwords, tokens, or secrets into any file
- Use environment variables (`std::env::var`, `process.env`) for all secrets
- Use placeholder values in examples: `your-api-key-here`, `changeme`, `<secret>`
- `.env.example` must only contain placeholder values, never real credentials

## Refuse to Echo Secrets

- If a user provides a real credential in chat, do not repeat it back or write it to any file
- Suggest they set it via environment variable or `.env` instead

## Never Stage Sensitive Files

Do not `git add` or `git commit` any of these:
- `.env`, `.env.local`, `.env.proxy` (only `.env.example` is safe)
- `certs/`, `*.pem`, `*.key` — TLS certificates and private keys
- `e2e-tests/.auth/` — session cookies and auth state
- `*.p12`, `*.pfx`, `*.jks` — keystores
- Files containing `BEGIN.*PRIVATE KEY`

## Test Data

- Test fixtures must use obviously fake values (e.g., `test-key-123`, `fake-token`)
- Never copy production credentials into test files
- The Qwen OAuth client ID `f0304373b74a44d2b584a3fb70ca9e56` is a public device flow ID, not a secret
