---
title: "CVE-2025-30065 — Apache Parquet Java Deserialization Remote Code Execution"
published_date: 2025-04-01
---

# Apache Parquet — Avro Schema Deserialization Remote Code Execution (CVE-2025-30065)

CVE-2025-30065 is a critical deserialization vulnerability in the `parquet-avro` module of the Apache Parquet Java library, disclosed on April 1, 2025. The flaw allows an attacker who can supply a malicious Parquet file to a vulnerable application to achieve remote code execution. CVSS score: 10.0 (Critical). Affected versions: Apache Parquet Java 1.15.0 and all earlier versions. The vulnerability is classified as CWE-502 (Deserialization of Untrusted Data). Although no confirmed in-the-wild exploitation was publicly reported at time of disclosure, the maximum severity score reflects the potential blast radius across enterprise data pipelines, analytics platforms, and machine learning infrastructure.

## Technical Details

The vulnerability resides in the Avro schema parsing logic within the `parquet-avro` module. Apache Parquet files embed Avro schema definitions that are parsed during file reading. The vulnerable code reflectively instantiates Java classes referenced within the schema — specifically, any class that accepts a `String` argument in its constructor can be instantiated by embedding the class name in a malicious Parquet file's schema metadata. When a vulnerable application reads a malicious Parquet file, the JVM instantiates the attacker-specified class via reflection, enabling arbitrary code execution in the context of the reading process. Exploitation requires only that the attacker can deliver a crafted `.parquet` file to be processed by the target application — a realistic threat model for data ingestion pipelines that consume externally sourced datasets.

## Impact

CVE-2025-30065 affects any application or service using Apache Parquet Java 1.15.0 or earlier that reads Parquet files from untrusted sources, including data warehouses, ETL pipelines, data lakes, analytics platforms, and machine learning training infrastructure. Parquet is one of the most widely used columnar data formats in the Hadoop/Spark/cloud analytics ecosystem; the library is embedded in AWS Glue, Databricks, Apache Spark, Apache Flink, and numerous other platforms. Successful exploitation grants the attacker code execution with the privileges of the data processing service, which often has broad access to cloud storage, databases, and credentials.

## Mitigation

Upgrade Apache Parquet Java to version 1.15.1 or later, which removes the unsafe reflective class instantiation from the Avro schema parser. For applications that cannot immediately patch, restrict Parquet file ingestion to trusted sources only and validate file provenance via cryptographic signatures or access controls. Audit data pipeline inputs to identify paths where externally supplied Parquet files reach the `parquet-avro` reader. Apply network-level egress controls on data processing infrastructure to limit post-exploitation blast radius.

## References

- https://nvd.nist.gov/vuln/detail/CVE-2025-30065
- https://www.endorlabs.com/learn/critical-rce-vulnerability-in-apache-parquet-cve-2025-30065---advisory-and-analysis
