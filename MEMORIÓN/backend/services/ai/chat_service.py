from __future__ import annotations

from threading import Lock
from typing import Any, Literal

from pydantic import BaseModel, Field


class ChatMessage(BaseModel):
    role: Literal["user", "assistant"]
    content: str = Field(min_length=1, max_length=32_000)


class ChatRequest(BaseModel):
    messages: list[ChatMessage] = Field(min_length=1, max_length=50)
    memories: list[str] = Field(default_factory=list, max_length=8)
    temperature: float = Field(default=0.7, ge=0.0, le=2.0)
    max_tokens: int = Field(default=768, ge=1, le=4096)


class ChatResponse(BaseModel):
    content: str


class ChatService:
    """Runs conversational inference without persistence or memory extraction."""

    def __init__(self, model: Any, lock: Lock | None = None) -> None:
        self._model = model
        self._lock = lock or Lock()

    def complete(self, request: ChatRequest) -> ChatResponse:
        memory_context = ""
        if request.memories:
            memory_context = (
                "\n\nMemorias relevantes proporcionadas por MEMORIÓN. Úsalas solo "
                "cuando ayuden a contestar y no inventes detalles:\n- "
                + "\n- ".join(request.memories)
            )
        messages: list[dict[str, str]] = [
            {
                "role": "system",
                "content": (
                    f"""Eres MEMORIÓN, un asesor personal de memoria local. Tu trabajo es
                    ayudar al usuario a conservar y recuperar información importante. Puedes ayudar de dos maneras: 1) invitar al usuario a contarte algo que quiera recordar, mediante un mensaje o un documento; y 2) ayudar a encontrar información a partir de lo que previamente decidió recordar. Cuando su intención no sea clara, pregúntale si quiere contarte algo para recordarlo o buscar algo que ya te contó. Responde en el idioma del usuario, con claridad y sin inventar recuerdos. No afirmes que una información fue guardada o recuperada si no aparece en la conversación o en las memorias proporcionadas.{memory_context}"""),
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
