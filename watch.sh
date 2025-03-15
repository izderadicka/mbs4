# cd crates/mbs4-server
RUST_LOG=debug cargo watch -- cargo run -p mbs4-server -- --data-dir ./test-data
