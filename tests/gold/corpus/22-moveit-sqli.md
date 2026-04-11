---
title: "CVE-2023-34362 — MOVEit Transfer SQL Injection to Remote Code Execution"
---

# Progress MOVEit Transfer — SQL Injection to RCE

CVE-2023-34362 is a critical SQL injection vulnerability in Progress MOVEit Transfer, a managed file transfer application, exploited as a zero-day beginning May 27, 2023. The Cl0p ransomware group conducted a mass exploitation campaign against thousands of organizations worldwide. Affected: all MOVEit Transfer versions before 2021.0.6, 2021.1.4, 2022.0.4, 2022.1.5, and 2023.0.1. CVSS score: 9.8 (Critical).

## Technical Details

The vulnerability exists in MOVEit Transfer's web application component (`moveitisapi.dll`). An unauthenticated attacker can send crafted HTTP POST requests to the `/human.aspx` endpoint containing SQL injection payloads that manipulate the application's database queries. The SQL injection exploits allowed attackers to enumerate database contents including user credentials, download file transfer metadata, and — through chained techniques — deploy ASPX webshells to the MOVEit web root. The Cl0p group's initial access was via the LEMURLOOT webshell, a custom ASPX backdoor that authenticated via a hardcoded `X-siLock-Comment` HTTP header before executing database queries and file operations. The LEMURLOOT webshell also harvested Azure Blob Storage credentials from MOVEit configuration.

## Impact

CVE-2023-34362 resulted in data exfiltration from thousands of organizations including US government agencies, airlines, financial institutions, and healthcare providers. The Cl0p group claimed to have stolen data from over 2,500 organizations. Compromised organizations include the US Department of Energy, Shell, British Airways, BBC, and Boots. The breach exposed personnel records, payroll data, and customer financial information. CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog.

## Mitigation

Apply the vendor patches immediately and firewall MOVEit Transfer from internet access during remediation. Hunt for LEMURLOOT and other webshells in the MOVEit web root (`C:\MOVEit Transfer\wwwroot\`). Audit Azure Blob Storage credentials configured in MOVEit. Review database logs and file transfer audit logs for unauthorized access. Rotate all MOVEit service account credentials. Consider the MOVEit Transfer incident a full breach requiring forensic investigation.
