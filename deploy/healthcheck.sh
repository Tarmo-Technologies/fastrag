#!/usr/bin/env bash
# Liveness probe for fastrag serve-http. Exit 0 if /health returns ok, nonzero otherwise.
set -euo pipefail

HOST="${FASTRAG_HOST:-127.0.0.1}"
PORT="${FASTRAG_PORT:-8081}"

response=$(curl -fsS --max-time 5 "http://${HOST}:${PORT}/health")
echo "$response" | grep -q '"status":"ok"'
