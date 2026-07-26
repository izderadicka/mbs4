#!/bin/bash
METRICS_TOKEN=test-token

SERVE_CLIENT=0
EXTRA_ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --serve-client) SERVE_CLIENT=1 ;;
        *) EXTRA_ARGS+=("$1") ;;
    esac
    shift
done

if [ "$SERVE_CLIENT" -eq 1 ]; then
    MODE_ARGS=(--static-dir ../mbs4-client/build)
else
    MODE_ARGS=(--base-url http://localhost:5173 --base-backend-url http://localhost:3000 --cors)
fi

RUST_LOG=debug cargo run -p mbs4-server -- --data-dir test-data --metrics-token "$METRICS_TOKEN" "${MODE_ARGS[@]}" "${EXTRA_ARGS[@]}"
