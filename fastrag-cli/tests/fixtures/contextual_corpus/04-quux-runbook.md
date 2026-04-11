# quux Operations Runbook

## Daily checks

Inspect the queue depth chart and confirm it returned to baseline
overnight. If the depth stayed flat through the early morning, escalate
to the platform on-call rotation.

## Restart procedure

Drain the front-end nodes one at a time. Wait for in-flight requests to
complete, then bounce the worker pool. The dashboard should return to
green within five minutes.
