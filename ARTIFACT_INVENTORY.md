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
| `<node-root>/profiles/*/{credentials,session}*`, `.vault-key` | Explicit sealed headless vault records and local wrapping key | Private owner/ACL boundary; remove with the owning profile or node |
| `<node-root>/lifecycle.json`, `lifecycle-hook.mjs`, `registrations.json`, `service-removal.json` | Locally registered removal of exact owned external services, with recovery journal | Keep through removal/reconciliation; uninstall removes these only after the hook succeeds |
| Sibling `.gesac-cleaner-*`, `.gesac-removed-*` | Independent self-uninstall work | Cleaner removes successful work; retain failed work for reconciliation |

The owner's deployment has a distinct service-host root at `private/service-host/`, including private SQLite, TLS CA/leaf material, signing key, isolated Redis Unix socket/AOF and `runtime/` for the self-updating supervisor. The node root is `/home/sal/.local/share/gesac/node`; Windows and macOS installations are inventoried by PAUH. Linux user registrations are `gesac-redis.service`, `gesac-host.service`, `gesac-node-update.{service,timer}` and `gesac-host-update.{service,timer}`. Node removal owns its two update registrations; service-host runtime removal owns its four registrations and retains the external business database and signing material. Do not delete these roots as build output.

Task-owned Redis source/tooling, Cargo vendors and downloaded CI artifacts remain under `.tmp/`. On the physical Mac, task-owned Rust tooling and immutable source build inputs are under `~/.local/share/gesac/{tooling,build}/`; these are rebuildable caches with a separate lifetime from the selected node core. PAUH owns final validation and recovery receipts under its private deployment directory; no credentials are stored in this public inventory.
