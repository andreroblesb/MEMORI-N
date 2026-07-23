from __future__ import annotations

import hashlib
from pathlib import Path


class ModelValidationError(ValueError):
    pass


class ModelValidator:
    def sha256(self, path: Path, chunk_size: int = 4 * 1024 * 1024) -> str:
        digest = hashlib.sha256()
        with path.open("rb") as file:
            while chunk := file.read(chunk_size):
                digest.update(chunk)
        return digest.hexdigest()

    def validate(self, path: Path, expected_sha256: str, expected_size: int) -> None:
        if not path.is_file():
            raise ModelValidationError(f"No existe el modelo descargado: {path.name}")
        actual_size = path.stat().st_size
        if expected_size and actual_size != expected_size:
            raise ModelValidationError(
                f"Tamaño inválido para {path.name}: esperado {expected_size}, recibido {actual_size}"
            )
        actual_hash = self.sha256(path)
        if actual_hash.lower() != expected_sha256.lower():
            raise ModelValidationError(f"SHA256 inválido para {path.name}")
