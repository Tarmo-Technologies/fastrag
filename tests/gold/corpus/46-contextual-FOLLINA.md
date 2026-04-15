---
title: "CVE-2022-30190 — Follina MSDT RCE (Contextual)"
published_date: 2022-06-01
---

# The Office Document Zero-Click Code Execution — Contextual Sample

This document intentionally omits the CVE identifier and product names from the body to exercise contextual retrieval. The title carries the identifying context.

The vulnerability was reported to the vendor as a zero-day in May 2022 and exploitation was observed in the wild before the patch was released. Nation-state threat actors were observed using it in targeted spear-phishing campaigns against European government and military targets.

## How the Attack Works

The attack chain begins with an office document that references an external HTML file via the document's relationship metadata. When the document is opened — or in some configurations merely previewed — the office application fetches the HTML file automatically. The HTML file contains a specially crafted URI that invokes a Windows diagnostic tool. The diagnostic tool's URI handler accepts parameters that are evaluated without adequate sanitization, causing embedded commands to execute.

## Why the Attack Bypasses Common Defenses

Macro execution controls, which many organizations rely on as the primary defense against document-based attacks, offer no protection because macros are not involved. The execution chain flows through the URI handler, bypassing the application sandbox for protocol handling. The victim's only required interaction is to open the document; in some email clients, the preview pane triggers the attack without any click.

## Remediation

Apply the vendor patch that addresses the URI handler. As an immediate workaround, remove the URI protocol handler registration from the Windows registry. Configure application control policies to prevent the diagnostic tool from being launched by office application processes. Monitor process tree events for unexpected diagnostic tool invocations with office application parents.
