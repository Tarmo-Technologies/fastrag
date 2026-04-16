---
title: "CVE-2025-21418 — Windows Ancillary Function Driver WinSock Privilege Escalation"
published_date: 2025-02-11
---

# Windows Ancillary Function Driver — WinSock Heap Overflow Privilege Escalation (CVE-2025-21418)

CVE-2025-21418 is a heap-based buffer overflow privilege escalation vulnerability in the Windows Ancillary Function Driver for WinSock (`afd.sys`), patched by Microsoft on February 11, 2025 (Patch Tuesday). The vulnerability allows a local attacker to escalate from a standard user account to SYSTEM privileges. CVSS score: 7.8 (Important). Microsoft confirmed active zero-day exploitation before the patch release. CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog. The vulnerability affects all supported Windows releases including Windows 10, Windows 11, and Windows Server 2008 R2 through Windows Server 2025.

## Technical Details

CVE-2025-21418 is a heap-based buffer overflow (CWE-122) in `afd.sys`, the kernel driver that implements the Windows Sockets (Winsock) Ancillary Function Driver. The driver is responsible for implementing socket I/O control operations and is loaded on all Windows systems. An attacker who already has code execution as a low-privileged user can trigger the heap overflow through crafted `DeviceIoControl` calls to the AFD driver, corrupting adjacent kernel heap allocations and ultimately redirecting execution to attacker-controlled code running at kernel privilege (SYSTEM). The precise triggering sequence was not publicly disclosed by Microsoft, but the exploitation pattern is consistent with techniques used in prior AFD driver vulnerabilities exploited in ransomware campaigns.

## Impact

CVE-2025-21418 provides a reliable local privilege escalation to SYSTEM on all supported Windows versions, making it a high-value second-stage payload for attackers who have already gained initial access. The vulnerability was exploited as a zero-day in live attacks before February 11, 2025. CISA's KEV listing confirms federal agency exploitation. As a kernel driver vulnerability affecting a universally loaded component (`afd.sys`), the privilege escalation is reliable and does not depend on specific application configurations. The flaw complements initial access payloads such as phishing, web exploitation, or supply chain compromises that land with limited privileges.

## Mitigation

Apply the February 2025 Patch Tuesday security update. Prioritize patching on externally accessible systems and systems running high-privilege applications. Implement Credential Guard and Windows Defender Exploit Guard attack surface reduction rules to limit post-exploitation capabilities. Monitor for anomalous `DeviceIoControl` calls to `afd.sys` via kernel telemetry (ETW, Sysmon driver activity events). Endpoint Detection and Response (EDR) tools with kernel visibility should detect exploitation attempts. Consider application whitelisting on sensitive systems to prevent untrusted code from executing in the first place.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-21418
- https://www.tenable.com/blog/microsofts-february-2025-patch-tuesday-addresses-55-cves-cve-2025-21418-cve-2025-21391
