---
title: "CVE-2020-0601 — Windows CryptoAPI Certificate Validation Bypass (CurveBall)"
---

# Windows CryptoAPI — Elliptic Curve Certificate Spoofing (CurveBall)

CVE-2020-0601, known as CurveBall, is a critical cryptographic validation vulnerability in the Windows CryptoAPI (`Crypt32.dll`), disclosed and patched by Microsoft in January 2020 (MS Security Advisory ADV200001). Reported to Microsoft by the NSA and GCHQ, the flaw allows an attacker to spoof code signing certificates, HTTPS certificates, and Authenticode signatures by crafting a malicious elliptic curve cryptography (ECC) certificate that Windows incorrectly validates as trusted. CVSS score: 8.1 (High).

## Technical Details

The vulnerability exists in how `Crypt32.dll` validates elliptic curve public keys specified in certificates. When validating a certificate that uses an explicit ECC curve definition, Windows fails to fully validate that the curve parameters match the named curve (e.g., P-256). An attacker can create a certificate with a custom ECC curve whose generator point is set to a value derived from the attacker's own private key but appears to satisfy the chain of trust up to a trusted root certificate (e.g., a Microsoft root). Because Windows accepts the spoofed chain, the malicious certificate is treated as valid. Proof-of-concept code was publicly released by researcher Tal Be'ery within hours of disclosure.

## Impact

CVE-2020-0601 enables spoofing of ECC-signed code signing certificates and TLS server certificates on unpatched Windows systems. An adversary can sign malware to appear as if it were signed by a trusted software vendor, bypassing Windows Defender SmartScreen and Authenticode checks. Additionally, an attacker performing MitM can present a forged HTTPS certificate that browsers on unpatched Windows trust, enabling credential harvesting. Affected systems include Windows 10 and Windows Server 2016/2019.

## Mitigation

Apply the January 2020 Patch Tuesday update (KB4534273). Detection is possible by examining certificate chain validation in network traffic and code signing logs. Security tools that perform independent certificate validation outside `Crypt32.dll` are not vulnerable. Post-patch, audit any code signed with ECC certificates to confirm certificates were not compromised.
