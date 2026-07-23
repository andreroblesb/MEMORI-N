from __future__ import annotations

import asyncio
import logging
from collections.abc import AsyncIterator
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from threading import Lock
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from .model_manager import ModelPaths

EventStatus = Literal[
    "PENDING", "DOWNLOADING", "VERIFYING", "COMPLETED", "LOADING", "READY", "FAILED"
]


@dataclass(frozen=True, slots=True)
class ModelEvent:
    status: EventStatus
    model_type: str | None = None
    progress: float | None = None
    downloaded_bytes: int | None = None
    total_bytes: int | None = None
    message: str | None = None
    timestamp: str = ""

    def to_dict(self) -> dict[str, object]:
        value = asdict(self)
        value["timestamp"] = self.timestamp or datetime.now(UTC).isoformat()
        return value


class ModelEventBus:
    def __init__(self, logger: logging.Logger | None = None) -> None:
        self._latest: dict[str, dict[str, object]] = {}
        self._global: dict[str, object] = ModelEvent(status="PENDING").to_dict()
        self._subscribers: set[asyncio.Queue[dict[str, object]]] = set()
        self._lock = Lock()
        self._logger = logger
        self._logged_progress_bucket: dict[str, int] = {}

    def publish(self, event: ModelEvent) -> None:
        payload = event.to_dict()
        with self._lock:
            if event.model_type:
                self._latest[event.model_type] = payload
            else:
                self._global = payload
            subscribers = tuple(self._subscribers)
        self._log_event(event)
        for queue in subscribers:
            try:
                queue.put_nowait(payload)
            except asyncio.QueueFull:
                pass

    def _log_event(self, event: ModelEvent) -> None:
        if self._logger is None:
            return
        label = event.model_type or "sistema"
        if event.status == "DOWNLOADING":
            bucket = int((event.progress or 0) // 5)
            if self._logged_progress_bucket.get(label) == bucket:
                return
            self._logged_progress_bucket[label] = bucket
            downloaded = _format_bytes(event.downloaded_bytes)
            total = _format_bytes(event.total_bytes)
            self._logger.info(
                "[%s] DOWNLOADING %.1f%% (%s / %s)",
                label,
                event.progress or 0,
                downloaded,
                total,
            )
            return
        message = f" - {event.message}" if event.message else ""
        log = self._logger.error if event.status == "FAILED" else self._logger.info
        log("[%s] %s%s", label, event.status, message)

    def publish_loading(self) -> None:
        self.publish(ModelEvent(status="LOADING", message="Cargando modelos en memoria"))

    def publish_ready(self, paths: "ModelPaths") -> None:
        self.publish(
            ModelEvent(
                status="READY",
                progress=100.0,
                message=f"Modelos disponibles en {paths.chat_model_path.parent}",
            )
        )

    def publish_failure(self, message: str) -> None:
        self.publish(ModelEvent(status="FAILED", message=message))

    def snapshot(self) -> dict[str, object]:
        with self._lock:
            return {"state": dict(self._global), "models": dict(self._latest)}

    async def subscribe(self) -> AsyncIterator[dict[str, object]]:
        queue: asyncio.Queue[dict[str, object]] = asyncio.Queue(maxsize=100)
        with self._lock:
            self._subscribers.add(queue)
            initial = {"status": "SNAPSHOT", **self.snapshot_unlocked()}
        try:
            yield initial
            while True:
                yield await queue.get()
        finally:
            with self._lock:
                self._subscribers.discard(queue)

    def snapshot_unlocked(self) -> dict[str, object]:
        return {"state": dict(self._global), "models": dict(self._latest)}


def _format_bytes(value: int | None) -> str:
    if value is None:
        return "desconocido"
    size = float(value)
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if size < 1024 or unit == "TB":
            return f"{size:.1f} {unit}"
        size /= 1024
    return f"{size:.1f} TB"
