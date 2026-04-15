---
title: "CWE-352 — Cross-Site Request Forgery (CSRF)"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-352 — Cross-Site Request Forgery

CWE-352 (Cross-Site Request Forgery) describes attacks where a malicious website, email, or document causes a victim's browser to send a forged request to a target web application where the victim is authenticated. Because the browser automatically includes cookies, the target application cannot distinguish the legitimate request from the forged one without an additional anti-forgery mechanism.

## Technical Details

CSRF exploits the browser's automatic inclusion of credentials (cookies, Basic Auth, client certificates) with cross-origin requests. A classic CSRF attack targets state-changing operations (fund transfer, password change, account deletion) via GET or POST. The attacker hosts a page with `<img src="https://bank.com/transfer?amount=1000&to=attacker">` or a hidden form that auto-submits via JavaScript. Modern CSRF defenses include: **Synchronizer Token Pattern** — a server-generated, unpredictable token embedded in forms and validated server-side. **Double Submit Cookie** — token in both cookie and request body, valid because cross-origin scripts cannot read cookies. **SameSite cookie attribute** — `SameSite=Strict` or `Lax` prevents cookies from being sent with cross-site requests (primary modern mitigation). **Origin/Referer header validation** — checking the `Origin` or `Referer` header, though these can be absent. **Custom request headers** — AJAX requests with `X-Requested-With: XMLHttpRequest` cannot be forged cross-origin via simple forms.

## Impact

CSRF can cause account takeover (password/email change), financial fraud, configuration changes, and any state-changing operation the victim user can perform. Historically, router admin interfaces were common CSRF targets — exploiting CSRF to change DNS settings and route traffic through attacker infrastructure. Combined with XSS (CWE-79), CSRF tokens become irrelevant as the attacker can read them via the injected script.

## Mitigation

Set `SameSite=Strict` (or `Lax`) on all session cookies — this is the most effective modern mitigation and is now the browser default in most implementations. Implement CSRF tokens on all state-changing endpoints. Verify `Origin` and `Referer` headers as defense-in-depth. Require re-authentication for sensitive operations (password change, MFA changes). Use the `sec-fetch-site` and `sec-fetch-mode` headers for server-side CORS policy enforcement.
