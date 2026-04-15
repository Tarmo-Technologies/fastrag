---
title: "CVE-2021-26855 — ProxyLogon Exchange SSRF (Contextual)"
published_date: 2021-03-02
---

# The Mail Server Pre-Auth Request Forgery Chain — Contextual Sample

This document intentionally omits the CVE identifier and product name from the body to exercise contextual retrieval. The title carries the identifying context.

This vulnerability was exploited as a zero-day by a Chinese state-sponsored threat actor before the patch was released. The campaign targeted defense contractors, law firms, and infectious disease researchers. Tens of thousands of servers worldwide were compromised in the weeks following public disclosure as multiple threat actor groups piled on.

## The Vulnerability Chain

The first flaw is a server-side request forgery in the authentication layer of the mail server's web interface. A cookie value controls which backend server handles the proxied request. By forging this cookie, an attacker can make the frontend server forward requests to arbitrary backend endpoints, authenticated as the machine's highest-privilege account. This bypasses the normal authentication requirement entirely.

## Chaining to Code Execution

A second vulnerability, chained with the first, allows writing a file to an arbitrary path on the server's disk via an endpoint that should only be accessible after authentication. By combining the authentication bypass with the file write, an attacker can place a web shell in the server's web root directory. The shell then provides interactive command execution.

## Post-Exploitation Activity

Threat actors deployed multiple backdoor families after gaining access — custom web shells, remote access tools, and credential harvesting utilities. Email data was exfiltrated in many cases. Because the mail server processes all organizational email and often has Active Directory integration, compromise frequently led to broader network access.

## Remediation

Apply the emergency security update. Use the vendor-provided script to detect and remove web shells from the installation directory. Audit recently created files in the web application directories. Reset all credentials used by the mail server to access directory services.
