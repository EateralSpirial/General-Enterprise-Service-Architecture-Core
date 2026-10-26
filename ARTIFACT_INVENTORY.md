# Artifact inventory

| Path | Owner and purpose | Retention |
| --- | --- | --- |
| `src/server/`, `src/node/`, `src/runtime.rs`, `src/database.rs`, `src/commands.rs` | GESAC runtime, account/catalogue and node lifecycle source | Retain with source |
| `docs/runtime.md` | Runtime and ownership contract | Retain |
| `docs/IMPLEMENTATION.md` | Validation and continuation checkpoint | Retain and reconcile after changes |
| `target/` | Cargo build output | Disposable after selected binaries are retained |
| `.tmp/` | Isolated test state, downloaded Redis source and build output | Remove unreferenced successful fixtures after recovery review |
| `private/` | Operator-selected deployments and private state; excluded from Git | Retain active installations, databases and recovery copies |
| `<server-root>/server.sqlite`, `server.json`, `signing-key`, `artifacts/` | Independent service-host accounts, releases, trust and receipts | Retain while the service host or its clients depend on them |
| `<node-root>/node.sqlite`, `node.json`, `profiles/`, `releases/`, `bin/` | Device identity, host profiles, journals and verified payloads | Preserve across updates; node uninstall owns removal |
| `<node-root>/update-state.json`, `bootstrap/` | Independent self-update selection and recovery | Preserve across updates and rollback |
| Platform Secret Service / DPAPI / Keychain records | Per-profile credentials and sessions | Remove with the owning profile or node |
| Sibling `.gesac-cleaner-*`, `.gesac-removed-*` | Independent self-uninstall work | Cleaner removes successful work; retain failed work for reconciliation |
