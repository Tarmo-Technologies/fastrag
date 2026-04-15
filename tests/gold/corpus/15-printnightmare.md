---
title: "CVE-2021-34527 — Windows Print Spooler PrintNightmare Remote Code Execution"
published_date: 2021-07-01
last_modified: 2021-07-07
---

# Windows Print Spooler — PrintNightmare Remote Code Execution

CVE-2021-34527, known as PrintNightmare, is a critical remote code execution and local privilege escalation vulnerability in the Windows Print Spooler service (`spoolsv.exe`), publicly disclosed in June 2021 via an accidental GitHub publication of proof-of-concept code. Patched by Microsoft in July 2021 (KB5004945), the vulnerability allows authenticated remote attackers to execute code as SYSTEM by abusing the `RpcAddPrinterDriverEx()` function to load a malicious DLL. CVSS score: 8.8 (High, but effectively critical for LPE).

## Technical Details

The Windows Print Spooler service exposes the `MS-RPRN` RPC interface, which includes the `RpcAddPrinterDriver` and `RpcAddPrinterDriverEx` functions for installing printer drivers. CVE-2021-34527 exploits insufficient privilege checks in `RpcAddPrinterDriverEx()`, allowing any authenticated domain user (not just administrators) to specify a remote UNC path to a malicious DLL as the printer driver, which the Spooler service loads as SYSTEM. For the remote variant, the attacker needs network access to the Spooler RPC endpoint (TCP 445 or RPC dynamic ports). For the local privilege escalation variant (CVE-2021-1675), a local user can achieve SYSTEM via the same mechanism. Proof-of-concept tools include `nightmare-dll` and various PowerShell-based PoCs.

## Impact

PrintNightmare enables authenticated SYSTEM-level remote code execution on any Windows system with the Print Spooler service running and accessible over the network. In enterprise environments where all workstations have Spooler running, a single compromised domain account can be used to achieve SYSTEM on every workstation, enabling ransomware deployment and lateral movement. Domain controllers were particularly valuable targets. The Print Spooler has a long history of vulnerabilities (Stuxnet also abused it via CVE-2010-2568).

## Mitigation

Stop and disable the Print Spooler service on domain controllers and servers that do not require printing: `Stop-Service -Name Spooler; Set-Service -Name Spooler -StartupType Disabled`. Apply the July 2021 patch. If printing is required, enable the Group Policy setting "Limits print driver installation to Administrators". Monitor for `spoolsv.exe` loading DLLs from network paths and unusual child processes. Detection: Sysmon Event ID 7 for DLL loads by spoolsv.exe from non-standard paths.
