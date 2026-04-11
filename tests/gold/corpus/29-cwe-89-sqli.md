---
title: "CWE-89 — SQL Injection: Improper Neutralization of SQL Special Elements"
---

# CWE-89 — SQL Injection

CWE-89 (Improper Neutralization of Special Elements used in an SQL Command — SQL Injection) is the premier database attack class. SQL injection occurs when user-controlled data is interpolated into SQL query strings without parameterization, allowing an attacker to modify query structure and execute arbitrary SQL.

## Technical Details

SQL injection arises from string concatenation of user input into SQL queries. Classic example: `"SELECT * FROM users WHERE name='" + username + "'"` allows injection via `username = "' OR '1'='1"`. Injection variants include: **In-band**: results returned directly (union-based, error-based). **Blind**: no direct output; enumeration via boolean conditions (`AND 1=1` vs `AND 1=2`) or time-based delays (`AND SLEEP(5)`). **Out-of-band**: data exfiltration via DNS or HTTP callbacks using database functions (`xp_dirtree`, `LOAD_FILE`, `UTL_HTTP`). Second-order injection: malicious data stored safely but later used unsanitized in a query. Stored procedures are not immune if they concatenate input internally. ORM frameworks reduce but do not eliminate injection risk — raw query methods (`query()`, `execute()`) still require parameterization.

## Impact

SQL injection enables unauthorized data access (SELECT), data modification (UPDATE/INSERT/DELETE), authentication bypass (`' OR 1=1 --`), stored procedure execution (EXEC xp_cmdshell), file system access (INTO OUTFILE / LOAD_FILE), and in some configurations OS command execution. Database servers running as high-privilege accounts amplify impact. SQLMap automates detection and exploitation of SQL injection. CVE-2023-34362 (MOVEit) is a recent high-profile SQL injection leading to mass data exfiltration.

## Mitigation

Use parameterized queries (prepared statements) for all database interactions — this is the single most effective control. ORMs using named parameters (`:param`, `?`) are safe when used correctly. Apply the principle of least privilege to database accounts. Implement WAF rules for SQLi pattern detection as defense-in-depth. Enable database auditing. Input validation is not a substitute for parameterized queries — filter bypasses are well-documented for every denylist approach.
