---
title: "CVE-2024-6387 — regreSSHion OpenSSH Signal Handler Race Condition RCE"
published_date: 2024-07-01
---

# OpenSSH — regreSSHion Signal Handler Race Condition Remote Code Execution

CVE-2024-6387, named regreSSHion, is a critical remote code execution vulnerability in OpenSSH's server (`sshd`) disclosed by Qualys in July 2024. The vulnerability is a signal handler race condition that was previously fixed in 2006 (CVE-2006-5051) and reintroduced in OpenSSH 8.5p1 (October 2020) via a code change to `SIGALRM` handling. Affected versions: OpenSSH < 4.4p1 (original), and OpenSSH 8.5p1 through 9.7p1 (regression). CVSS score: 8.1 (High, but effectively critical as unauthenticated RCE).

## Technical Details

The race condition occurs in `sshd`'s `SIGALRM` handler when a client fails to complete authentication within the `LoginGraceTime` period (default 120 seconds). The `SIGALRM` signal invokes `cleanup_exit()`, which calls `syslog()` — an async-signal-unsafe function that uses `malloc()` and `free()` internally. A concurrent thread performing memory allocation can race with the signal handler's `malloc`/`free` calls, potentially corrupting the heap. The vulnerability is exploitable via heap manipulation to achieve RCE as root (sshd runs as root before privilege separation completes). Exploitation is probabilistic and timing-dependent; the Qualys proof of concept required approximately 10,000 attempts against the default configuration. The attack can be detected by the large number of failed authentication attempts in logs.

## Impact

CVE-2024-6387 enables unauthenticated remote code execution as root on vulnerable OpenSSH servers with default configuration, given sufficient connection attempts. Qualys estimated 14 million potentially vulnerable sshd instances exposed to the internet at time of disclosure. Exploitation is noisy due to the number of required attempts, making it detectable via log analysis and rate limiting. Exploitation was observed in the wild against Linux/glibc targets; OpenBSD systems were not affected due to its non-vulnerable `syslog()` implementation.

## Mitigation

Upgrade to OpenSSH 9.8p1 or apply distribution patches. As an immediate mitigation, set `LoginGraceTime 0` in `sshd_config` — this disables the grace period timeout but eliminates the vulnerable code path (at the cost of potential denial-of-service via connection exhaustion). Rate-limit inbound SSH connections with `MaxStartups` and network-level controls. Enable `PerSourcePenalties` (available in OpenSSH 9.8p1). Detection: monitor for large numbers of failed authentication attempts from a single IP in the authentication log.
