---
title: "CVE-2022-26134 — Atlassian Confluence OGNL Injection Remote Code Execution"
published_date: 2022-06-02
---

# Atlassian Confluence — OGNL Injection RCE (Pre-Auth)

CVE-2022-26134 is a critical pre-authentication remote code execution vulnerability in Atlassian Confluence Server and Data Center, disclosed and actively exploited as a zero-day in June 2022. Patched in Confluence 7.4.17, 7.13.7, 7.14.3, 7.15.2, 7.16.4, 7.17.4, and 7.18.1. The flaw is an OGNL (Object-Graph Navigation Language) injection in the Confluence web framework, enabling unauthenticated attackers to execute arbitrary code as the `confluence` OS user. CVSS score: 10.0 (Critical). Volexity observed exploitation by a Chinese nation-state actor before the patch was available.

## Technical Details

The OGNL injection flaw exists in how Confluence processes certain HTTP request URIs. An attacker sends a specially crafted HTTP GET or POST request to a URI containing an OGNL expression (e.g., `/%24%7B...%7D/`), which is evaluated by the Confluence Web Work framework's action mapping layer without authentication checks. OGNL can invoke arbitrary Java objects and methods, including `java.lang.Runtime.exec()` for OS command execution. A typical payload executes a reverse shell or writes a webshell to the Confluence home directory. The attack is a single unauthenticated HTTP request.

## Impact

CVE-2022-26134 provides unauthenticated RCE on internet-exposed Confluence instances. Post-exploitation artifacts documented by Volexity include web shells (`web.shell`, `wshell.php`), the BEHINDER and CHOPPER webshell implants, and credential harvesting from Confluence database connections. Because Confluence stores credentials and sensitive project data, successful exploitation typically results in data exfiltration. Mass exploitation began within hours of PoC publication, scanning the entire internet for vulnerable instances.

## Mitigation

Upgrade to a patched Confluence version immediately. As an emergency workaround before patching: block requests matching OGNL injection patterns at WAF (`%24%7B`, `${`), restrict internet access to Confluence, or take the instance offline. Detection: scan for unexpected files in the Confluence home and web directories, unusual `confluence` user processes, and access log entries with `%24%7B` or URL-encoded OGNL markers.
