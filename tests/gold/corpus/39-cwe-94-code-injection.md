---
title: "CWE-94 — Code Injection: Improper Control of Code Generation"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-94 — Code Injection

CWE-94 (Improper Control of Generation of Code) describes vulnerabilities where an application dynamically generates and executes code incorporating user-supplied input without adequate sanitization. Unlike OS command injection (CWE-78), code injection targets the application's own runtime (Python `eval()`, PHP `eval()`, Ruby `eval()`, JavaScript `eval()`, Perl `eval()`) or template engines, enabling arbitrary code execution within the application's process and privilege context.

## Technical Details

Code injection surfaces include: **Language eval functions** — `eval(user_input)` in Python, PHP, Ruby, JavaScript. **Template injection (SSTI)** — Server-Side Template Injection in Jinja2, Twig, FreeMarker, Velocity when user input is rendered as template syntax (e.g., `{{7*7}}` returning 49 confirms Jinja2 SSTI). **OGNL injection** — CVE-2017-5638 (Apache Struts) and CVE-2022-26134 (Confluence) are OGNL code injection. **Expression Language injection** — Spring EL, JSP EL, MVEL expression injection. **Deserialization** — object deserialization that triggers gadget chain code execution (CWE-502). Jinja2 SSTI payloads like `{{''.__class__.__mro__[1].__subclasses__()[xxx].__init__.__globals__['__builtins__']['__import__']('os').system('id')}}` demonstrate the power of template engine RCE.

## Impact

Code injection provides arbitrary code execution within the application's runtime context. Exploitation typically achieves reading sensitive files, executing OS commands, accessing the database, and establishing persistent backdoors. SSTI is particularly dangerous in cloud environments as it often provides access to environment variables containing cloud provider credentials.

## Mitigation

Never pass user input to `eval()` or template rendering without explicit sanitization. Use logic-free templating (Mustache, Handlebars in safe mode) where template expressions cannot invoke arbitrary functions. Implement sandboxed template evaluation environments. WAF rules for SSTI patterns (`{{`, `{%`, `${{`). Apply input validation to reject template metacharacters. Use AST-based analysis to detect dangerous eval usage in code reviews.
