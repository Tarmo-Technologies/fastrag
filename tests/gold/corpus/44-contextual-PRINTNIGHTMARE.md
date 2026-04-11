---
title: "CVE-2021-34527 — PrintNightmare Windows Print Spooler (Contextual)"
---

# The Print Service Privilege Escalation — Contextual Sample

This document intentionally omits the CVE identifier and product name from the body to exercise contextual retrieval. The title carries the identifying context.

The vulnerability was accidentally disclosed to the public when a proof-of-concept was published to GitHub prematurely, weeks before the official patch was released. The service affected runs by default on all Windows installations, including domain controllers, making exploitation extremely widespread.

## The Exploitation Mechanism

The affected service exposes a remote procedure call interface that includes functions for installing printer drivers. The vulnerability exists because the privilege check for one of these functions is insufficient — any authenticated domain user can invoke it, not just administrators. The function accepts a path to a driver DLL that can be a UNC path pointing to an attacker-controlled network share. The service loads this DLL with the highest available privilege level, executing the attacker's code as SYSTEM.

## Attack Scenarios

For the remote code execution variant, an attacker needs any valid domain credentials and network access to the RPC endpoint. This makes it a powerful lateral movement tool after initial compromise — a single low-privilege account can be used to gain SYSTEM on any workstation with the service running and accessible. For domain controllers, this represents full domain compromise. The local privilege escalation variant requires only a local user account.

## Remediation

The most reliable immediate mitigation is to stop and disable the affected service on systems that do not require printing. Domain controllers in particular almost never need printing capability. Apply the vendor patch when available. If printing cannot be disabled, restrict driver installation to administrators only via group policy. Monitor for the service loading DLLs from network paths.
