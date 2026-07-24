from __future__ import annotations

import json
import math
import re
from threading import Lock
from typing import Any, Literal

from pydantic import BaseModel, Field

from .chat_service import ChatMessage


class KnowledgeExtractionRequest(BaseModel):
    messages: list[ChatMessage] = Field(min_length=1, max_length=5)


class KnowledgeExtractionResponse(BaseModel):
    should_store: bool
    content: str | None = None


class DocumentKnowledgeRequest(BaseModel):
    text: str = Field(min_length=1, max_length=8_000)


class KnowledgeCandidatesResponse(BaseModel):
    candidates: list[str]


class KnowledgeRefinementRequest(BaseModel):
    candidates: list[str] = Field(min_length=1, max_length=16)
    source_type: Literal["chat", "document"]


class KnowledgeRefinementResponse(BaseModel):
    items: list[str]


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

    def extract_document(
        self, request: DocumentKnowledgeRequest
    ) -> KnowledgeCandidatesResponse:
        prompt = (
            "Extrae del fragmento documental entre 1 y 12 conocimientos candidatos "
            "útiles para responder preguntas sobre su contenido. Omite encabezados "
            "aislados, texto de navegación, ruido y repeticiones. Conserva nombres, "
            "fechas, cantidades, condiciones y relaciones. No inventes información ni "
            "uses conocimiento externo. Devuelve JSON estricto con esta forma: "
            '{"candidates":["..."]}. Si no hay contenido informativo devuelve '
            '{"candidates":[]}.\n\nFragmento:\n' + request.text
        )
        parsed = self._json_completion(prompt, max_tokens=700)
        return KnowledgeCandidatesResponse(
            candidates=self._clean_items(parsed.get("candidates"), maximum=12)
        )

    def refine(
        self, request: KnowledgeRefinementRequest
    ) -> KnowledgeRefinementResponse:
        candidates = "\n".join(f"- {item}" for item in request.candidates)
        prompt = (
            "Eres la segunda revisión de memoria de MEMORIÓN. Revisa los candidatos "
            "sin consultar fuentes externas. Puedes conservar, reformular, fusionar o "
            "dividir. Cada resultado debe ser autocontenido, claro y fiel; debe resolver "
            "pronombres solo cuando el candidato permita hacerlo. Separa hechos que "
            "puedan recordarse independientemente, pero no rompas una relación o "
            "condición inseparable. Elimina duplicados y frases vacías. No agregues "
            "explicaciones. Devuelve JSON estricto: {\"items\":[\"...\"]}.\n\n"
            "Ejemplos:\n"
            "Candidato: El perro del usuario se llama Milo, tiene seis años y consume "
            "alimento renal desde abril.\n"
            "Resultado: {\"items\":[\"El perro del usuario se llama Milo.\","
            "\"Milo tiene seis años.\",\"Milo consume alimento renal desde abril.\"]}\n\n"
            "Candidato: La reunión será el 14 de agosto a las 10:00 en la sala Norte.\n"
            "Resultado: {\"items\":[\"La reunión será el 14 de agosto a las 10:00 "
            "en la sala Norte.\"]}\n\n"
            "Candidatos: El proyecto usa SQLite. / La persistencia del proyecto usa "
            "SQLite.\n"
            "Resultado: {\"items\":[\"El proyecto usa SQLite para persistencia.\"]}\n\n"
            f"Tipo de fuente: {request.source_type}\nCandidatos:\n{candidates}"
        )
        parsed = self._json_completion(prompt, max_tokens=900)
        items = self._clean_items(parsed.get("items"), maximum=20)
        if not items:
            items = self._clean_items(request.candidates, maximum=20)
        return KnowledgeRefinementResponse(items=items)

    def _json_completion(self, prompt: str, max_tokens: int) -> dict[str, Any]:
        with self._chat_lock:
            result = self._chat_model.create_chat_completion(
                messages=[{"role": "user", "content": prompt}],
                temperature=0.0,
                max_tokens=max_tokens,
                stream=False,
            )
        raw = result["choices"][0]["message"]["content"]
        if not isinstance(raw, str):
            return {}
        match = re.search(r"\{.*\}", raw, re.DOTALL)
        if not match:
            return {}
        try:
            parsed = json.loads(match.group(0))
        except json.JSONDecodeError:
            return {}
        return parsed if isinstance(parsed, dict) else {}

    @staticmethod
    def _clean_items(value: Any, maximum: int) -> list[str]:
        if not isinstance(value, list):
            return []
        result: list[str] = []
        seen: set[str] = set()
        for item in value:
            if not isinstance(item, str):
                continue
            clean = " ".join(item.split()).strip()
            key = clean.casefold()
            if not clean or key in seen:
                continue
            seen.add(key)
            result.append(clean[:2_000])
            if len(result) >= maximum:
                break
        return result

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
