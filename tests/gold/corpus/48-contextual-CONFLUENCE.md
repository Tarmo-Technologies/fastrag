---
title: "CVE-2022-26134 — Confluence OGNL Injection (Contextual)"
---

# The Collaboration Platform Expression Injection — Contextual Sample

This document intentionally omits the CVE identifier and product name from the body to exercise contextual retrieval. The title carries the identifying context.

The vulnerability was exploited by a state-sponsored threat actor before the vendor was notified, making it a true zero-day. The vendor initially published a mitigation advisory without a patch, then released the patch the following day. The window of zero-day exploitation and the brief period before organizations patched resulted in significant compromise of internet-exposed instances.

## The Exploitation Path

An unauthenticated attacker sends a single HTTP GET or POST request to the application with a specially crafted URI. The URI contains an expression in the web framework's expression language, enclosed in characters that signal the framework to evaluate the expression dynamically. Because no authentication is required to reach the vulnerable request routing code, the expression evaluates with the application's runtime privileges, enabling arbitrary Java code execution.

## Attacker Tools Observed

Security researchers documented multiple backdoor families deployed through this vulnerability. The threat actor used sophisticated implants capable of executing commands, managing files, and proxying connections. One implant used a custom authentication mechanism — a hardcoded HTTP header value — to prevent other attackers from hijacking the backdoor.

## Why This Class of Flaw is Dangerous

Expression injection flaws are particularly dangerous because they are often pre-authentication, require minimal attacker skill to exploit once a proof-of-concept is available, and the resulting code execution runs in the context of the application server, which frequently has access to databases, internal network resources, and credential stores.

## Remediation

Upgrade to the patched version immediately. If immediate patching is not possible, block internet access to the application or take it offline. Scan for web shells and unexpected files in the application directories. Treat any exposed instance as fully compromised and conduct forensic analysis.
