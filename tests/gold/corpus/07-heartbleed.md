---
title: "CVE-2014-0160 — OpenSSL Heartbleed Memory Disclosure"
published_date: 2014-04-07
---

# OpenSSL — Heartbleed Memory Disclosure

CVE-2014-0160, known as Heartbleed, is a critical information disclosure vulnerability in the OpenSSL cryptographic library affecting versions 1.0.1 through 1.0.1f and 1.0.2-beta. Disclosed in April 2014 by Neel Mehta of Google Security and independently by Codenomicon, the flaw allows an attacker to read up to 64 KB of memory from a process using a vulnerable OpenSSL build per request, with requests repeatable indefinitely.

## Technical Details

The vulnerability exists in the TLS/DTLS heartbeat extension (RFC 6520) implementation. A client sends a HeartbeatRequest containing a payload and a length field. OpenSSL failed to validate that the declared payload length matched the actual payload length before copying memory into the response buffer using `memcpy`. An attacker sends a 1-byte payload but declares a length of 65535, causing OpenSSL to read 64 KB of adjacent heap memory into the response. The bug was introduced in OpenSSL 1.0.1 (released March 2012) and was present for over two years before disclosure. The flaw is in `ssl/d1_both.c` and `ssl/t1_lib.c`.

## Impact

Heartbleed enables unauthenticated remote memory disclosure without leaving traces in server logs. Leaked memory may contain private TLS session keys, private RSA/DSA keys, usernames, passwords, session tokens, and other sensitive application data. Private key disclosure means all historical encrypted traffic captured by adversaries could be decrypted retroactively. Hundreds of thousands of servers were affected at disclosure. Major sites including Yahoo, LastPass, and numerous banks were confirmed vulnerable.

## Mitigation

Upgrade to OpenSSL 1.0.1g or later, or recompile with `-DOPENSSL_NO_HEARTBEATS`. Revoke and reissue TLS certificates after patching, as private keys may have been exfiltrated. Force re-authentication of all sessions. Detection: scan with publicly available Heartbleed scanners (e.g., `sslyze --heartbleed`). Network IDS signatures exist for the malformed heartbeat pattern.
