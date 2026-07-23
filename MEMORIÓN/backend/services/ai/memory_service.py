from __future__ import annotations

import json
import math
import re
from threading import Lock
from typing import Any

from pydantic import BaseModel, Field

from .chat_service import ChatMessage


class KnowledgeExtractionRequest(BaseModel):
    messages: list[ChatMessage] = Field(min_length=1, max_length=5)


class KnowledgeExtractionResponse(BaseModel):
    should_store: bool
    content: str | None = None


class EmbeddingRequest(BaseModel):
    text: str = Field(min_length=1, max_length=32_000)


class EmbeddingResponse(BaseModel):
    embedding: list[float]
    dimensions: int


class MemoryAiService:
    def __init__(
        self,
        chat_model: Any,
        embedding_model: Any,
        chat_lock: Lock,
        embedding_lock: Lock,
    ) -> None:
        self._chat_model = chat_model
        self._embedding_model = embedding_model
        self._chat_lock = chat_lock
        self._embedding_lock = embedding_lock

    def extract(self, request: KnowledgeExtractionRequest) -> KnowledgeExtractionResponse:
        transcript = "\n".join(
            f"{message.role}: {message.content}" for message in request.messages
        )
        prompt = (
            "Analiza el último mensaje del usuario. Guarda solo datos declarativos "
            "duraderos y útiles sobre el usuario, sus preferencias, personas, mascotas, "
            "proyectos o hechos que explícitamente afirma. No guardes preguntas, saludos, "
            "órdenes, hipótesis, contenido trivial ni información inferida. Devuelve JSON "
            'estricto: {"should_store":boolean,"content":string|null}. Si guardas, content '
            "debe ser una afirmación autocontenida, breve y fiel, sin añadir información.\n\n"
            f"Conversación:\n{transcript}"
        )
        with self._chat_lock:
            result = self._chat_model.create_chat_completion(
                messages=[{"role": "user", "content": prompt}],
                temperature=0.0,
                max_tokens=180,
                stream=False,
            )
        raw = result["choices"][0]["message"]["content"]
        if not isinstance(raw, str):
            return KnowledgeExtractionResponse(should_store=False)
        match = re.search(r"\{.*\}", raw, re.DOTALL)
        if not match:
            return KnowledgeExtractionResponse(should_store=False)
        try:
            parsed = json.loads(match.group(0))
        except json.JSONDecodeError:
            return KnowledgeExtractionResponse(should_store=False)
        content = parsed.get("content")
        should_store = parsed.get("should_store") is True and isinstance(content, str)
        return KnowledgeExtractionResponse(
            should_store=should_store,
            content=content.strip() if should_store and content.strip() else None,
        )

    def embed(self, request: EmbeddingRequest) -> EmbeddingResponse:
        with self._embedding_lock:
            result = self._embedding_model.create_embedding(request.text)
        vector = result["data"][0]["embedding"]
        if not isinstance(vector, list) or not vector:
            raise RuntimeError("El modelo no devolvió un embedding")
        norm = math.sqrt(sum(float(value) ** 2 for value in vector))
        if norm == 0:
            raise RuntimeError("El embedding tiene norma cero")
        normalized = [float(value) / norm for value in vector]
        return EmbeddingResponse(embedding=normalized, dimensions=len(normalized))
