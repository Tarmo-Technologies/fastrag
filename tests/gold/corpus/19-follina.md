---
title: "CVE-2022-30190 — Follina MSDT Remote Code Execution via DOCX"
published_date: 2022-06-01
---

# Microsoft MSDT — Follina Remote Code Execution via Office Documents

CVE-2022-30190, known as Follina, is a zero-day remote code execution vulnerability in the Microsoft Support Diagnostic Tool (MSDT), disclosed in May 2022 and patched in June 2022 (KB5014699). The vulnerability allows code execution via a specially crafted Office document (`.docx` or `.rtf`) that references an external `ms-ms://` URI, triggering MSDT without user opening a macro or enabling editing. Exploitation requires the victim to open or preview the document. CVSS score: 7.8 (High). Observed exploitation in the wild before patch availability, attributed to multiple threat actors including nation-state groups.

## Technical Details

Office documents can include external OLE references via the `word/_rels/document.xml.rels` relationship file. A crafted `docx` references an HTML file via an external URI (`http://attacker.com/payload.html`). The HTML file contains a `ms-msdt:` URI with a PowerShell payload embedded in the `PCWDiagnostic` IT parameter. When Office's `msmsdt.exe` processes the URI, it passes the payload to PowerShell via the `/skip force` parameter. The `ms-msdt:` protocol handler bypasses Protected View in some configurations when the file comes via email preview. The attack works even without macros, bypassing macro-based defenses. The vulnerability class is improper input validation (CWE-20) in the MSDT protocol handler.

## Impact

CVE-2022-30190 enables RCE via document preview, bypassing macro execution controls that many organizations relied on as a primary defense against document-based attacks. Threat actors weaponized this vulnerability in spear-phishing campaigns targeting governments and financial institutions. Chinese APT group TA413 and Russian-nexus groups were among those observed exploiting Follina in targeted attacks within weeks of public disclosure.

## Mitigation

Apply the June 2022 patch (KB5014699). As an immediate workaround, disable the `ms-msdt` URI handler via registry: `reg delete HKEY_CLASSES_ROOT\ms-ms// /f`. Block `msdt.exe` from executing via AppLocker or WDAC policies. Disable Office external content loading. Detection: monitor for `msdt.exe` spawned by Office processes (`winword.exe`, `excel.exe`); Sysmon Event ID 1 with parent process matching Office applications.
