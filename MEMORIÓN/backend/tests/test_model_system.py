from __future__ import annotations

import hashlib
import asyncio
import json
from pathlib import Path

import httpx
import pytest

from backend.services.ai.downloader import Downloader
from backend.services.ai.manifest import ManifestError, ModelManifest, ModelSpec
from backend.services.ai.model_manager import ModelManager


def write_manifest(path: Path, payload: bytes, declared_size: int | None = None) -> None:
    digest = hashlib.sha256(payload).hexdigest()
    path.write_text(
        json.dumps(
            {
                "version": "1.0.0",
                "models": {
                    kind: {
                        "version": "1.0.0",
                        "filename": f"{kind}.gguf",
                        "url": f"https://models.test/{kind}.gguf",
                        "sha256": digest,
                        "size": len(payload) if declared_size is None else declared_size,
                    }
                    for kind in ("chat", "embedding")
                },
            }
        ),
        encoding="utf-8",
    )


def test_manifest_requires_both_model_types(tmp_path: Path) -> None:
    path = tmp_path / "manifest.json"
    path.write_text('{"version":"1","models":{}}', encoding="utf-8")
    with pytest.raises(ManifestError):
        ModelManifest.load(path)


def test_model_version_and_size_default_from_manifest(tmp_path: Path) -> None:
    payload = b"model"
    path = tmp_path / "manifest.json"
    write_manifest(path, payload, declared_size=0)
    raw = json.loads(path.read_text(encoding="utf-8"))
    for model in raw["models"].values():
        model.pop("version")
        model.pop("size")
    path.write_text(json.dumps(raw), encoding="utf-8")

    manifest = ModelManifest.load(path)
    assert manifest.models["chat"].version == manifest.version
    assert manifest.models["chat"].size == 0


def test_manager_downloads_and_reuses_valid_models(tmp_path: Path) -> None:
    payload = b"small fake gguf"
    manifest_path = tmp_path / "manifest.json"
    write_manifest(manifest_path, payload)
    requests = 0

    async def handler(request: httpx.Request) -> httpx.Response:
        nonlocal requests
        requests += 1
        return httpx.Response(200, content=payload, request=request)

    downloader = Downloader(transport=httpx.MockTransport(handler), retries=1)
    manager = ModelManager(manifest_path, tmp_path / "models", downloader=downloader)

    paths = asyncio.run(manager.ensure_models())
    assert paths.chat_model_path.read_bytes() == payload
    assert paths.embedding_model_path.read_bytes() == payload
    assert (tmp_path / "models" / "chat.metadata.json").is_file()

    asyncio.run(manager.ensure_models())
    assert requests == 2


def test_size_zero_is_inferred(tmp_path: Path) -> None:
    payload = b"size comes from HTTP"
    manifest_path = tmp_path / "manifest.json"
    write_manifest(manifest_path, payload, declared_size=0)

    async def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=payload, request=request)

    manager = ModelManager(
        manifest_path,
        tmp_path / "models",
        downloader=Downloader(transport=httpx.MockTransport(handler), retries=1),
    )
    paths = asyncio.run(manager.ensure_models())
    assert paths.chat_model_path.stat().st_size == len(payload)


def test_failed_update_preserves_previous_model(tmp_path: Path) -> None:
    models = tmp_path / "models"
    models.mkdir()
    old = models / "chat.gguf"
    old.write_bytes(b"old model")
    bad_payload = b"corrupt"
    spec = ModelSpec(
        type="chat",
        version="2",
        filename="chat.gguf",
        url="https://models.test/chat.gguf",
        sha256=hashlib.sha256(b"expected").hexdigest(),
        size=len(b"expected"),
    )

    async def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=bad_payload, request=request)

    downloader = Downloader(transport=httpx.MockTransport(handler), retries=1)
    with pytest.raises(Exception):
        asyncio.run(downloader.download(spec, models / "chat.gguf.tmp"))
    assert old.read_bytes() == b"old model"
    assert not (models / "chat.gguf.tmp").exists()
