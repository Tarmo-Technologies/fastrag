---
title: "CWE-295 — Improper Certificate Validation: TLS Trust Failures"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-295 — Improper Certificate Validation

CWE-295 (Improper Certificate Validation) describes failures to properly validate TLS/SSL certificates, enabling man-in-the-middle (MitM) attacks. Improper validation is endemic in mobile applications, embedded devices, and custom TLS implementations. The vulnerability class enables passive eavesdropping and active traffic manipulation on supposedly encrypted connections.

## Technical Details

Certificate validation failures take several forms: **Disabled validation** — setting `verify=False` (Python requests), `setSSLSocketFactory(new SSLSocketFactory(new TrustStrategy() { return true; }))` (Java), or using `curl -k`. **Hostname validation bypass** — accepting any valid certificate regardless of hostname match; OpenSSL `SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE)`. **Custom TrustManager** — Android apps implementing `X509TrustManager.checkServerTrusted()` as a no-op. **Certificate pinning bypass** — legitimate pinning poorly implemented (trusting any pinned certificate in the chain rather than the leaf). **Self-signed certificate trust** — adding arbitrary CA certificates to the trust store. CVE-2020-0601 (CurveBall) is a certificate validation bypass at the OS level.

## Testing and Detection

Tools for detecting CWE-295 include: BURP Suite's SSL proxy (can MitM if device trusts Burp's CA), `mitmproxy`, `SSLStrip`, and Frida-based runtime hooks that bypass pinning. For Android: TrustMe, Frida scripts targeting `checkServerTrusted`, and APK static analysis with MobSF. Certificate transparency logs can detect issuance of fraudulent certificates for monitored domains.

## Mitigation

Always enable full certificate chain validation including hostname verification. Use library defaults rather than custom TrustManagers. Implement certificate pinning for high-value applications (pin the public key hash, not the full certificate). Use certificate transparency monitoring. For embedded devices, manage a minimal CA trust store rather than trusting the full commercial CA ecosystem. Static analysis tools (Semgrep rules, Android Lint) can detect disabled validation patterns.
