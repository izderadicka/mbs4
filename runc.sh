#!/usr/bin/bash

docker run --rm --name mbs4  -p 3000:3000 -v $(pwd)/test-data/:/data -v $(pwd)/../mbs4-client/build:/client \
-u $(id -u):$(id -g) -e RUST_LOG=debug   mbs4 --static-dir /client --cors