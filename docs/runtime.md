# GESAC runtime contract

GESAC is a service-host and node system for signed software and content delivery. A service plan groups platform-specific products. Each node tracks multiple independent host/account/trust profiles. Each installed instance has one source profile and one local lifecycle adapter. PAUH supplies its existing maintenance adapter, retaining Core admission, backups, operation identity and shared Codex ownership.

Service-host SQLite owns accounts, hashed sessions, plans, grants, signed releases and operation receipts. Node SQLite owns profiles, subscriptions, installed instances and operation journals. Secrets belong to platform vaults. Redis holds online coordination claims and projections; a durable operation retains target ownership through uncertainty and survives a Redis restart. Offline staging and installation use signed local packages and node locks.

Server administration runs locally against the private server root. Network clients authenticate, list their granted plans, fetch signed releases/artifacts and create or reconcile account/device/instance update operations. A repeated request returns the original operation; conflicting identity reuse is rejected. Only a verified terminal receipt releases target admission.

Node adapters are locally registered executable/argument arrays. Remote manifests contain files and metadata, with execution authority supplied by the local adapter. Adapter input is JSON on stdin; results are bounded JSON. Prepare/activate/reconcile/rollback retain one operation ID. An adapter timeout retains uncertainty and requires reconciliation before another activation.

Self-updates retain the independent verified launcher and original bootstrap. Self-uninstall removes the node's own profile configuration, vault records, database, cache and binaries through an independent cleaner. Managed products and server business databases retain their explicit ownership. A node with unresolved updates holds removal until reconciled.

The service host can run as `server supervise` under a stable self-update Home. It starts a verified server child, observes committed core selection and adopts the selected executable through its own child lifecycle. TLS and HTTP servers drain on termination. The independent update clock performs one-shot signed self-updates; the supervisor retains the service-host database and its original root.

Profiles can explicitly select `credential_store: "sealed"` for a headless installation whose native vault is locked. AES-256-GCM binds each ciphertext to its profile/purpose. The local wrapping key uses the same private-file/ACL boundary as the installation; the current OS user remains trusted. Profile and node removal delete these records and their local key. Native OS vaults remain the default.

An installation can own external services through an operator-created private `lifecycle.json`. Its `uninstall` field is a locally registered adapter command. Node removal first requires that hook to return `status: "removed"`; a failure retains configuration and vault records for reconciliation. Installers register only their own scoped services in this hook.
