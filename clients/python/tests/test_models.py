from typing import Any

from fastrag_client.models import (
    BatchResult,
    CorpusInfo,
    DeleteResult,
    IngestResult,
    SearchHit,
)


def test_search_hit_from_full_json():
    raw: dict[str, Any] = {
        "score": 0.87,
        "chunk_text": "SQL injection vulnerability",
        "snippet": "<b>SQL</b> injection",
        "source": {"id": "cve-1", "body": "SQL injection..."},
        "source_path": "cve-1",
        "chunk_index": 0,
        "section": "intro",
        "pages": [1, 2],
        "element_kinds": ["text"],
        "language": "en",
        "metadata": {"severity": "HIGH", "cvss": 9.8},
    }
    hit = SearchHit.model_validate(raw)
    assert hit.score == 0.87
    assert hit.snippet == "<b>SQL</b> injection"
    assert hit.source["id"] == "cve-1"
    assert hit.metadata["cvss"] == 9.8


def test_search_hit_minimal_json():
    raw: dict[str, Any] = {"score": 0.5}
    hit = SearchHit.model_validate(raw)
    assert hit.score == 0.5
    assert hit.chunk_text == ""
    assert hit.snippet is None
    assert hit.source is None
    assert hit.metadata == {}


def test_search_hit_extra_fields_allowed():
    raw: dict[str, Any] = {"score": 0.5, "new_field": "value"}
    hit = SearchHit.model_validate(raw)
    assert hit.score == 0.5


def test_batch_result_with_hits():
    raw: dict[str, Any] = {
        "index": 0,
        "hits": [{"score": 0.9, "chunk_text": "test"}],
    }
    result = BatchResult.model_validate(raw)
    assert result.index == 0
    assert len(result.hits) == 1
    assert result.hits[0].score == 0.9
    assert result.error is None


def test_batch_result_with_error():
    raw: dict[str, Any] = {"index": 1, "error": "corpus not found"}
    result = BatchResult.model_validate(raw)
    assert result.hits is None
    assert result.error == "corpus not found"


def test_ingest_result():
    raw: dict[str, Any] = {
        "corpus": "default",
        "records_new": 2,
        "records_updated": 0,
        "records_unchanged": 0,
        "chunks_added": 6,
    }
    result = IngestResult.model_validate(raw)
    assert result.corpus == "default"
    assert result.records_new == 2
    assert result.chunks_added == 6


def test_delete_result():
    raw: dict[str, Any] = {"corpus": "default", "id": "cve-1", "deleted": True}
    result = DeleteResult.model_validate(raw)
    assert result.deleted is True


def test_corpus_info():
    raw: dict[str, Any] = {"name": "default", "path": "/data/corpus", "status": "ready"}
    info = CorpusInfo.model_validate(raw)
    assert info.name == "default"
    assert info.status == "ready"
