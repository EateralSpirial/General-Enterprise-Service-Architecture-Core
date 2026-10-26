# GESAC development

GESAC owns a general software delivery and evolution plane: service-host accounts, signed service plans, node subscriptions, installations, coordinated updates, offline recovery and self-removal. PAUH Daemon and its OS adapters are one managed product. Product adapters retain their business state and runtime ownership.

Read `docs/runtime.md` for the executable contracts and `docs/IMPLEMENTATION.md` for validation and continuation. Keep server, node, coordination and lifecycle code under their corresponding `src/` owners. GESAC stores its own SQLite databases; Redis coordinates online operations. Treat uncertain execution as a reconciliation state with the original operation identity.

Use `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked`. Integration tests use isolated accounts, stores and ports. Preserve existing private credentials and managed-product state. Keep generated state under ignored `.tmp/` or the operator-selected private installation.
