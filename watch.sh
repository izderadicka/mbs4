# cd crates/mbs4-server
RUST_LOG=debug cargo watch -- cargo run -p mbs4-server -- --data-dir test-data --base-url http://localhost:5173 --base-backend-url http://localhost:3000 --cors
