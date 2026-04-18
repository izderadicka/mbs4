METRICS_TOKEN=test-token

if [ -z "$1" ]; then
    RUST_LOG=debug cargo run -p mbs4-server -- --data-dir test-data --base-url http://localhost:5173 --base-backend-url http://localhost:3000 --cors --metrics-token $METRICS_TOKEN
else 
    RUST_LOG=debug cargo run -p mbs4-server -- --data-dir test-data --static-dir ../mbs4-client/build --metrics-token $METRICS_TOKEN
fi
