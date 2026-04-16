---
title: "CVE-2025-24813 — Apache Tomcat Partial PUT Path Equivalence RCE"
published_date: 2025-03-10
last_modified: 2025-03-18
---

# Apache Tomcat — Partial PUT Deserialization Remote Code Execution (CVE-2025-24813)

CVE-2025-24813 is a remote code execution vulnerability in Apache Tomcat arising from improper handling of file paths during partial PUT requests. The vulnerability was originally disclosed on March 10, 2025, with an initial CVSS score of 5.5 (Medium). After a public proof-of-concept was released on March 13, NVD revised the score to 9.8 (Critical) on March 18, 2025. Affected versions: Apache Tomcat 9.0.0.M1 through 9.0.98, 10.1.0-M1 through 10.1.34, and 11.0.0-M1 through 11.0.2. Active exploitation was detected on March 12, 2025 — one day before PoC publication — confirming independent discovery and weaponization.

## Technical Details

The vulnerability is a path equivalence issue (CWE-706) in Tomcat's partial PUT implementation. When a PUT request uses the `Content-Range` header, Tomcat stores the uploaded data as a temporary session file in its session persistence directory. File names containing an internal dot character (e.g., `file.Name`) are treated as a serialized Java session file by the default file-based session persistence manager (`FileStore`). An attacker can upload a malicious serialized Java object payload via a partial PUT request to a target filename of their choice. A subsequent GET request with a crafted `JSESSIONID` cookie pointing to the uploaded filename triggers deserialization of the malicious payload, executing arbitrary Java code as the Tomcat process user. Exploitation requires partial PUT support to be enabled (not the default) and file-based session persistence.

## Impact

CVE-2025-24813 enables unauthenticated remote code execution on Tomcat servers with the vulnerable configuration. The first in-the-wild attack was detected in Poland by Wallarm security researchers on March 12, 2025 — prior to public PoC release — indicating independent exploitation by threat actors. Tomcat powers a large fraction of enterprise Java web applications, and many deployments inadvertently enable partial PUT support. CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog. Post-exploitation activity observed included webshell deployment and command execution for reconnaissance and lateral movement.

## Mitigation

Upgrade Apache Tomcat to 11.0.3, 10.1.35, or 9.0.99 or later. Disable partial PUT support if not required: set `readonly="true"` on the default servlet in `web.xml` or remove the partial PUT configuration. Avoid using file-based session persistence (`FileStore`) exposed to attacker-controlled input. Audit Tomcat session persistence directories for unexpected `.session` files. Apply WAF rules to block PUT requests to non-API endpoints. Monitor Tomcat access logs for PUT requests with `Content-Range` headers.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-24813
- https://www.rapid7.com/blog/post/2025/03/19/etr-apache-tomcat-cve-2025-24813-what-you-need-to-know/
