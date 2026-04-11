---
title: "CWE-22 — Path Traversal (Directory Traversal)"
---

# CWE-22: Path Traversal

CWE-22 describes the weakness where software uses external input to
construct a file path without sufficiently neutralising special elements
such as `../` sequences that can resolve to a location outside the
intended directory.

## Common Attack Patterns

An attacker submits a crafted filename like `../../etc/passwd` or URL-
encoded variants (`%2e%2e%2f`) to read arbitrary files on the server.
In write-capable endpoints, the same technique can overwrite sensitive
configuration files.

## Affected Languages and Frameworks

Any language that concatenates user input into a file path without
canonicalisation is vulnerable. Historically exploited in Java servlet
containers, Node.js servers, and Python web frameworks.

## Mitigation

Resolve the canonical path after applying user input and verify that the
result is within the expected base directory. Reject requests where the
canonicalised path escapes the root. Prefer a secure file-access
abstraction that enforces a chroot-style boundary.
