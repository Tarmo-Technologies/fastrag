---
title: "CVE-2017-0144 — EternalBlue SMBv1 Remote Code Execution"
published_date: 2017-03-14
---

# Microsoft SMBv1 — EternalBlue Remote Code Execution

CVE-2017-0144 is a critical remote code execution vulnerability in Microsoft's implementation of the SMBv1 protocol, patched in MS17-010 (March 2017). The exploit, codenamed EternalBlue, was allegedly developed by the NSA and leaked by the Shadow Brokers in April 2017. It enables unauthenticated remote code execution against Windows systems with SMBv1 enabled and the SMB port (445/TCP) accessible. CVSS score: 9.3 (Critical). It was subsequently weaponized in the WannaCry and NotPetya global cyberattacks.

## Technical Details

EternalBlue exploits an integer overflow in the SMBv1 `Transaction2` request handler in `srv.sys`. Specifically, the vulnerability involves incorrect buffer size calculations when processing `SetupCount` and `TotalParameterCount` fields in an SMB_COM_TRANSACTION2 request. The overflow allows an attacker to write attacker-controlled data into the Windows kernel pool heap, leading to kernel-mode code execution. The exploit requires no authentication and works over network via TCP port 445. The DoublePulsar kernel backdoor implant was typically deployed as a second stage. Metasploit module `exploit/windows/smb/ms17_010_eternalblue` implements this attack.

## Impact

CVE-2017-0144 provides unauthenticated SYSTEM-level remote code execution on vulnerable Windows XP through Windows Server 2008 R2 systems. The WannaCry ransomware (May 2017) used EternalBlue to spread laterally across networks, infecting over 200,000 systems in 150 countries within 24 hours and causing estimated damage of $4–8 billion USD. NotPetya, deployed in June 2017, used EternalBlue combined with Mimikatz credential harvesting for worm propagation, causing over $10 billion in damage.

## Mitigation

Apply MS17-010 immediately. Disable SMBv1 via PowerShell: `Set-SmbServerConfiguration -EnableSMB1Protocol $false`. Block port 445/TCP at network perimeter and between workstations (lateral movement prevention). Enable Windows Firewall rules restricting SMB access. For unsupported systems (XP, 2003), Microsoft released emergency patches. Detection: Zeek/Suricata signatures for Transaction2 anomalies; event log 4625/4776 for failed auth attempts preceding SMB exploitation.
