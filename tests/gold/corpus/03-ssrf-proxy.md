---
title: "SSRF in Proxy Middleware — Exploitation and Mitigation"
---

# Server-Side Request Forgery via Proxy Middleware

SSRF allows an attacker to coerce a server into making arbitrary outbound
HTTP requests on their behalf. In proxy middleware, the typical attack
path is a URL parameter that is forwarded without validation to the
internal network.

## Exploitation

An attacker submits a request with a URL pointing at internal metadata
endpoints (e.g. cloud instance metadata services) or internal admin
interfaces not exposed to the public internet.

## Mitigation

Enforce a strict allowlist of outbound hosts. Never forward user-supplied
URLs without validation. Block private IP ranges and link-local addresses
at the egress layer.
