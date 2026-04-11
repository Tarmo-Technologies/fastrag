---
title: "CWE-190 — Integer Overflow: Arithmetic Wrap-Around Vulnerabilities"
---

# CWE-190 — Integer Overflow or Wraparound

CWE-190 (Integer Overflow or Wraparound) describes vulnerabilities where arithmetic operations on integer values produce results that exceed the maximum value representable by the integer type, causing the value to wrap around (overflow) or be truncated. When the overflowed value is subsequently used in memory allocation size calculations or array indexing, the result is typically a heap or stack buffer overflow exploitable for code execution.

## Technical Details

Integer overflow patterns include: **Signed overflow** — undefined behavior in C/C++; signed overflow in malloc size calculation produces a small allocation from a large product (e.g., `width * height` overflows 32-bit signed int). **Unsigned wraparound** — defined behavior in C/C++ but logically incorrect when used as a size; `(uint32_t)(-1)` wraps to 0xFFFFFFFF. **Sign extension** — promoting a negative signed int to unsigned size_t produces a large value. **Truncation** — assigning a 64-bit value to a 32-bit variable silently discards high bits, enabling attacker control of the lower bits.

The classic exploitation chain: an attacker controls `count` (number of elements) and element `size`. The multiplication `count * size` overflows to a small value; `malloc(overflow_value)` allocates a small buffer; a subsequent loop copies `count * size` actual bytes into the small buffer, causing heap overflow. CVE-2004-0230 (TCP RST attacks) and numerous image parser CVEs follow this pattern.

## Relationship to CVEs

CVE-2023-4863 (libwebp) involves a Huffman table size overflow. Many SSL/TLS library CVEs involve integer overflows in length field parsing. The PNG parser CVE-2004-0597 used a classic integer overflow in chunk size multiplication. CVE-2021-30860 (CoreGraphics / FORCEDENTRY) used an integer overflow in PDF JBIG2 parsing for NSO Group's zero-click iMessage exploit.

## Mitigation

Use integer overflow-safe arithmetic: `__builtin_mul_overflow()` in GCC/Clang, `checked_add`/`checked_mul` in Rust, or SafeInt in C++. Validate that computed sizes are reasonable before using in allocation. Enable UBSan (Undefined Behavior Sanitizer, `-fsanitize=signed-integer-overflow`) in testing. Use 64-bit size types for allocation calculations. Language-level bounds: Rust panics on overflow in debug builds; Python has arbitrary precision integers.
