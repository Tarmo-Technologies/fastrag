---
title: "CVE-2017-5638 — Apache Struts OGNL Injection Remote Code Execution"
---

# Apache Struts — OGNL Expression Injection RCE

CVE-2017-5638 is a critical remote code execution vulnerability in Apache Struts 2, disclosed in March 2017. The flaw exists in the Jakarta Multipart parser (`org.apache.struts2.dispatcher.multipart.JakartaMultiPartRequest`) used to handle file uploads. An attacker can inject an OGNL (Object-Graph Navigation Language) expression in the `Content-Type` HTTP header, causing the Struts framework to evaluate and execute arbitrary Java code. CVSS score: 10.0 (Critical). This vulnerability was exploited in the Equifax data breach, exposing 147 million consumer records.

## Technical Details

When the Jakarta multipart parser encounters a malformed `Content-Type` header during file upload processing, it constructs an error message that is passed to the OGNL expression evaluator without sanitization. OGNL can invoke arbitrary Java methods, access the JVM runtime, and execute OS commands via `Runtime.exec()` or `ProcessBuilder`. A minimal exploit sets `Content-Type` to an OGNL payload such as `%{(#_='multipart/form-data').(#dm=@ognl.OgnlContext@DEFAULT_MEMBER_ACCESS).(#_memberAccess?(#_memberAccess=#dm):...).(#cmd='id').(#iswin=(@java.lang.System@getProperty('os.name').toLowerCase().contains('win')))...}`. The attack requires only a single HTTP POST request with no authentication.

## Impact

CVE-2017-5638 provides unauthenticated remote code execution via a single HTTP request against any application using the vulnerable Struts multipart parser. The Equifax breach (July 2017) exploited this vulnerability against an internet-facing application, resulting in exfiltration of Social Security numbers, birth dates, addresses, and credit card data for 147 million US consumers. Metasploit module `exploit/multi/http/struts2_content_type_ognl` automates exploitation.

## Mitigation

Upgrade to Struts 2.3.32 or 2.5.10.1 and later. As an immediate workaround, replace the Jakarta multipart parser with a different implementation (e.g., Pell parser) by changing struts.multipart.parser in struts.xml. Apply WAF rules blocking OGNL expression patterns in Content-Type headers. Implement regular patch cadence for application framework dependencies. Detection: scan access logs for `Content-Type` headers containing `%{` or `#cmd=`.
