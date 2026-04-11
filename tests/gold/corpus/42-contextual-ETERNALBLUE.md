---
title: "CVE-2017-0144 — EternalBlue SMBv1 (Contextual)"
---

# The SMB Protocol Integer Overflow — Contextual Sample

This document intentionally omits the CVE identifier and product name from the body to test contextual retrieval. The title carries the identifying context.

The exploit was originally developed by an intelligence agency and leaked publicly in April 2017 by a group calling itself the Shadow Brokers. Within weeks it was incorporated into two of the most destructive worm campaigns ever recorded, affecting organizations in 150 countries.

## How the Vulnerability Works

The affected protocol implementation contains an integer overflow in the transaction request handler. Specifically, the size calculation for a buffer allocation uses attacker-controlled field values without sufficient validation. The overflow produces a smaller-than-expected allocation; subsequent processing writes attacker-controlled data beyond the end of the buffer into adjacent kernel pool memory. This allows an attacker to overwrite control structures in the Windows kernel heap and redirect execution to shellcode.

## Attack Prerequisites

The attacker needs network access to port 445 on the target system and the target must have the first version of the affected protocol enabled. No authentication is required. The entire exploit sequence is a single TCP connection.

## Global Impact

When the vulnerability was weaponized in a ransomware worm, it propagated automatically across networks by scanning for the open port, exploiting the flaw, and deploying its payload — all without user interaction. The damage was estimated in the billions of dollars. A second destructive campaign months later combined the same exploit with credential harvesting to spread even more aggressively.

## Remediation

Apply the vendor security update that addresses this class of flaws. Disable the vulnerable protocol version across the environment. Block the associated network port at segment boundaries. Segment workstations from each other to limit lateral movement even if a single machine is compromised.
