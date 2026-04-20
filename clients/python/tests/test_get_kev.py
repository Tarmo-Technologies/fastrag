"""Tests for FastRAGClient.get_kev (thin lookup over GET /kev/{id})."""

from __future__ import annotations

import httpx
import pytest
import respx

from fastrag_client import FastRAGClient
from fastrag_client.errors import ServerError
from fastrag_client.models import SearchHit

_KEV_HIT: dict = {
    "score": 1.0,
    "chunk_text": "CISA KEV entry for Log4Shell.",
    "source_path": "kev/CVE-2021-44228",
    "chunk_index": 0,
    "pages": [],
    "element_kinds": [],
    "metadata": {"cve_id": "CVE-2021-44228", "kev_flag": "true"},
}


@respx.mock
def test_get_kev_happy_path(base_url: str) -> None:
    respx.get(f"{base_url}/kev/CVE-2021-44228").mock(
        return_value=httpx.Response(200, json={"hits": [_KEV_HIT]})
    )
    client = FastRAGClient(base_url=base_url)
    rec = client.get_kev("CVE-2021-44228")
    assert rec is not None
    assert isinstance(rec, SearchHit)
    assert rec.metadata["cve_id"] == "CVE-2021-44228"
    assert rec.metadata["kev_flag"] == "true"


@respx.mock
def test_get_kev_returns_none_on_404(base_url: str) -> None:
    respx.get(f"{base_url}/kev/CVE-9999-0000").mock(
        return_value=httpx.Response(404, json={"error": "kev_not_found", "id": "CVE-9999-0000"})
    )
    client = FastRAGClient(base_url=base_url)
    assert client.get_kev("CVE-9999-0000") is None


@respx.mock
def test_get_kev_returns_none_on_empty_hits(base_url: str) -> None:
    respx.get(f"{base_url}/kev/CVE-0-0").mock(return_value=httpx.Response(200, json={"hits": []}))
    client = FastRAGClient(base_url=base_url)
    assert client.get_kev("CVE-0-0") is None


@respx.mock
def test_get_kev_raises_on_5xx(base_url: str) -> None:
    respx.get(f"{base_url}/kev/CVE-1").mock(
        return_value=httpx.Response(503, json={"error": "bundle_not_loaded"})
    )
    client = FastRAGClient(base_url=base_url)
    with pytest.raises(ServerError):
        client.get_kev("CVE-1")
