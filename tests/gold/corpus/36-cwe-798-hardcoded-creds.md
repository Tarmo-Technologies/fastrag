---
title: "CWE-798 — Hardcoded Credentials: Embedded Authentication Secrets"
---

# CWE-798 — Use of Hard-coded Credentials

CWE-798 (Use of Hard-coded Credentials) describes software that contains hardcoded usernames, passwords, cryptographic keys, or API tokens embedded in source code, binaries, or configuration files. Hardcoded credentials cannot be changed without modifying the code, and once discovered (via reverse engineering, code leak, or public repository exposure), they grant persistent unauthorized access.

## Technical Details

Hardcoded credentials appear in multiple forms: **Default vendor credentials** — factory-default usernames and passwords (admin/admin, admin/password) in routers, cameras, and embedded devices. **Backdoor accounts** — intentional or accidental accounts with hardcoded passwords for debugging or maintenance access. **Embedded API keys** — cloud provider keys, database connection strings, or third-party service tokens hardcoded in source. **Cryptographic keys** — private keys or HMAC secrets hardcoded in code or configuration.

Hardcoded credentials are frequently discovered via: GitHub Dorks (searching public repositories for connection strings, API keys), reverse engineering of firmware or compiled binaries (`strings`, `binwalk`, Ghidra analysis), Docker image layer inspection, and JavaScript source analysis in web applications. Tools like Gitleaks, TruffleHog, and detect-secrets automate credential scanning. The Shodan search engine indexes internet-exposed devices with default credentials.

## Notable Cases

CVE-2024-3094 (xz-utils) featured an intentional backdoor that effectively hardcoded the attacker's authentication key. Numerous ICS/SCADA vendors have disclosed CVEs for hardcoded credentials in control system software. The Mirai botnet (2016) exploited hardcoded credentials in IoT devices to build a massive DDoS botnet. Samsung and Cisco have had multiple CVEs for hardcoded credentials in networking equipment.

## Mitigation

Never hardcode credentials in source code or binaries. Use secrets management systems (HashiCorp Vault, AWS Secrets Manager, Azure Key Vault). Store credentials in environment variables or encrypted credential stores, never in version control. Implement pre-commit hooks with credential scanning (Gitleaks, git-secrets). Rotate all credentials if a code repository is ever exposed publicly. For vendor products, require credential change on first login and enforce password complexity.
