"""Asynchronous fastrag HTTP client."""

from __future__ import annotations

import json
from typing import Any

import httpx

from .client import _build_headers, _raise_for_status
from .filters import FilterExpr
from .models import BatchResult, CorpusInfo, DeleteResult, IngestResult, SearchHit


class AsyncFastRAGClient:
    """Asynchronous client for fastrag's HTTP API."""

    def __init__(
        self,
        base_url: str,
        *,
        token: str | None = None,
        tenant_id: str | None = None,
        timeout: float = 30.0,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._client = httpx.AsyncClient(
            base_url=self._base_url,
            headers=_build_headers(token, tenant_id),
            timeout=timeout,
        )

    async def query(
        self,
        q: str,
        *,
        top_k: int = 5,
        corpus: str = "default",
        filter: FilterExpr | None = None,
        snippet_len: int = 150,
        fields: list[str] | None = None,
        rerank: str | None = None,
        over_fetch: int | None = None,
    ) -> list[SearchHit]:
        params: dict[str, Any] = {
            "q": q,
            "top_k": top_k,
            "corpus": corpus,
            "snippet_len": snippet_len,
        }
        if filter is not None:
            params["filter"] = str(filter)
        if fields is not None:
            params["fields"] = ",".join(fields)
        if rerank is not None:
            params["rerank"] = rerank
        if over_fetch is not None:
            params["over_fetch"] = over_fetch

        resp = await self._client.get("/query", params=params)
        _raise_for_status(resp)
        return [SearchHit.model_validate(h) for h in resp.json()]

    async def batch_query(
        self,
        queries: list[dict[str, Any]],
    ) -> list[BatchResult]:
        serialized = []
        for q in queries:
            item = dict(q)
            if "filter" in item and isinstance(item["filter"], FilterExpr):
                item["filter"] = str(item["filter"])
            serialized.append(item)

        resp = await self._client.post("/batch-query", json={"queries": serialized})
        _raise_for_status(resp)
        data = resp.json()
        return [BatchResult.model_validate(r) for r in data["results"]]

    async def ingest(
        self,
        records: list[dict[str, Any]],
        *,
        id_field: str,
        text_fields: list[str],
        metadata_fields: list[str] | None = None,
        metadata_types: dict[str, str] | None = None,
        array_fields: list[str] | None = None,
        chunk_strategy: str = "recursive",
        chunk_size: int = 1000,
        chunk_overlap: int = 200,
        corpus: str = "default",
    ) -> IngestResult:
        params: dict[str, Any] = {
            "corpus": corpus,
            "id_field": id_field,
            "text_fields": ",".join(text_fields),
            "chunk_strategy": chunk_strategy,
            "chunk_size": chunk_size,
            "chunk_overlap": chunk_overlap,
        }
        if metadata_fields:
            params["metadata_fields"] = ",".join(metadata_fields)
        if metadata_types:
            params["metadata_types"] = ",".join(f"{k}={v}" for k, v in metadata_types.items())
        if array_fields:
            params["array_fields"] = ",".join(array_fields)

        ndjson = "\n".join(json.dumps(r) for r in records) + "\n"
        resp = await self._client.post(
            "/ingest",
            content=ndjson.encode(),
            headers={"content-type": "application/x-ndjson"},
            params=params,
        )
        _raise_for_status(resp)
        return IngestResult.model_validate(resp.json())

    async def delete(self, id: str, *, corpus: str = "default") -> DeleteResult:
        resp = await self._client.delete(f"/ingest/{id}", params={"corpus": corpus})
        _raise_for_status(resp)
        return DeleteResult.model_validate(resp.json())

    async def stats(self, *, corpus: str = "default") -> dict[str, Any]:
        resp = await self._client.get("/stats", params={"corpus": corpus})
        _raise_for_status(resp)
        return resp.json()

    async def corpora(self) -> list[CorpusInfo]:
        resp = await self._client.get("/corpora")
        _raise_for_status(resp)
        return [CorpusInfo.model_validate(c) for c in resp.json()["corpora"]]

    async def health(self) -> bool:
        try:
            resp = await self._client.get("/health")
            return resp.status_code == 200
        except (httpx.ConnectError, httpx.TimeoutException):
            return False

    async def close(self) -> None:
        await self._client.aclose()

    async def __aenter__(self) -> AsyncFastRAGClient:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()
