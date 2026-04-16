---
title: "CVE-2025-22457 — Ivanti Connect Secure Second Stack Overflow RCE Wave"
published_date: 2025-04-03
---

# Ivanti Connect Secure — Second Pre-Authentication RCE (CVE-2025-22457)

CVE-2025-22457 is a critical stack-based buffer overflow in Ivanti Connect Secure, Ivanti Policy Secure, and Ivanti ZTA Gateways, publicly disclosed on April 3, 2025. The vulnerability allows a remote unauthenticated attacker to execute arbitrary code on the appliance. Affected versions: Ivanti Connect Secure before 22.7R2.6, Ivanti Policy Secure before 22.7R1.4, and Ivanti ZTA Gateways before 22.8R2.2. CVSS score: 9.8 (Critical, per NIST NVD). Mandiant published research attributing active exploitation since mid-March 2025 to UNC5221, a suspected China-nexus threat group linked to prior Ivanti zero-day campaigns.

## Technical Details

CVE-2025-22457 is a stack-based buffer overflow (CWE-787 / CWE-121) in the ICS web server. The overflow is triggered by a specially crafted HTTP request containing an oversized `X-Forwarded-For` header value. Ivanti initially patched the overflow in February 2025 (ICS version 22.7R2.6) and assessed it as non-exploitable due to character restrictions — the overflow payload was limited to period and digit characters (`0-9` and `.`). However, threat actors reverse-engineered the patch, identified the underlying offset, and developed a working exploit by chaining multiple header fields to achieve reliable RCE on 22.7R2.5 and earlier. Post-exploitation activity included deployment of TRAILBLAZE (in-memory dropper) and BRUSHFIRE (passive backdoor), as well as a new malware family named SPAWNCHIMERA.

## Impact

CVE-2025-22457 enables pre-authentication remote code execution on internet-facing Ivanti appliances. CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog on April 4, 2025, with a remediation deadline of April 11, 2025. At the time of public disclosure, over 5,000 ICS appliances were assessed as vulnerable. UNC5221's exploitation pattern targets government, defense, telecommunications, and financial sector organizations for intelligence collection. The attack represents a third successive Ivanti Connect Secure zero-day exploited by the same threat cluster within 18 months.

## Mitigation

Upgrade Ivanti Connect Secure to 22.7R2.6 or later. Run the Ivanti Integrity Checker Tool (ICT) both before and after patching to detect compromise indicators. Perform a factory reset if compromise is suspected, as patching an already-compromised appliance does not remove attacker footholds. Hunt for TRAILBLAZE, BRUSHFIRE, and SPAWNCHIMERA indicators of compromise. Review VPN authentication logs for anomalous sessions originating from known-bad IPs. Restrict ICS management interfaces from direct internet exposure.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-22457
- https://cloud.google.com/blog/topics/threat-intelligence/china-nexus-exploiting-critical-ivanti-vulnerability
