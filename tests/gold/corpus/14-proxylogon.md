---
title: "CVE-2021-26855 — Microsoft Exchange ProxyLogon SSRF to RCE"
---

# Microsoft Exchange — ProxyLogon Server-Side Request Forgery Chain

CVE-2021-26855 is the primary SSRF vulnerability in the ProxyLogon exploit chain affecting Microsoft Exchange Server 2013, 2016, and 2019, patched in March 2021 (KB5000871). Discovered by DEVCORE researcher Orange Tsai and exploited by HAFNIUM (attributed to Chinese state-sponsored actors) as a zero-day before patching. The SSRF flaw, combined with CVE-2021-27065 (arbitrary file write), enables unauthenticated remote code execution and webshell deployment. CVSS for CVE-2021-26855: 9.1 (Critical).

## Technical Details

CVE-2021-26855 is a server-side request forgery vulnerability in Exchange's Client Access Service (CAS). The Exchange HTTP proxy module allows cookie-based authentication via the `X-BEResource` cookie. The cookie specifies a backend server to proxy to; because the backend authentication uses the Exchange server's own machine account (NT AUTHORITY\SYSTEM), the attacker can craft requests that authenticate as SYSTEM to Exchange backend endpoints. By forging the backend path, an attacker can access the Exchange EWS (Exchange Web Services), OAB (Offline Address Book), and other internal endpoints without credentials. CVE-2021-27065 chains a subsequent arbitrary file write via the OAB virtual directory to write a webshell to disk.

## Impact

CVE-2021-26855 combined with CVE-2021-27065 enables pre-authentication remote code execution resulting in full control of the Exchange server. HAFNIUM exploited these as zero-days to install ASPX webshells (e.g., `web.config` or `aspx` files in Exchange directories), exfiltrate email, and establish persistent backdoors. Hundreds of thousands of Exchange servers were exploited worldwide. Microsoft MSTIC documented HAFNIUM's use of Covenant, PowerCat, and custom malware post-exploitation.

## Mitigation

Apply the Emergency Exchange security update (KB5000871). Run the EOMT (Exchange On-premises Mitigation Tool) provided by Microsoft as immediate mitigation. Scan for webshells using Microsoft's CSS-Exchange scripts. Check for indicators including unusual files in `\inetpub\wwwroot\aspnet_client\`, unusual `w3wp.exe` child processes, and suspicious Exchange log entries with malformed cookies. Restrict Exchange to authenticated access only and apply layered network controls.
