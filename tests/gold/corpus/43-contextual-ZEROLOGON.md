---
title: "CVE-2020-1472 — Zerologon Netlogon (Contextual)"
published_date: 2020-08-17
---

# The Authentication Protocol Cryptographic Bypass — Contextual Sample

This document intentionally omits the CVE identifier and product/protocol name from the body to exercise contextual retrieval. The title provides the identifying context.

The vulnerability was discovered by a security researcher and disclosed in August 2020 with a published paper describing the mathematical flaw. A proof-of-concept exploit was published shortly after, and multiple ransomware groups integrated it into their attack chains within weeks.

## The Cryptographic Flaw

The authentication protocol in question uses a block cipher in CFB mode with an initialization vector that should be randomly generated for each session. The implementation, however, uses an all-zero initialization vector. With this IV, there is a 1-in-256 probability that any given plaintext input will produce an all-zero ciphertext. An attacker who sends an all-zero client challenge can therefore guess the correct authenticator value with an expected 256 connection attempts — a trivially fast brute-force over a network.

## What the Attacker Achieves

Once the attacker successfully authenticates by exploiting the statistical property, they can call a remote procedure that resets the machine account password of the targeted server to an empty string. When this is done against the primary domain controller, the attacker can then perform a full credential dump of the entire directory — including all user password hashes and the Kerberos service account hash. From there, persistent and privileged access to every system in the domain follows.

## Remediation

Apply the vendor patch, which enforces secure channel usage. Enable the enforcement mode group policy setting after patching all domain-joined systems. Monitor authentication event logs for connection failures that indicate systems still using the vulnerable code path. The patch involves two phases — an initial compatibility phase and a later enforcement phase.
