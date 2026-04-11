---
title: "CWE-125 — Out-of-Bounds Read: Memory Disclosure and Crash Primitives"
---

# CWE-125 — Out-of-Bounds Read

CWE-125 (Out-of-Bounds Read) describes vulnerabilities where software reads data beyond the allocated memory buffer boundary. While less immediately exploitable than out-of-bounds writes (CWE-787), OOB reads enable information disclosure — leaking memory contents that may include pointer addresses (defeating ASLR), secret keys, tokens, and sensitive data from adjacent memory regions.

## Technical Details

Out-of-bounds reads arise from the same root causes as writes: missing bounds checks, integer overflows in length calculations, and off-by-one errors. Classic examples: reading a `char[]` buffer using a length that is greater than the allocation (`memcpy` destination from undersized source), using an attacker-controlled index into an array without range checking, and protocol parsers that trust length fields in packet headers.

CVE-2014-0160 (Heartbleed) is the paradigmatic OOB read: the declared length field determines how many bytes are copied into the heartbeat response, reading far beyond the actual payload into adjacent heap contents. The vulnerability returned 64 KB of server memory per request with no upper limit on repetition, enabling progressive heap disclosure.

## Information Disclosure Uses in Exploit Chains

OOB reads are frequently the first step in a multi-stage exploit: a read primitive leaks a pointer or return address, breaking ASLR; subsequent writes are then aimed precisely at the known address. In browser exploitation, a controlled OOB read in the JavaScript engine can leak the base address of loaded modules. In kernel exploitation, an OOB read in a kernel structure can disclose the kASLR slide.

## Mitigation

Apply the same controls as for CWE-787: bounds checking, safe APIs, AddressSanitizer during development, and compiler fortification flags. Enable ASLR system-wide (randomizes what is disclosed). Hardware memory tagging (ARM MTE) detects out-of-bounds accesses at runtime. Fuzzing with ASan coverage is particularly effective at finding OOB read conditions in parsers and protocol implementations.
