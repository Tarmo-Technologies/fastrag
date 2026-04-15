---
title: "CWE-476 — NULL Pointer Dereference: Null Reference Crashes and Exploits"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-476 — NULL Pointer Dereference

CWE-476 (NULL Pointer Dereference) describes vulnerabilities where a program dereferences a pointer that it expects to be valid but which is NULL. In most contexts this causes a crash (denial of service), but in kernel and embedded contexts a NULL deref can be exploitable for privilege escalation due to the ability to map the zero page.

## Technical Details

NULL dereferences arise when: return values from memory allocation functions (`malloc()`, `calloc()`, `new`) or API calls that return NULL on failure are used without checking for NULL. In C, common patterns include unchecked `strdup()`, `fopen()`, library functions that return NULL for invalid inputs, and out-of-memory conditions. Kernel NULL pointer dereferences historically allowed privilege escalation by mapping page zero (`mmap(0, ...)` with `MAP_FIXED`) and placing shellcode there; the kernel dereference then executes attacker code. Modern mitigations disable zero-page mapping (`vm.mmap_min_addr` sysctl on Linux, kernel SMEP/SMAP).

## Denial of Service and Reliability Impact

In user-space applications, NULL dereferences manifest as segmentation faults and crashes. In server applications, a crash may be remotely triggerable, causing denial of service. CVSS scoring for NULL deref vulnerabilities typically reflects the DoS impact, with higher scores for kernel or safety-critical system contexts.

## Relationship to Other Weakness Classes

NULL pointer dereferences can be enabled by use-after-free (CWE-416) conditions, failed allocation checks, and type confusion errors. In Rust, the borrow checker and `Option<T>` type system eliminate null pointer dereferences by construction — this is one of Rust's primary memory safety guarantees. In Java and other garbage-collected languages, NullPointerException is the equivalent runtime error, though not exploitable for code execution.

## Mitigation

Always check return values for NULL before dereferencing. Use defensive programming patterns: allocate and check before use. Enable compiler warnings for potential null dereferences (Clang's `-Weverything`, GCC's `-Wnull-dereference`). Use static analysis tools (Coverity, CodeQL null dereference rules). In Rust, use `Option<T>` and `.unwrap_or()` / `?` operator patterns to handle null cases explicitly. Set `vm.mmap_min_addr` to at least 65536 on Linux systems to prevent zero-page mapping exploitation.
