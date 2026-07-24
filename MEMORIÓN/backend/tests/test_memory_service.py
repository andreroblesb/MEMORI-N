import json
from threading import Lock

from backend.services.ai.memory_service import (
    DocumentKnowledgeRequest,
    EmbeddingRequest,
    KnowledgeExtractionRequest,
    KnowledgeRefinementRequest,
    MemoryAiService,
)


class FakeChat:
    def create_chat_completion(self, **_):
        return {
            "choices": [
                {
                    "message": {
                        "content": json.dumps(
                            {"should_store": True, "content": "El perro del usuario se llama Milo."}
                        )
                    }
                }
            ]
        }


class FakeEmbedding:
    def create_embedding(self, _):
        return {"data": [{"embedding": [3.0, 4.0]}]}


def test_extracts_declarative_knowledge_and_normalizes_embedding() -> None:
    service = MemoryAiService(FakeChat(), FakeEmbedding(), Lock(), Lock())
    extraction = service.extract(
        KnowledgeExtractionRequest(
            messages=[{"role": "user", "content": "Mi perro se llama Milo"}]
        )
    )
    assert extraction.should_store
    assert extraction.content == "El perro del usuario se llama Milo."
    embedding = service.embed(EmbeddingRequest(text=extraction.content))
    assert embedding.dimensions == 2
    assert embedding.embedding == [0.6, 0.8]


class FakeTwoPassChat:
    def create_chat_completion(self, **kwargs):
        prompt = kwargs["messages"][0]["content"]
        if "Fragmento:" in prompt:
            payload = {
                "candidates": [
                    "El perro del usuario se llama Milo, tiene seis años y consume alimento renal."
                ]
            }
        else:
            payload = {
                "items": [
                    "El perro del usuario se llama Milo.",
                    "Milo tiene seis años.",
                    "Milo consume alimento renal.",
                ]
            }
        return {"choices": [{"message": {"content": json.dumps(payload)}}]}


def test_document_knowledge_is_extracted_then_split_by_review() -> None:
    service = MemoryAiService(FakeTwoPassChat(), FakeEmbedding(), Lock(), Lock())
    candidates = service.extract_document(
        DocumentKnowledgeRequest(
            text="El perro del usuario se llama Milo, tiene seis años y consume alimento renal."
        )
    )
    refined = service.refine(
        KnowledgeRefinementRequest(
            candidates=candidates.candidates,
            source_type="document",
        )
    )
    assert refined.items == [
        "El perro del usuario se llama Milo.",
        "Milo tiene seis años.",
        "Milo consume alimento renal.",
    ]
