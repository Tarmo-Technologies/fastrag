---
title: "CVE-2019-11510 — Pulse Secure VPN Arbitrary File Read"
---

# Pulse Secure VPN — Arbitrary File Read (Pre-Auth)

CVE-2019-11510 is a critical pre-authentication arbitrary file read vulnerability in Pulse Connect Secure VPN appliances, disclosed in April 2019 and patched in the SA44101 advisory. Affected versions include Pulse Connect Secure 8.1R before 8.1R15.1, 8.2 before 8.2R12.1, 8.3 before 8.3R7.1, and 9.0 before 9.0R3.4. CVSS score: 10.0 (Critical). The vulnerability was heavily exploited by nation-state actors including APT groups tied to Chinese intelligence.

## Technical Details

The flaw is a path traversal vulnerability in the Pulse Secure web interface. Unauthenticated HTTP requests to the URL `/dana-na/../dana/html5acc/guacamole/../../../../../../etc/passwd?/dana/html5acc/guacamole/` bypass authentication checks due to improper URL normalization and path traversal in the VPN gateway's web server component. Attackers can read arbitrary files from the appliance filesystem, including `/etc/passwd`, SSL certificate private keys, cached plaintext credentials stored in `/data/runtime/mtmp/lmdb/dataa/data.mdb`, and session tokens. The credential cache file often contains plaintext or weakly obfuscated Active Directory credentials.

## Impact

CVE-2019-11510 enables unauthenticated reading of sensitive files from the VPN appliance. Threat actors, particularly those attributed to APT5 and other Chinese-nexus groups, used this vulnerability to harvest VPN credentials and then authenticate legitimately to target networks. CISA Alert AA20-010A documented widespread exploitation. The NSA and CISA jointly published guidance on exploitation patterns. Post-exploitation typically leads to Active Directory compromise and lateral movement throughout the victim network. IRGC-affiliated actors also exploited this vulnerability.

## Mitigation

Apply the vendor patch (SA44101) immediately. Check appliance integrity using Pulse Secure's integrity checking tool. Reset all Active Directory and local account passwords associated with VPN users, as credentials may have been harvested. Review VPN access logs for anomalous access patterns predating the patch. CISA provided a detection tool for checking compromise indicators. Block unauthenticated access to `/dana-na/` paths at WAF level.
