#!/usr/bin/bash

exec mbs4-server --listen-address 0.0.0.0 --port 3000 --data-dir  /data "$@"