---
title: "libfoo 2.3.1 — CVE-2024-12345 Remote Code Execution"
---

# libfoo Advisory CVE-2024-12345

A critical buffer overflow in libfoo 2.3.1 allows remote code execution
via a crafted config file. The vulnerability is triggered during startup
when the library parses the user-supplied TOML header.

## Impact

Full remote code execution as the user running the process. The
vulnerability has been observed exploited in the wild against
internet-exposed daemons.

## Mitigation

Upgrade to libfoo 2.3.2 or later. No workaround is available for the
affected versions.
