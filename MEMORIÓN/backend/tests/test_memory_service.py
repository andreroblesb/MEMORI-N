import json
from threading import Lock

from backend.services.ai.memory_service import (
    EmbeddingRequest,
    KnowledgeExtractionRequest,
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
