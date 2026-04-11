---
title: "CVE-2020-1472 — Zerologon Netlogon Privilege Escalation"
---

# Microsoft Netlogon — Zerologon Privilege Escalation to Domain Admin

CVE-2020-1472, known as Zerologon, is a critical privilege escalation vulnerability in the Windows Netlogon Remote Protocol (MS-NRPC), disclosed by Secura researcher Tom Tervoort and patched in August 2020 (ADV200013). The vulnerability allows an unauthenticated attacker with network access to a domain controller to completely compromise an Active Directory domain by resetting the domain controller computer account password to an empty string. CVSS score: 10.0 (Critical).

## Technical Details

The flaw stems from the use of AES-CFB8 encryption in the Netlogon authentication protocol with a static all-zero initialization vector (IV). AES-CFB8 with an all-zero IV produces an all-zero ciphertext block with a probability of 1/256 for any given plaintext. The Netlogon authentication protocol uses an 8-byte client challenge; by sending ComputeNetlogonCredential with an all-zero challenge, an attacker has a 1-in-256 chance (expected ~256 attempts) of producing an all-zero authenticator that passes server verification. Once authenticated as the DC machine account, the attacker can call `NetrServerPasswordSet2` with an empty password hash to reset the domain controller account. This breaks domain functionality until repaired but grants DCSync rights.

## Impact

CVE-2020-1472 enables an unauthenticated network attacker to achieve full domain compromise. Once the domain controller machine account password is reset, the attacker can perform a DCSync attack (using secretsdump) to dump all domain hashes including the krbtgt account, enabling Golden Ticket forgery and persistent domain dominance. CISA Emergency Directive 20-04 required federal agencies to patch within 3 days. Multiple ransomware groups incorporated Zerologon into their attack chains for rapid domain takeover.

## Mitigation

Apply the August 2020 security update immediately, followed by the February 2021 enforcement phase update. Enable Secure Channel enforcement via group policy once all domain-joined devices are patched (`FullSecureChannelProtection=1`). Monitor Event IDs 5827–5831 for Netlogon connection denials indicating vulnerable clients attempting to connect. Detection tooling (e.g., zerologon-check by Secura) can verify exposure without exploitation.
