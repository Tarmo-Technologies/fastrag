---
title: "CVE-2022-22965 — Spring Framework Spring4Shell Remote Code Execution"
---

# Spring Framework — Spring4Shell ClassLoader Data Binding RCE

CVE-2022-22965, known as Spring4Shell, is a critical remote code execution vulnerability in the Spring Framework, disclosed in March 2022. The vulnerability affects Spring MVC and Spring WebFlux applications running on JDK 9+ that use `@RequestMapping` with data binding via `@ModelAttribute` or similar. By manipulating class loader properties through crafted HTTP parameters, an attacker can write a malicious JSP webshell to disk via the Tomcat access log mechanism. CVSS score: 9.8 (Critical). Affected: Spring Framework 5.3.x before 5.3.18 and 5.2.x before 5.2.20.

## Technical Details

The exploit abuses Spring's data binding mechanism, which uses `BeanWrapper` to set object properties from HTTP request parameters. On JDK 9+, the `Class.getModule()` method is accessible, allowing property traversal to reach the Tomcat `ClassLoader` and its `Resources` object. The exploit chain: set `class.module.classLoader.resources.context.parent.pipeline.first.pattern` to a JSP payload (e.g., a webshell), set `suffix`, `directory`, `prefix`, and `fileDateFormat` to control the output file path, then trigger a log rotation to flush the JSP to disk. The PoC requires the application to be deployed as a WAR on Tomcat (not Spring Boot executable JAR).

## Impact

CVE-2022-22965 enables unauthenticated remote code execution resulting in webshell deployment on Tomcat-hosted Spring MVC applications. Exploitation requires JDK 9+, Tomcat deployment, and Spring Framework 5.3.x/5.2.x with data binding enabled. Automated scanners and Metasploit modules were available within days of disclosure. Real-world exploitation was observed against enterprise applications.

## Mitigation

Upgrade to Spring Framework 5.3.18 or 5.2.20. For Tomcat-specific deployments, prevent access to the ClassLoader via data binding by disallowing patterns matching `class.*`, `Class.*`, `%.class.*`, and `*.class.*`. WAF rules blocking `class.module.classLoader` in HTTP parameters provide immediate protection. Verify JDK version — JDK 8 environments are not exploitable via the Tomcat log-write chain.
