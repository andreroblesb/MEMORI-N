from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Literal
from urllib.parse import urlparse

ModelType = Literal["chat", "embedding"]
MODEL_TYPES: tuple[ModelType, ...] = ("chat", "embedding")


class ManifestError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class ModelSpec:
    type: ModelType
    version: str
    filename: str
    url: str
    sha256: str
    size: int

    @classmethod
    def from_dict(
        cls, model_type: ModelType, value: object, default_version: str
    ) -> "ModelSpec":
        if not isinstance(value, dict):
            raise ManifestError(f"models.{model_type} debe ser un objeto")
        try:
            spec = cls(
                type=model_type,
                version=str(value.get("version", default_version)).strip(),
                filename=str(value["filename"]).strip(),
                url=str(value["url"]).strip(),
                sha256=str(value["sha256"]).strip().lower(),
                size=int(value.get("size", 0)),
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise ManifestError(f"models.{model_type} está incompleto") from exc
        spec.validate()
        return spec

    def validate(self) -> None:
        if not self.version:
            raise ManifestError(f"models.{self.type}.version está vacío")
        if not self.filename or Path(self.filename).name != self.filename:
            raise ManifestError(f"models.{self.type}.filename debe ser un nombre de archivo")
        if not self.filename.lower().endswith(".gguf"):
            raise ManifestError(f"models.{self.type}.filename debe terminar en .gguf")
        parsed = urlparse(self.url)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise ManifestError(f"models.{self.type}.url debe ser una URL HTTP(S)")
        if len(self.sha256) != 64 or any(c not in "0123456789abcdef" for c in self.sha256):
            raise ManifestError(f"models.{self.type}.sha256 debe tener 64 caracteres hexadecimales")
        if self.size < 0:
            raise ManifestError(f"models.{self.type}.size no puede ser negativo")


@dataclass(frozen=True, slots=True)
class ModelManifest:
    version: str
    models: dict[ModelType, ModelSpec]

    @classmethod
    def load(cls, path: Path) -> "ModelManifest":
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError as exc:
            raise ManifestError(f"No existe el manifest: {path}") from exc
        except json.JSONDecodeError as exc:
            raise ManifestError(f"El manifest no contiene JSON válido: {exc}") from exc
        if not isinstance(raw, dict) or not str(raw.get("version", "")).strip():
            raise ManifestError("El manifest necesita una version")
        models = raw.get("models")
        if not isinstance(models, dict):
            raise ManifestError("El manifest necesita el objeto models")
        manifest_version = str(raw["version"]).strip()
        parsed_models = {
            kind: ModelSpec.from_dict(kind, models.get(kind), manifest_version)
            for kind in MODEL_TYPES
        }
        if len({spec.filename for spec in parsed_models.values()}) != len(MODEL_TYPES):
            raise ManifestError("chat y embedding deben usar filenames distintos")
        return cls(
            version=manifest_version,
            models=parsed_models,
        )
