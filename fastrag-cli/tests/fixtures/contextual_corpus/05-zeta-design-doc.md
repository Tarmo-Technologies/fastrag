# zeta — Storage Tier Design Doc

## Goals

The zeta storage tier separates hot and cold data so that the working
set fits in fast NVMe while archival objects spill to bulk disk. The
goal is single-digit-millisecond reads for the hot tier without
sacrificing capacity overall.

## Approach

A bloom filter in front of the cold tier shortcuts the lookup path for
keys that have already been demoted. Promotions happen on read using a
lazy admission policy.
