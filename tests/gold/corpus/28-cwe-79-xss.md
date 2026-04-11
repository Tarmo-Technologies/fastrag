---
title: "CWE-79 — Cross-Site Scripting (XSS): Improper Neutralization of Input"
---

# CWE-79 — Cross-Site Scripting (XSS)

CWE-79 (Improper Neutralization of Special Elements in Web Page Output — Cross-Site Scripting) describes vulnerabilities where an application includes unvalidated, unescaped user-supplied data in HTML output, allowing an attacker to inject client-side scripts that execute in the victim's browser context. XSS is among the most prevalent web vulnerabilities, appearing in OWASP Top 10 consistently.

## Technical Details

XSS manifests in three primary variants. **Reflected XSS**: attacker-controlled input in the current HTTP request is echoed in the response without encoding. Exploitation requires tricking a victim into making a crafted request (e.g., via a malicious link). **Stored XSS**: malicious script is persisted in the application database (comments, profile fields) and rendered to other users. **DOM-based XSS**: client-side JavaScript reads from attacker-controllable sources (e.g., `location.hash`, `document.referrer`) and writes to dangerous sinks (`innerHTML`, `eval()`, `document.write()`). Encoding requirements depend on context: HTML context requires entity encoding, JavaScript string context requires JavaScript string escaping, URL context requires percent-encoding, CSS context requires CSS escaping. Context-insensitive escaping (e.g., only replacing `<` and `>`) is insufficient.

## Impact

XSS enables session hijacking via `document.cookie` theft, credential phishing by modifying login forms, keylogging, cross-site request forgery bypass, clickjacking, and browser exploitation. Stored XSS in administrative interfaces can lead to account takeover of privileged users. Modern browsers' Same-Origin Policy does not protect against XSS because the injected script runs in the legitimate origin's context. Content Security Policy (CSP) can limit XSS impact but is frequently misconfigured.

## Mitigation

Apply context-appropriate output encoding using a trusted library (e.g., OWASP Java Encoder, DOMPurify for client-side). Implement a strict Content Security Policy prohibiting inline scripts and restricting script sources. Use modern frameworks that auto-escape template output (React JSX, Angular templates). Mark session cookies `HttpOnly` and `Secure` to prevent cookie theft. Validate and sanitize input on ingress as defense-in-depth, but do not rely on input validation alone.
