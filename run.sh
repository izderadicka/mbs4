if [ -z "$1" ]; then
    RUST_LOG=debug cargo run -p mbs4-server -- --data-dir test-data --base-url http://localhost:5173 --base-backend-url http://localhost:3000 --cors
else 
    RUST_LOG=debug cargo run -p mbs4-server -- --data-dir test-data --static-dir ../mbs4-client/build
fi
