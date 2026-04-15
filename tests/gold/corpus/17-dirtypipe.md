---
title: "CVE-2022-0847 — Linux Kernel Dirty Pipe Privilege Escalation"
published_date: 2022-03-07
---

# Linux Kernel — Dirty Pipe Pipe Buffer Overwrite

CVE-2022-0847, known as Dirty Pipe, is a local privilege escalation vulnerability in the Linux kernel, discovered by Max Kellermann and disclosed in March 2022. The vulnerability allows an unprivileged local user to overwrite arbitrary read-only files, including SUID binaries and files owned by root, by abusing an uninitialized `PIPE_BUF_FLAG_CAN_MERGE` flag in the pipe buffer implementation. Affected kernels: 5.8 through 5.16.10, 5.15.24, and 5.10.101 (patched). CVSS score: 7.8 (High). The name derives from `Dirty COW` (CVE-2016-5195), which it resembles in impact but differs in mechanism.

## Technical Details

The vulnerability exists in `lib/iov_iter.c`'s `copy_page_to_iter_pipe()` and `push_pipe()` functions, which fail to initialize the `flags` member of new pipe buffer structures. When a pipe splice operation reads data from a file into a pipe, the uninitialized `PIPE_BUF_FLAG_CAN_MERGE` flag may be set from stale memory. When the attacker subsequently writes to the pipe, the kernel merges the write into the existing page cache entry (which backs the file on disk) rather than creating a new buffer, bypassing the read-only protection. This allows overwriting arbitrary bytes in any file readable by the attacker — in practice any world-readable file.

## Impact

Dirty Pipe allows any unprivileged local user to overwrite SUID root binaries (e.g., `/usr/bin/passwd`, `/bin/su`), escalate to root, and gain full system compromise. A simple PoC overwrites the first byte of `/etc/passwd` to modify root's password hash, or replaces a SUID binary entry point. The vulnerability also has implications for container escape when the host kernel is vulnerable: a process with read access to a host file via a bind mount can overwrite it. Android devices running kernel 5.8+ were also affected.

## Mitigation

Upgrade to Linux kernel 5.16.11, 5.15.25, or 5.10.102. For container platforms, verify host kernel version. Detection: monitor for unexpected modifications to SUID binaries (file integrity monitoring on `/usr/bin`, `/bin`, `/sbin`) and audit `splice()` syscall usage. Immutable file attributes (`chattr +i`) can protect critical binaries but are not a complete mitigation.
