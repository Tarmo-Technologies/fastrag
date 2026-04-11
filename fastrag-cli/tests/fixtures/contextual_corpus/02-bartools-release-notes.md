# bartools 4.7 Release Notes

## What's new

bartools 4.7 ships with a new packet inspection mode and refreshed
dashboard widgets. Operators on the long-term support track will see
this release backported in two weeks.

## Installation

Run `bartools-installer --channel stable --version 4.7` on every
managed host. The installer is idempotent and re-runs cleanly.

## Compatibility

bartools 4.7 keeps wire-protocol compatibility with 4.5 and 4.6, so
mixed-version fleets continue to operate while the rollout progresses.
