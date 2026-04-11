# CVE-2024-1234: RCE in libfoo

## Summary

This advisory describes a critical vulnerability affecting all versions
of libfoo prior to 2.3.1.

## Details

The vulnerability allows remote code execution via crafted input to the
`parse_header` function. An attacker supplying a malformed header can
trigger arbitrary code execution in the context of the process using
libfoo.

## Mitigation

Upgrade to libfoo 2.3.1 or later. Sites that cannot upgrade immediately
should disable untrusted input to header parsing.
