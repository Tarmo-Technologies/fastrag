---
title: "CVE-2021-22205 — GitLab ExifTool Remote Code Execution via Image Upload"
---

# GitLab — ExifTool Remote Code Execution via Image Upload

CVE-2021-22205 is a critical pre-authentication remote code execution vulnerability in GitLab CE/EE, patched in April 2021 but widely unpatched until mass exploitation began in October 2021. The vulnerability exists in GitLab's use of ExifTool to parse image metadata. An attacker can upload a crafted DjVu file containing a malicious ExifTool payload, causing GitLab to execute arbitrary OS commands as the `git` user. CVSS score: 10.0 (Critical). Affected versions: GitLab CE/EE 11.9 through 13.8.8, 13.9 through 13.9.6, 13.10 through 13.10.3.

## Technical Details

GitLab uses ExifTool to process uploaded images and strip metadata for privacy. ExifTool up to version 12.23 contains a code injection vulnerability (CVE-2021-22204) in its DjVu file parser. DjVu `ANTz` annotations support an `(include ...)` directive that ExifTool processes via Perl `eval()` after decompressing with `bzdecompress`. An attacker crafts a DjVu file with a Perl payload inside the `ANTz` chunk; when GitLab processes the upload (even unauthenticated via the import endpoint or certain API paths), ExifTool evaluates the Perl code. Public exploits use the pattern `(metadata (Copyright "\\" . qx{cmd} . ")"))` in the annotation block.

## Impact

CVE-2021-22205 allows unauthenticated remote code execution on the GitLab server. Attackers gained shells as the `git` user, enabling repository access, credential theft from `.gitconfig` and environment variables, and in many cases privilege escalation to root via misconfigured sudoers. Mass exploitation in October–November 2021 resulted in cryptocurrency mining deployments and backdoor installation. Unpatched GitLab instances exposed to the internet (hundreds of thousands at the time) were compromised en masse.

## Mitigation

Upgrade GitLab to 13.10.3, 13.9.6, or 13.8.8 and later, or upgrade ExifTool to 12.24+. For immediate workaround, disable image processing or block DjVu file uploads. Detection: review GitLab logs for unusual image upload requests and unexpected child processes spawned by `gitlab-workhorse`. Scan for suspicious cron jobs and SSH keys added to the `git` user's authorized_keys.
