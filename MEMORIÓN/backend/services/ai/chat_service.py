from __future__ import annotations

from threading import Lock
from typing import Any, Literal

from pydantic import BaseModel, Field


class ChatMessage(BaseModel):
    role: Literal["user", "assistant"]
    content: str = Field(min_length=1, max_length=32_000)


class ChatRequest(BaseModel):
    messages: list[ChatMessage] = Field(min_length=1, max_length=50)
    temperature: float = Field(default=0.7, ge=0.0, le=2.0)
    max_tokens: int = Field(default=768, ge=1, le=4096)


class ChatResponse(BaseModel):
    content: str


class ChatService:
    """Runs conversational inference without persistence or memory extraction."""

    def __init__(self, model: Any) -> None:
        self._model = model
        self._lock = Lock()

    def complete(self, request: ChatRequest) -> ChatResponse:
        messages: list[dict[str, str]] = [
            {
                "role": "system",
                "content": (
                    "Eres MEMORIÓN, un asistente local claro y útil. "
                    "Responde en el idioma del usuario. No afirmes recordar datos "
                    "fuera de la conversación proporcionada."
                ),
            },
            *[message.model_dump() for message in request.messages],
        ]
        with self._lock:
            result = self._model.create_chat_completion(
                messages=messages,
                temperature=request.temperature,
                max_tokens=request.max_tokens,
                stream=False,
            )
        try:
            content = result["choices"][0]["message"]["content"]
        except (KeyError, IndexError, TypeError) as exc:
            raise RuntimeError("El modelo devolvió una respuesta sin contenido") from exc
        if not isinstance(content, str) or not content.strip():
            raise RuntimeError("El modelo devolvió una respuesta vacía")
        return ChatResponse(content=content.strip())
