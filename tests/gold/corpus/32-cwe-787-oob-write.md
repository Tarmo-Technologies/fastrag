---
title: "CWE-787 — Out-of-Bounds Write: Buffer Overflow Exploitation"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-787 — Out-of-Bounds Write

CWE-787 (Out-of-Bounds Write) is the most prevalent memory safety vulnerability class in the CWE Top 25. It occurs when software writes data past the end (or before the beginning) of an intended buffer. Depending on what is overwritten — heap metadata, return addresses, function pointers, or adjacent data — the impact ranges from denial of service (crash) to arbitrary code execution.

## Technical Details

Out-of-bounds writes arise from missing bounds checks, incorrect length calculations, integer overflows in size computation, or off-by-one errors in loops. Stack-based overflows (classic buffer overflows) overwrite the saved return address or saved frame pointer on the call stack, redirecting execution. Heap-based overflows overwrite heap metadata (glibc `malloc` header fields), adjacent object fields, or function pointers stored on the heap. Modern exploitation techniques for CWE-787 include: heap feng shui (grooming allocation layout), tcache poisoning in glibc, fastbin corruption, and GOT (Global Offset Table) overwrite. Mitigations deployed against CWE-787 exploitation include: stack canaries (detects stack overwrites before return), ASLR (randomizes load addresses), NX/DEP (prevents code execution in data regions), safe stack, shadow stack (CET), and CFI.

## Relationship to CVEs

CWE-787 underlies many high-profile CVEs. CVE-2023-4863 (libwebp) is an out-of-bounds write via Huffman table overflow. CVE-2022-0847 (Dirty Pipe) exploits an uninitialized flag that effectively allows out-of-bounds page cache writes. CVE-2021-44228 (Log4Shell) involves JNDI deserialization that often chains through heap corruption. Stack-based examples include CVE-2021-3156 (sudo heap overflow) and numerous Windows kernel pool overflows.

## Mitigation

Use memory-safe languages for new code. In C/C++, always perform bounds checks before array writes. Use safe string functions (`strlcpy`, `strlcat`) instead of `strcpy`/`strcat`. Enable compiler mitigations: `-fstack-protector-strong`, `-D_FORTIFY_SOURCE=2`, PIE, and full RELRO. Enable AddressSanitizer and fuzzing in CI to detect out-of-bounds writes during development. For binary protection: enable ASLR system-wide, use CFI, and enable Intel CET/Shadow Stack where available.
