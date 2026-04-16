---
title: "CVE-2025-0108 — Palo Alto PAN-OS Management Interface Authentication Bypass"
published_date: 2025-02-12
---

# Palo Alto Networks PAN-OS — Management Web Interface Authentication Bypass (CVE-2025-0108)

CVE-2025-0108 is an authentication bypass vulnerability in the Palo Alto Networks PAN-OS management web interface, disclosed on February 12, 2025. The flaw allows an unauthenticated attacker with network access to the management interface to bypass authentication and invoke certain PHP scripts. CVSS score: 8.8 (High). CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog on February 18, 2025, after GreyNoise detected 26 unique IPs attempting exploitation within hours of proof-of-concept publication. The vulnerability affects PAN-OS 10.1.x before 10.1.14, 10.2.x before 10.2.14, 11.1.x before 11.1.6, and 11.2.x before 11.2.5 when the management web interface is exposed to the internet.

## Technical Details

CVE-2025-0108 arises from inconsistent URL handling between the Nginx reverse proxy and the Apache web server within the PAN-OS management interface. By crafting specially formed URLs, an attacker can bypass Nginx authentication checks while the request is still routed to the underlying PHP application. This technique — known as a path confusion or proxy misrouting attack — allows the attacker to invoke PHP management scripts that would otherwise require administrative credentials. While the bypass alone does not enable RCE, it can be combined with CVE-2024-9474 (a PAN-OS privilege escalation disclosed in November 2024) and CVE-2025-0111 (a PAN-OS file read vulnerability) to form a chained attack that achieves root-level access to the firewall. Researchers from Assetnote discovered the vulnerability.

## Impact

CVE-2025-0108 affects Palo Alto Networks next-generation firewall appliances with internet-exposed management interfaces. The Shadowserver Foundation identified approximately 3,300 PAN-OS management interfaces exposed to the internet at the time of disclosure. When chained with CVE-2024-9474 and CVE-2025-0111, the attack chain achieves root access on the firewall, enabling configuration exfiltration, VPN credential theft, network traffic interception, and persistent backdoor installation. Nation-state threat actors have previously targeted Palo Alto management interfaces (CVE-2024-3400 in 2024), and CVE-2025-0108 represents continued targeting of perimeter network security appliances.

## Mitigation

Upgrade to PAN-OS 10.1.14, 10.2.14, 11.1.6, 11.2.5, or later. Immediately restrict management interface access: configure firewall management profiles to allow only trusted internal IP ranges, never expose the PAN-OS web management interface directly to the internet. Apply Palo Alto's best practice assessment (BPA) recommendations for management plane isolation. Also patch CVE-2024-9474 and CVE-2025-0111 to eliminate the known exploit chain. Monitor management interface access logs for unexpected authentication attempts and PHP script invocations.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-0108
- https://security.paloaltonetworks.com/CVE-2025-0108
