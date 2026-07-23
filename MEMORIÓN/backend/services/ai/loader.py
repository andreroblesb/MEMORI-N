from __future__ import annotations

import gc
import os
from dataclasses import dataclass
from typing import Any, Protocol

from .model_manager import ModelPaths


class LlamaFactory(Protocol):
    def __call__(self, **kwargs: object) -> Any: ...


@dataclass(slots=True)
class LoadedModels:
    chat: Any
    embedding: Any


class ModelLoader:
    def __init__(self, factory: LlamaFactory | None = None) -> None:
        self._factory = factory
        self._models: LoadedModels | None = None

    @property
    def is_loaded(self) -> bool:
        return self._models is not None

    @property
    def models(self) -> LoadedModels:
        if self._models is None:
            raise RuntimeError("Los modelos todavía no están cargados")
        return self._models

    def load(self, paths: ModelPaths) -> LoadedModels:
        factory = self._factory or self._import_factory()
        chat = factory(
            model_path=str(paths.chat_model_path),
            n_ctx=int(os.getenv("MEMORION_CHAT_N_CTX", "4096")),
            n_threads=self._thread_count(),
            verbose=False,
        )
        try:
            embedding = factory(
                model_path=str(paths.embedding_model_path),
                embedding=True,
                n_ctx=int(os.getenv("MEMORION_EMBEDDING_N_CTX", "2048")),
                n_threads=self._thread_count(),
                verbose=False,
            )
        except Exception:
            del chat
            gc.collect()
            raise
        self._models = LoadedModels(chat=chat, embedding=embedding)
        return self._models

    def unload(self) -> None:
        self._models = None
        gc.collect()

    @staticmethod
    def _thread_count() -> int:
        configured = os.getenv("MEMORION_MODEL_THREADS")
        if configured:
            return max(1, int(configured))
        return max(1, (os.cpu_count() or 2) - 1)

    @staticmethod
    def _import_factory() -> LlamaFactory:
        try:
            from llama_cpp import Llama
        except ImportError as exc:
            raise RuntimeError(
                "llama-cpp-python no está instalado en el entorno del backend"
            ) from exc
        return Llama
