from __future__ import annotations

import asyncio
import json
import os
import sys
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

from .downloader import Downloader, EventCallback
from .events import ModelEvent
from .logging_config import application_data_directory
from .manifest import MODEL_TYPES, ModelManifest, ModelSpec, ModelType
from .validator import ModelValidator


@dataclass(frozen=True, slots=True)
class ModelPaths:
    chat_model_path: Path
    embedding_model_path: Path


@dataclass(frozen=True, slots=True)
class ModelMetadata:
    version: str
    sha256: str
    download_date: str
    size: int
    mtime_ns: int


def bundled_manifest_path() -> Path:
    if getattr(sys, "frozen", False):
        base = Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
        candidate = base / "manifest.json"
        if candidate.exists():
            return candidate
    return Path(__file__).resolve().parents[2] / "manifest.json"


class ModelManager:
    def __init__(
        self,
        manifest_path: Path,
        models_directory: Path,
        downloader: Downloader | None = None,
        validator: ModelValidator | None = None,
        event_callback: EventCallback | None = None,
    ) -> None:
        self.manifest_path = manifest_path
        self.models_directory = models_directory
        self.validator = validator or ModelValidator()
        self.event_callback = event_callback or (lambda _: None)
        self.downloader = downloader or Downloader(
            validator=self.validator, event_callback=self.event_callback
        )
        self._lock = asyncio.Lock()

    @classmethod
    def from_defaults(
        cls, event_callback: EventCallback | None = None, app_name: str = "MEMORIÓN"
    ) -> "ModelManager":
        return cls(
            manifest_path=bundled_manifest_path(),
            models_directory=application_data_directory(app_name) / "models",
            event_callback=event_callback,
        )

    async def ensure_models(self) -> ModelPaths:
        async with self._lock:
            manifest = ModelManifest.load(self.manifest_path)
            self.models_directory.mkdir(parents=True, exist_ok=True)
            for spec in manifest.models.values():
                (self.models_directory / f"{spec.filename}.tmp").unlink(missing_ok=True)
            resolved: dict[ModelType, Path] = {}
            for model_type in MODEL_TYPES:
                resolved[model_type] = await self._ensure_model(manifest.models[model_type])
            return ModelPaths(
                chat_model_path=resolved["chat"],
                embedding_model_path=resolved["embedding"],
            )

    def get_model_path(self, model_type: ModelType) -> Path:
        manifest = ModelManifest.load(self.manifest_path)
        return self.models_directory / manifest.models[model_type].filename

    async def _ensure_model(self, spec: ModelSpec) -> Path:
        target = self.models_directory / spec.filename
        temporary = self.models_directory / f"{spec.filename}.tmp"
        metadata_path = self.models_directory / f"{target.stem}.metadata.json"
        temporary.unlink(missing_ok=True)

        if await asyncio.to_thread(self._is_current, target, metadata_path, spec):
            self.event_callback(
                ModelEvent(status="COMPLETED", model_type=spec.type, progress=100.0)
            )
            return target

        await self.downloader.download(spec, temporary)
        # os.replace is atomic on the same filesystem and leaves the old target
        # untouched until the validated temporary file is ready.
        os.replace(temporary, target)
        metadata = ModelMetadata(
            version=spec.version,
            sha256=spec.sha256,
            download_date=datetime.now(UTC).isoformat(),
            size=target.stat().st_size,
            mtime_ns=target.stat().st_mtime_ns,
        )
        self._write_metadata_atomic(metadata_path, metadata)
        self.event_callback(
            ModelEvent(status="COMPLETED", model_type=spec.type, progress=100.0)
        )
        return target

    def _is_current(self, target: Path, metadata_path: Path, spec: ModelSpec) -> bool:
        if not target.is_file():
            return False
        metadata = self._read_metadata(metadata_path)
        stat = target.stat()
        if (
            metadata
            and metadata.version == spec.version
            and metadata.sha256 == spec.sha256
            and metadata.size == stat.st_size
            and (spec.size == 0 or spec.size == stat.st_size)
            and metadata.mtime_ns == stat.st_mtime_ns
        ):
            return True
        try:
            self.validator.validate(target, spec.sha256, spec.size)
        except ValueError:
            return False
        refreshed = ModelMetadata(
            version=spec.version,
            sha256=spec.sha256,
            download_date=metadata.download_date
            if metadata
            else datetime.now(UTC).isoformat(),
            size=stat.st_size,
            mtime_ns=stat.st_mtime_ns,
        )
        self._write_metadata_atomic(metadata_path, refreshed)
        return True

    @staticmethod
    def _read_metadata(path: Path) -> ModelMetadata | None:
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
            return ModelMetadata(
                version=str(raw["version"]),
                sha256=str(raw["sha256"]).lower(),
                download_date=str(raw["download_date"]),
                size=int(raw["size"]),
                mtime_ns=int(raw["mtime_ns"]),
            )
        except (FileNotFoundError, KeyError, TypeError, ValueError, json.JSONDecodeError):
            return None

    @staticmethod
    def _write_metadata_atomic(path: Path, metadata: ModelMetadata) -> None:
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_text(
            json.dumps(asdict(metadata), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        os.replace(temporary, path)
