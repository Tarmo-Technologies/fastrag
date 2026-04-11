---
title: "CWE-502 — Deserialization of Untrusted Data"
---

# CWE-502: Deserialization of Untrusted Data

CWE-502 describes the weakness of deserializing untrusted data without
validating that the incoming object is safe. Exploitation typically
uses gadget chains — sequences of classes whose constructors or
setters produce side effects when instantiated.

## Relationship to Gadget Chains

A gadget chain is a sequence of serializable types that, when
deserialized in order, trigger arbitrary code execution. Java, .NET,
and Python pickle are historically affected.

## Mitigation

Avoid deserializing untrusted data entirely. If deserialization is
unavoidable, restrict the allowed type set via an allowlist.
