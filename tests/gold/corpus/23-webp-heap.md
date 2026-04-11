---
title: "CVE-2023-4863 — libwebp Heap Buffer Overflow in WebP Codec"
---

# libwebp — Heap Buffer Overflow in WebP Huffman Decoding

CVE-2023-4863 is a critical heap buffer overflow vulnerability in the `libwebp` library's WebP image decoding, disclosed in September 2023. Initially disclosed as a Chrome vulnerability, the flaw resides in the shared `libwebp` library used by Google Chrome, Mozilla Firefox, Microsoft Edge, Safari, Electron applications, and countless other software products. The vulnerability is in the Huffman table building code within the WebP lossless decoder. CVSS score: 8.8 (High, though vendor scored 10.0). Actively exploited as a zero-day by commercial spyware vendors (likely NSO Group or similar).

## Technical Details

The vulnerability exists in `libwebp/src/dec/vp8l_dec.c` in the `ReadHuffmanCodeLengths()` function. A maliciously crafted WebP lossless image can trigger an out-of-bounds write by providing a Huffman code length table that exceeds the allocated buffer size. Specifically, the `ProcessRows()` function can be manipulated to write beyond the bounds of the `pixels` buffer via the `color_cache` lookup during Huffman decoding. The flaw was originally tracked as CVE-2023-41064 in Apple's security update for iOS/macOS before being assigned CVE-2023-4863 for the underlying library. The same root cause affected all consumers of libwebp, making this a supply-chain-style vulnerability across the software ecosystem.

## Impact

CVE-2023-4863 enables heap buffer overflow via a maliciously crafted WebP image. In browsers, this was exploited for renderer process compromise, typically chained with a sandbox escape for full device compromise. The NSO Group's Pegasus spyware or similar commercial surveillance tools were believed to use this exploit chain against journalists and activists. The breadth of affected software (Chrome, Firefox, Edge, 1Password, Slack, Teams via Electron) meant that merely viewing a malicious image in any affected application was sufficient for exploitation.

## Mitigation

Update libwebp to version 1.3.2 or later. Chrome 116.0.5845.187, Firefox 117.0.1, Edge 116.0.1938.81, and later versions include the fix. For Electron applications, update to the patched Electron version. Organizations should inventory all software consuming libwebp. Detection of exploitation attempts is difficult as WebP images are common; focus on behavioral detection of unusual renderer process activity and unexpected child processes from browser applications.
