---
title: "CVE-2025-29824 — Windows CLFS Use-After-Free Privilege Escalation (Ransomware Zero-Day)"
published_date: 2025-04-08
---

# Windows Common Log File System — Use-After-Free Privilege Escalation (CVE-2025-29824)

CVE-2025-29824 is a use-after-free vulnerability in the Windows Common Log File System (CLFS) kernel driver (`clfs.sys`), patched by Microsoft on April 8, 2025 (Patch Tuesday). The flaw allows a locally authenticated attacker with standard user privileges to escalate to SYSTEM. CVSS score: 7.8 (High). Microsoft confirmed active zero-day exploitation before patch release, with the Storm-2460 threat group leveraging the vulnerability to deploy Play ransomware and the Grixba infostealer. The vulnerability affects Windows 10 (multiple versions), Windows 11, and Windows Server 2008 R2 through 2025; Windows 11 24H2 was not affected by the observed exploitation.

## Technical Details

CVE-2025-29824 is classified as CWE-416 (Use After Free). The exploit targets the Windows CLFS kernel driver through manipulation of log file structures in kernel memory. An attacker who has already achieved code execution as a standard user can trigger the use-after-free condition to corrupt kernel memory, enabling arbitrary kernel memory writes and subsequent privilege escalation to SYSTEM. Microsoft's threat intelligence team (MSTIC) identified the exploit as being delivered via the PipeMagic trojan — a plugin-based backdoor distributed through trojanized applications. PipeMagic downloads the CLFS exploit payload and executes it to perform privilege escalation before deploying ransomware. Affected sectors include IT and real estate (U.S.), financial services (Venezuela), software development (Spain), and retail (Saudi Arabia).

## Impact

CVE-2025-29824 was exploited as a zero-day in ransomware attacks by the Balloonfly threat group (operating Play ransomware) and Storm-2460. The vulnerability enables a standard user account on a compromised system to escalate to SYSTEM, unlocking full deployment capabilities for ransomware, credential dumping, and lateral movement tooling. CISA added the vulnerability to the KEV catalog with a remediation deadline of April 29, 2025. The CLFS driver has been a recurring ransomware exploitation target: earlier CLFS zero-days (CVE-2022-24521, CVE-2023-28252) were similarly exploited in ransomware campaigns.

## Mitigation

Apply the April 2025 Patch Tuesday update (KB articles vary by Windows version). Prioritize patching for Windows 10 and Windows Server 2019/2022 systems, which were observed in exploitation. Hunt for PipeMagic indicators: trojanized application installers distributing fake or modified software. Monitor for CLFS driver anomalies using kernel auditing. Detect Grixba infostealer artifacts in post-exploitation forensics. Windows 11 24H2 users should still patch as the vulnerability is present even if the specific exploit variant did not affect it.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-29824
- https://www.microsoft.com/en-us/security/blog/2025/04/08/exploitation-of-clfs-zero-day-leads-to-ransomware-activity/
