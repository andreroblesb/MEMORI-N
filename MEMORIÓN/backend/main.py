from __future__ import annotations

import asyncio
import json
from contextlib import asynccontextmanager, suppress

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse

from .services.ai.chat_service import ChatRequest, ChatResponse, ChatService
from .services.ai.events import ModelEvent, ModelEventBus
from .services.ai.loader import ModelLoader
from .services.ai.logging_config import configure_model_logging
from .services.ai.model_manager import ModelManager


model_logger, model_log_path = configure_model_logging()
event_bus = ModelEventBus(logger=model_logger)
model_manager = ModelManager.from_defaults(event_callback=event_bus.publish)
model_loader = ModelLoader()
chat_service: ChatService | None = None
initialization_task: asyncio.Task[None] | None = None


async def initialize_models() -> None:
    global chat_service
    event_bus.publish(ModelEvent(status="PENDING", message="Preparando modelos"))
    model_logger.info("Manifest: %s", model_manager.manifest_path)
    model_logger.info("Carpeta de modelos: %s", model_manager.models_directory)
    try:
        paths = await model_manager.ensure_models()
        event_bus.publish_loading()
        await asyncio.to_thread(model_loader.load, paths)
        chat_service = ChatService(model_loader.models.chat)
        event_bus.publish_ready(paths)
    except asyncio.CancelledError:
        raise
    except Exception as exc:
        model_logger.exception("Inicialización de modelos fallida")
        event_bus.publish_failure(str(exc))


@asynccontextmanager
async def lifespan(_: FastAPI):
    global initialization_task
    initialization_task = asyncio.create_task(initialize_models())
    yield
    if initialization_task and not initialization_task.done():
        initialization_task.cancel()
        with suppress(asyncio.CancelledError):
            await initialization_task
    model_loader.unload()


app = FastAPI(title="MEMORIÓN backend", version="0.2.0", lifespan=lifespan)
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ],
    allow_methods=["GET", "POST"],
    allow_headers=["*"],
)


@app.get("/health")
def health() -> dict[str, object]:
    snapshot = event_bus.snapshot()
    state = snapshot["state"]
    status = state["status"] if isinstance(state, dict) else "PENDING"
    return {
        "status": (
            "ready"
            if model_loader.is_loaded
            else "failed"
            if status == "FAILED"
            else "starting"
        ),
        "models": snapshot,
    }


@app.get("/api/models/status")
def model_status() -> dict[str, object]:
    return {
        **event_bus.snapshot(),
        "manifest_path": str(model_manager.manifest_path),
        "models_directory": str(model_manager.models_directory),
        "log_path": str(model_log_path),
    }


@app.get("/api/models/events")
async def model_events() -> StreamingResponse:
    async def stream():
        async for event in event_bus.subscribe():
            yield f"event: model-progress\ndata: {json.dumps(event, ensure_ascii=False)}\n\n"

    return StreamingResponse(
        stream(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )


@app.post("/api/models/retry", status_code=202)
async def retry_models() -> dict[str, str]:
    global initialization_task
    if initialization_task and not initialization_task.done():
        return {"status": "already_running"}
    initialization_task = asyncio.create_task(initialize_models())
    return {"status": "started"}


@app.post("/api/chat", response_model=ChatResponse)
async def chat(request: ChatRequest) -> ChatResponse:
    if not model_loader.is_loaded or chat_service is None:
        raise HTTPException(
            status_code=503,
            detail="Los modelos todavía se están preparando. Intenta nuevamente en unos segundos.",
        )
    try:
        return await asyncio.to_thread(chat_service.complete, request)
    except Exception as exc:
        model_logger.exception("La inferencia conversacional falló")
        raise HTTPException(status_code=500, detail=str(exc)) from exc
