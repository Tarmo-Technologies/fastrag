"""Pydantic v2 response models for fastrag API."""

from typing import Any

from pydantic import BaseModel, ConfigDict


class SearchHit(BaseModel):
    model_config = ConfigDict(extra="allow")

    score: float
    chunk_text: str = ""
    snippet: str | None = None
    source: dict[str, Any] | None = None
    source_path: str = ""
    chunk_index: int = 0
    section: str | None = None
    pages: list[int] = []
    element_kinds: list[str] = []
    language: str | None = None
    metadata: dict[str, Any] = {}


class BatchResult(BaseModel):
    model_config = ConfigDict(extra="allow")

    index: int
    hits: list[SearchHit] | None = None
    error: str | None = None


class IngestResult(BaseModel):
    model_config = ConfigDict(extra="allow")

    corpus: str
    records_new: int
    records_updated: int
    records_unchanged: int
    chunks_added: int


class DeleteResult(BaseModel):
    model_config = ConfigDict(extra="allow")

    corpus: str
    id: str
    deleted: bool


class CorpusInfo(BaseModel):
    model_config = ConfigDict(extra="allow")

    name: str
    path: str
    status: str
