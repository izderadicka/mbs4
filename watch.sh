# cd crates/mbs4-server
RUST_LOG=debug cargo watch -- cargo run -p mbs4-server -- --oidc-config ./test-data/oidc-config.toml
