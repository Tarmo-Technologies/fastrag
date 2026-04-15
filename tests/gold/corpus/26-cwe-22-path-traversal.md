---
title: "CWE-22 — Path Traversal: Improper Limitation of a Pathname"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-22 — Path Traversal

CWE-22 (Improper Limitation of a Pathname to a Restricted Directory) describes a class of vulnerabilities where an application uses external input to construct a file path without adequately neutralizing special elements such as `../` sequences. This allows an attacker to access files or directories outside the intended restricted directory, potentially reading sensitive system files, source code, or configuration containing credentials.

## Technical Details

Path traversal vulnerabilities arise when file paths derived from user input are used in file system operations without canonical normalization. Classic attack patterns include URL-encoded traversal sequences (`%2e%2e%2f`, `..%2f`, `%2e%2e/`), double-encoded sequences (`%252e%252e%252f`), and Unicode normalization attacks. On Windows, additional variants include `..\\`, `..%5C`, and drive-letter prefix injection. Secure path handling requires: (1) resolving the input path to its canonical form using `realpath()` or equivalent, (2) verifying the canonical path begins with the intended base directory prefix, and (3) rejecting inputs that escape the prefix. File systems on some platforms allow null-byte injection (`file.txt\x00.jpg`) to truncate path extensions.

## Common Exploitation Scenarios

Path traversal is frequently exploited to read `/etc/passwd`, `/etc/shadow`, application configuration files (database credentials, API keys), and SSH private keys. In web applications, traversal via file download/view endpoints is the most common pattern: `GET /download?file=../../../etc/passwd`. Directory traversal in archive extraction (Zip Slip: CVE-2018-1002207) allows writing files outside the extraction target, enabling code execution. Many CVEs in web application frameworks, archive libraries, and file management tools are rooted in CWE-22.

## Mitigation

Always canonicalize paths before use. Use framework-provided safe path APIs. Implement an allowlist of permitted file names or directories rather than a denylist of `../` sequences (denylists are easily bypassed). Run application processes with minimal filesystem permissions (principle of least privilege). On Java, use `Path.normalize()` followed by `startsWith()` check against the base directory. For archive extraction, validate extracted file paths before writing.
