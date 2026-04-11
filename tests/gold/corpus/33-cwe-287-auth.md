---
title: "CWE-287 — Improper Authentication: Authentication Bypass Patterns"
---

# CWE-287 — Improper Authentication

CWE-287 (Improper Authentication) covers vulnerabilities where a system fails to adequately verify that a user, process, or device is who it claims to be. Authentication failures range from trivially bypassable login checks to cryptographic authentication protocol weaknesses. It ranks in the CWE Top 25 Most Dangerous Software Weaknesses.

## Technical Details

Common authentication weakness patterns include: **Predictable tokens** — session identifiers or reset tokens generated from weak entropy sources (timestamps, sequential IDs, user attributes), enabling forging or guessing. **Insecure direct comparison** — comparing password hashes using non-constant-time comparison enables timing attacks to recover the hash byte-by-byte. **Type juggling** — in PHP, `==` comparison between `"0"` and `false` evaluates to true, enabling authentication bypass with type-confused inputs. **Algorithm confusion in JWT** — changing the algorithm from RS256 to HS256 allows signing tokens with the public key as the HMAC secret. **Missing authentication on secondary endpoints** — protecting the primary login page but leaving API or administrative endpoints unauthenticated.

## Notable Exploitation Patterns

CVE-2020-1472 (Zerologon) is a cryptographic authentication bypass in Netlogon. CVE-2019-11510 (Pulse VPN) achieves authentication bypass via path traversal. SQL injection (CWE-89) frequently leads to authentication bypass (`' OR 1=1 --`). Default credentials in IoT/embedded devices are a pervasive CWE-287 pattern. OAuth implementation flaws (state parameter CSRF, token substitution) represent modern authentication bypass variants.

## Mitigation

Implement multi-factor authentication for privileged accounts. Use established authentication frameworks rather than custom implementations. Generate session tokens with cryptographically secure random number generators (CSPRNG) of sufficient length (128+ bits). Use constant-time comparison for security tokens. Enforce authentication on all endpoints including administrative, API, and monitoring surfaces. Apply the principle of fail-secure: deny access when authentication state cannot be determined.
