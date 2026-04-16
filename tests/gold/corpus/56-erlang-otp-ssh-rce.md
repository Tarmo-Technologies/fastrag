---
title: "CVE-2025-32433 — Erlang/OTP SSH Pre-Authentication Remote Code Execution"
published_date: 2025-04-16
---

# Erlang/OTP — SSH Pre-Authentication Remote Code Execution (CVE-2025-32433)

CVE-2025-32433 is a critical pre-authentication remote code execution vulnerability in the Erlang/OTP SSH server implementation, disclosed on April 16, 2025. The flaw arises from improper enforcement of authentication state in SSH connection protocol message handling: the SSH daemon processes connection protocol messages (channel open requests) before authentication is complete. CVSS score: 10.0 (Critical). Affected versions: Erlang/OTP prior to OTP-27.3.3, OTP-26.2.5.11, and OTP-25.3.2.20. Public proof-of-concept code was released shortly after disclosure, and CISA added the vulnerability to the Known Exploited Vulnerabilities (KEV) catalog on June 9, 2025.

## Technical Details

In the SSH protocol, a client must complete the authentication handshake before it can open channels or execute commands. CVE-2025-32433 exploits a flaw in Erlang/OTP's SSH server where channel open requests (SSH connection protocol messages with codes >= 80) are processed without verifying that authentication has succeeded. An unauthenticated remote attacker can send an SSH_MSG_CHANNEL_OPEN message immediately after the key exchange phase, before presenting credentials. The SSH daemon opens the channel and allows command execution, running with the privileges of the SSH service process — typically root in many Erlang/OTP deployments (e.g., Ejabberd, RabbitMQ, CouchDB, and telecom switching infrastructure). The GitHub Security Advisory GHSA-37cp-fgq5-7wc2 contains the canonical disclosure. Multiple public exploit modules were published within days of disclosure.

## Impact

CVE-2025-32433 grants full unauthenticated root access on any system running a vulnerable Erlang/OTP SSH server. Because Erlang/OTP underpins distributed systems, telecommunications infrastructure, message brokers (RabbitMQ), databases (CouchDB, Riak), and XMPP servers (Ejabberd), the exposure surface spans IoT, industrial control systems, financial services, and cloud infrastructure. Censys identified a significant number of OTP SSH servers exposed to the internet. Active exploitation was observed targeting OT networks, education, healthcare, and high-technology sectors. Attackers were observed attempting to access services over industrial and IT ports following successful exploitation.

## Mitigation

Upgrade Erlang/OTP to OTP-27.3.3, OTP-26.2.5.11, or OTP-25.3.2.20 respectively. If patching is not immediately possible, disable the Erlang/OTP SSH daemon if unused, or restrict SSH access to trusted networks using firewall rules. Applications using Erlang's built-in SSH server (`:ssh.start/0` or `ssh:daemon/2`) are directly affected; applications using system OpenSSH (not OTP SSH) are not affected. Audit RabbitMQ, CouchDB, Ejabberd, and custom Erlang application deployments for SSH daemon exposure. Monitor for anomalous SSH connections preceding authenticated sessions in Erlang application logs.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-32433
- https://github.com/erlang/otp/security/advisories/GHSA-37cp-fgq5-7wc2
