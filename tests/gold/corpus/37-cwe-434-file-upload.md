---
title: "CWE-434 — Unrestricted File Upload: Dangerous File Type Execution"
---

# CWE-434 — Unrestricted Upload of File with Dangerous Type

CWE-434 describes vulnerabilities where a web application allows users to upload files without adequately validating type, content, or filename, enabling attackers to upload executable code (webshells, scripts) that is subsequently executed by the server. File upload vulnerabilities are a direct path to remote code execution in web applications.

## Technical Details

Unrestricted upload vulnerabilities exploit gaps in file type validation. **Extension filtering bypass**: application blocks `.php` but allows `.php5`, `.phtml`, `.pHp`, `.php.jpg` (Apache MultiViews), or double extensions. **Content-type spoofing**: application trusts the `Content-Type` header (attacker-controlled) rather than inspecting file magic bytes. **Image polyglots**: files that are simultaneously valid JPEG images (magic bytes: `FF D8 FF`) and valid PHP/JSP scripts — the image is executed as PHP when `exif_imagetype()` returns true. **Null byte injection**: `shell.php%00.jpg` terminates the filename at the null byte in C-based parsers. **Archive extraction**: Zip/tar archives extracted server-side may contain path traversal entries (Zip Slip) that place webshells outside the intended upload directory. CVE-2021-22205 (GitLab ExifTool RCE) is a file upload processing vulnerability exploiting DjVu content.

## Exploitation

A webshell uploaded to a public web directory provides interactive command execution in the browser: `<?php system($_GET["cmd"]); ?>` accessed as `https://target.com/uploads/shell.php?cmd=id`. Webshells vary from simple one-liners to full-featured backdoors like China Chopper, B374k, and WSO (Web Shell by Orb).

## Mitigation

Store uploaded files outside the web root, preventing direct HTTP access. Rename all uploaded files to random names with validated extensions, never trust user-supplied filenames. Validate file type by inspecting magic bytes with a trusted library. Serve user-uploaded files through a download handler that sets `Content-Disposition: attachment`. Disable script execution in upload directories (Apache `Options -ExecCGI`, nginx `location /uploads { deny all; }`). Run file processing (e.g., image conversion) in a sandboxed environment.
