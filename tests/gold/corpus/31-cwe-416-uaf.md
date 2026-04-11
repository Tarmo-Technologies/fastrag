---
title: "CWE-416 — Use After Free: Memory Safety Vulnerability"
---

# CWE-416 — Use After Free

CWE-416 (Use After Free) describes a memory safety vulnerability class where a program continues to reference memory after it has been freed (deallocated). An attacker who can control the heap contents after the free can craft heap layouts to overwrite the freed region with attacker-controlled data, diverting program control flow to achieve arbitrary code execution or information disclosure.

## Technical Details

Use-after-free vulnerabilities occur in C/C++ programs when: (1) a pointer to heap-allocated memory is retained after `free()`, (2) the freed memory is reallocated for different data (heap spray or deterministic reuse), and (3) the dangling pointer is subsequently accessed. In browser exploitation, UAF is the dominant exploit primitive. An attacker typically triggers the free via one code path, fills the freed chunk with a crafted object (heap grooming) via another path, then triggers use of the dangling pointer. The reused memory causes type confusion, enabling the attacker to control virtual function table pointers (vtables) for RCE. Modern mitigations including PartitionAlloc (Chrome), BH-Alloc, and typed allocators attempt to prevent cross-type reuse. Garbage-collected languages (Java, Go, Rust, Python) are immune to UAF; the vulnerability class is exclusive to manual memory management languages.

## Exploitation in Modern Targets

Browser engine CVEs frequently involve CWE-416: V8 (CVE-2021-21224), WebKit, and Gecko have all had exploited UAF vulnerabilities. In the kernel context, UAF in network stack or driver code is a common privilege escalation vector (e.g., Linux net/core UAFs). CVE-2023-4863 (libwebp heap overflow) is related — heap overflows often create the UAF conditions that enable exploitation. Exploitation primitives built on UAF include addrof (read object address) and fakeobj (forge typed object at address).

## Mitigation

Prefer memory-safe languages (Rust, Go, Java) for new security-critical code. In C/C++, zero pointers immediately after freeing (`ptr = NULL; free(ptr_copy)`), use smart pointers (`unique_ptr`, `shared_ptr`), and enable AddressSanitizer during testing (`-fsanitize=address`). Enable CFI (Control Flow Integrity) to prevent vtable hijacking. In browsers, enable site isolation and MTE (Memory Tagging Extension) on ARM hardware. Static analysis tools (CodeQL, Semgrep) can detect some UAF patterns.
