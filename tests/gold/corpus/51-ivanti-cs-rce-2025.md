---
title: "CVE-2025-0282 — Ivanti Connect Secure Stack-Based Buffer Overflow RCE"
published_date: 2025-01-08
---

# Ivanti Connect Secure — Pre-Authentication Remote Code Execution (CVE-2025-0282)

CVE-2025-0282 is a critical stack-based buffer overflow in Ivanti Connect Secure (ICS) VPN appliances, publicly disclosed on January 8, 2025, alongside an emergency advisory from Ivanti. The vulnerability allows a remote unauthenticated attacker to achieve remote code execution on the appliance. Affected versions: Ivanti Connect Secure before 22.7R2.5, Ivanti Policy Secure before 22.7R1.2, and Ivanti Neurons for ZTA Gateways before 22.7R2.3. CVSS score: 9.0 (Critical). Mandiant attributed exploitation to UNC5221, a suspected China-nexus espionage threat actor, with evidence of exploitation beginning in late December 2024 — before public disclosure.

## Technical Details

The vulnerability is a stack-based buffer overflow (CWE-121 / CWE-787) in the ICS HTTP(S) web server binary (`/home/bin/web`). The flaw arises from improper bounds checking when processing IFT (IF-T/TLS) packets from unauthenticated clients. Crafted requests can overwrite fixed-length stack buffers and corrupt the execution flow to redirect control to attacker-controlled shellcode. Exploitation is constrained to characters limited to digits and dots (`0123456789.`) in the overflow payload — Ivanti initially assessed this restriction made RCE impossible, but threat actors demonstrated otherwise through a sophisticated exploitation chain. Following successful exploitation, attackers deployed two newly identified malware families: the TRAILBLAZE in-memory dropper and the BRUSHFIRE passive backdoor for persistent access.

## Impact

CVE-2025-0282 provides pre-authentication remote code execution on internet-exposed ICS appliances. Mandiant observed active exploitation against ICS 9.x (end-of-life) and 22.7R2.5 and earlier versions. CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog with a remediation deadline of January 15, 2025. Approximately 5,000 Ivanti VPN appliances remained vulnerable to the critical flaw after public disclosure. UNC5221 has a history of exploiting Ivanti zero-days (CVE-2023-46805 and CVE-2024-21887) for large-scale espionage campaigns against government and critical infrastructure targets.

## Mitigation

Upgrade Ivanti Connect Secure to version 22.7R2.5 or later. Run the Ivanti Integrity Checker Tool (ICT) to assess appliance compromise before patching, as Ivanti noted patching a compromised appliance does not evict an attacker. Perform a factory reset of the appliance if compromise is suspected. Rotate all credentials for accounts that may have authenticated through the appliance. Monitor for TRAILBLAZE and BRUSHFIRE malware indicators. Restrict ICS management interface access to internal networks and block direct internet exposure of the appliance.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-0282
- https://cloud.google.com/blog/topics/threat-intelligence/ivanti-connect-secure-vpn-zero-day
