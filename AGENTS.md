# GESAC development

Read [README.md](README.md) for product scope. `src/client/` owns enrollment,
installation and Windows scheduled updates; `src/server/` owns HTTP and SQLite;
`src/admin.rs` owns publication; `src/model.rs` owns signed release formats;
`web/` owns the enterprise interface.

Use the pinned Rust toolchain. Run `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --release --locked`
and `cargo build --release --locked`. Tests use isolated state. Keep disposable
output in `.tmp/`; retain account credentials and service state with their owners.
Product execution and business data belong to the installed product.
