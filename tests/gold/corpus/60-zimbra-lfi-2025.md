---
title: "CVE-2025-68645 — Zimbra Collaboration Webmail Classic Local File Inclusion"
published_date: 2025-12-23
---

# Zimbra Collaboration Suite — Webmail Classic UI Local File Inclusion (CVE-2025-68645)

CVE-2025-68645 is a local file inclusion (LFI) vulnerability in the Webmail Classic UI of Synacor Zimbra Collaboration Suite (ZCS), disclosed on December 23, 2025. The vulnerability allows an unauthenticated remote attacker to include and execute arbitrary files from the WebRoot directory, potentially leading to remote code execution. CVSS score: 8.8 (High). Affected versions: ZCS 10.0 (fixed in 10.0.18) and ZCS 10.1 (fixed in 10.1.13). CISA added CVE-2025-68645 to the Known Exploited Vulnerabilities (KEV) catalog based on confirmed active exploitation. Zimbra email servers have historically been high-value targets for nation-state actors due to the email content they hold.

## Technical Details

The vulnerability exists in the `RestFilter` servlet within the Webmail Classic UI. The servlet processes user-supplied request parameters without sufficient validation, allowing an unauthenticated remote attacker to craft requests to the `/h/rest` endpoint that manipulate file path resolution. By supplying specially constructed path parameters, an attacker can cause the servlet to include arbitrary files from within the Zimbra WebRoot directory using the underlying PHP-based web application. If attacker-controlled content (e.g., uploaded attachments or webshells placed via other means) exists within the reachable directory tree, the LFI can be escalated to remote code execution. The vulnerability is classified as CWE-22 (Path Traversal) and CWE-73 (External Control of File Name or Path).

## Impact

CVE-2025-68645 enables unauthenticated attackers to read sensitive files from the Zimbra WebRoot and, under conditions where attacker-controlled files exist on disk, execute arbitrary code. Zimbra Collaboration Suite is widely deployed in government, financial, and enterprise environments as an on-premises email platform. Nation-state actors — including groups attributed to Russian SVR and Chinese MSS — have a documented history of targeting Zimbra vulnerabilities for email collection and persistent access (see CVE-2022-41352 and CVE-2023-37580). CISA confirmed active exploitation in the wild. Organizations running ZCS 10.0 or 10.1 without the December 2025 patch are at immediate risk of compromise.

## Mitigation

Upgrade to Zimbra Collaboration Suite 10.0.18 or later (for ZCS 10.0 branch), or 10.1.13 or later (for ZCS 10.1 branch). Restrict network access to the Zimbra web interface to trusted IP ranges where possible; avoid direct internet exposure of the `/h/rest` endpoint. Audit Zimbra web application directories for unexpected files, especially PHP files or JSP files in directories writable by the web server. Review Zimbra access logs for anomalous requests to `/h/rest` with unusual path parameters. Consider deploying a WAF rule blocking path traversal patterns targeting the `RestFilter` servlet.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-68645
- https://wiki.zimbra.com/wiki/Zimbra_Security_Advisories
