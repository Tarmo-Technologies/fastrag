---
title: "CVE-2026-24858 — Fortinet FortiOS FortiCloud SSO Authentication Bypass"
published_date: 2026-01-28
---

# Fortinet FortiOS — FortiCloud SSO Authentication Bypass Zero-Day (CVE-2026-24858)

CVE-2026-24858 is a critical authentication bypass vulnerability in Fortinet FortiOS, FortiManager, FortiWeb, FortiProxy, and FortiAnalyzer affecting the FortiCloud Single Sign-On (SSO) authentication mechanism, publicly disclosed and patched on January 28, 2026. CVSS score: 9.4 (Critical). The vulnerability is classified as CWE-288 (Authentication Bypass Using an Alternate Path or Channel). CISA added CVE-2026-24858 to the Known Exploited Vulnerabilities (KEV) catalog on January 27, 2026, following reports from multiple Fortinet customers that attackers had gained unauthorized administrative access to FortiGate firewalls beginning January 20, 2026 — before Fortinet issued a patch.

## Technical Details

CVE-2026-24858 allows an attacker who holds a valid FortiCloud account — potentially obtained through credential theft, phishing, or a compromised third-party tenant — to authenticate to FortiOS devices registered to a different FortiCloud organization than their own. The underlying flaw is in the SAML-based FortiCloud SSO authentication flow: the implementation does not correctly validate that the authenticating identity's FortiCloud tenant matches the tenant associated with the target device. An attacker can craft a SAML assertion or manipulate the SSO redirect flow to authenticate as an administrative user on any FortiCloud-registered device, regardless of organizational boundary. Post-authentication, attackers immediately downloaded device configuration files (containing hashed credentials) and created local administrative accounts with usernames such as `cloud-noc@mail.io` and `cloud-init@mail.io` for persistence.

## Impact

CVE-2026-24858 was exploited as a zero-day against Fortinet enterprise customers beginning January 20, 2026. Attackers performed unauthorized firewall configuration changes, exfiltrated device configurations (which contain credential hashes and network topology information), created persistent local admin accounts, and modified VPN configurations to add new accounts for ongoing access. The FortiCloud SSO feature is not enabled by default, but is enabled automatically when administrators register a device to FortiCare via the GUI unless explicitly disabled. Organizations that registered FortiGate devices using the default GUI workflow were exposed without awareness. The attack pattern is consistent with network perimeter device targeting previously observed from Chinese-nexus threat actors.

## Mitigation

Apply the Fortinet patch released January 28, 2026. As an immediate workaround: disable FortiCloud SSO login on all FortiGate devices (System -> Settings -> toggle "Allow administrative login using FortiCloud SSO" to Off). Audit all local admin accounts for unauthorized additions; remove accounts matching the observed IOC patterns (`cloud-noc@mail.io`, `cloud-init@mail.io`). Rotate all administrative credentials and review VPN configuration for unauthorized accounts. Export and review device configurations for unauthorized changes. Review FortiGate logs for authentication events originating from unexpected IP addresses via the SSO path.

## References

- https://www.cisa.gov/news-events/alerts/2026/01/28/fortinet-releases-guidance-address-ongoing-exploitation-authentication-bypass-vulnerability-cve-2026
- https://socradar.io/blog/cve-2026-24858-fortinet-fortios-sso-patch/
