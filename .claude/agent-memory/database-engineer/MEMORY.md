# Database Engineer Memory

## Schema Patterns & Gotchas

### Migration System
- All DDL lives in `backend/src/web_ui/config_db.rs` (~5360 lines total)
- No external migration files (no `migrations/` directory) -- everything is embedded Rust
- `run_migrations()` called from `ConfigDb::connect()` on every startup
- Versioning via `schema_version` table; current version: **24**
- v1 inline in run_migrations; v3+ as `migrate_to_vN()` methods; v4+ use transactions
- Never modify existing migration blocks -- always add new versioned blocks
- See `schema-inventory.md` for full table/column details

### Table Count: 21 active tables (mcp_clients dropped in v16)

### Provider ID Evolution
- v21 dropped all CHECK constraints; validation now in Rust `ProviderId::from_str()`
- Removed providers: gemini (v13), qwen (v22)

### Idempotency
- CREATE TABLE uses IF NOT EXISTS; ALTER ADD COLUMN uses IF NOT EXISTS
- Later constraints use DO $$ IF NOT EXISTS pattern
- Version inserts are not idempotent but MAX(version) check prevents re-execution

### run_migrations() Quirk
- v20/v21 share a max_version read (v21 uses stale value). Not a real bug but inconsistent.
