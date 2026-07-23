from __future__ import annotations

import asyncio
from collections.abc import Callable
from pathlib import Path

import httpx

from .events import ModelEvent
from .manifest import ModelSpec
from .validator import ModelValidator

EventCallback = Callable[[ModelEvent], None]


class DownloadError(RuntimeError):
    pass


class Downloader:
    def __init__(
        self,
        validator: ModelValidator | None = None,
        event_callback: EventCallback | None = None,
        retries: int = 3,
        chunk_size: int = 1024 * 1024,
        timeout_seconds: float = 60.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.validator = validator or ModelValidator()
        self.event_callback = event_callback or (lambda _: None)
        self.retries = retries
        self.chunk_size = chunk_size
        self.timeout_seconds = timeout_seconds
        self.transport = transport

    async def download(self, spec: ModelSpec, temporary_path: Path) -> Path:
        temporary_path.parent.mkdir(parents=True, exist_ok=True)
        temporary_path.unlink(missing_ok=True)
        last_error: Exception | None = None

        for attempt in range(1, self.retries + 1):
            try:
                await self._download_once(spec, temporary_path)
                downloaded_size = temporary_path.stat().st_size
                self.event_callback(
                    ModelEvent(
                        status="VERIFYING",
                        model_type=spec.type,
                        progress=100.0,
                        total_bytes=spec.size or downloaded_size,
                        downloaded_bytes=downloaded_size,
                    )
                )
                await asyncio.to_thread(
                    self.validator.validate, temporary_path, spec.sha256, spec.size
                )
                return temporary_path
            except (httpx.HTTPError, OSError, ValueError) as exc:
                last_error = exc
                temporary_path.unlink(missing_ok=True)
                if attempt < self.retries:
                    self.event_callback(
                        ModelEvent(
                            status="PENDING",
                            model_type=spec.type,
                            message=(
                                f"Intento {attempt} falló: {exc}. "
                                f"Reintentando ({attempt + 1}/{self.retries})"
                            ),
                        )
                    )
                    await asyncio.sleep(min(2 ** (attempt - 1), 4))

        message = (
            f"No se pudo descargar {spec.type} después de {self.retries} intentos: {last_error}"
        )
        self.event_callback(
            ModelEvent(status="FAILED", model_type=spec.type, message=message)
        )
        raise DownloadError(message) from last_error

    async def _download_once(self, spec: ModelSpec, temporary_path: Path) -> None:
        timeout = httpx.Timeout(self.timeout_seconds, connect=30.0)
        async with httpx.AsyncClient(
            follow_redirects=True, timeout=timeout, transport=self.transport
        ) as client:
            async with client.stream("GET", spec.url) as response:
                response.raise_for_status()
                header_size = int(response.headers.get("content-length", "0"))
                total = spec.size or header_size
                downloaded = 0
                with temporary_path.open("wb") as output:
                    async for chunk in response.aiter_bytes(self.chunk_size):
                        output.write(chunk)
                        downloaded += len(chunk)
                        progress = min(downloaded / total * 100, 100.0) if total else 0.0
                        self.event_callback(
                            ModelEvent(
                                status="DOWNLOADING",
                                model_type=spec.type,
                                progress=round(progress, 2),
                                downloaded_bytes=downloaded,
                                total_bytes=total or None,
                            )
                        )
