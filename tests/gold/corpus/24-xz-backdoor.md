---
title: "CVE-2024-3094 — xz-utils Supply Chain Backdoor in liblzma"
published_date: 2024-03-29
---

# xz-utils — Supply Chain Backdoor in SSH Authentication (liblzma)

CVE-2024-3094 is a critical supply chain backdoor in xz-utils versions 5.6.0 and 5.6.1, discovered by Microsoft engineer Andres Freund in March 2024. A malicious maintainer (operating as "Jia Tan" / JiaT75) who had been building trust in the xz-utils project over approximately two years inserted a sophisticated backdoor into the xz build system. The backdoor specifically targets `sshd` on systemd-based Linux systems, hooking RSA public key authentication to allow backdoor operators to authenticate using an undisclosed private key. CVSS score: 10.0 (Critical).

## Technical Details

The backdoor was injected via the xz release tarball's build system (`m4/build-to-host.m4`). The malicious build scripts extract hidden payloads from test binary files (`tests/files/bad-3-corrupt_lzma2.xz` and `good-large_compressed.lzma`) committed to the repository. The extracted payload is a shared library that patches `sshd` at runtime via GNU `ifunc` and `IFUNC` resolver hooks. On affected systems where `sshd` links against `libsystemd`, which links against `liblzma`, the backdoor is loaded into the `sshd` address space. The backdoor intercepts RSA public key authentication in OpenSSH, allowing the attacker to authenticate with a hardcoded Ed448 private key. The backdoor also suppresses authentication logging for the backdoor operator. Discovery was aided by Freund noticing unusual CPU usage in `sshd` on Debian Sid.

## Impact

CVE-2024-3094 provides pre-authentication remote access to `sshd` on affected systems — effectively a silent root backdoor in SSH on Debian Sid, Fedora Rawhide, and other rolling-release distributions that had adopted xz 5.6.x. Though caught before reaching stable distributions, the sophistication of the attack (two-year trust-building, multi-stage payload hiding, IFUNC hooking) represents the most technically advanced supply chain attack publicly disclosed to date.

## Mitigation

Downgrade xz-utils to 5.4.x. Fedora Rawhide and Debian Sid users who updated in March 2024 should audit their systems. Check the xz version: `xz --version`. Verify sshd binary integrity against distribution-provided hashes. CISA issued advisory AA24-099A. Examine xz changelogs and commit history for the malicious commits (xz GitHub repository was taken offline temporarily). The incident motivated audit work for `build-to-host.m4` patterns across the open source ecosystem.
