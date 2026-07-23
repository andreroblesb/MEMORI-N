# Modelos locales de MEMORIÓN

El backend solo distingue las capacidades `chat` y `embedding`. Las URL, nombres,
versiones y hashes viven exclusivamente en `manifest.json`.

## Configuración

Edita `manifest.json` y completa `url` y `sha256` para ambos modelos. `size` es
opcional: puede permanecer en `0`; si contiene un valor positivo también se
validará el tamaño exacto.

Los modelos se guardan en el directorio de datos de usuario que resuelve
`platformdirs`, dentro de `MEMORIÓN/models`. No se escriben en el repositorio.

## Ejecución de desarrollo

Desde la raíz del proyecto:

```powershell
python -m pip install -r backend/requirements.txt
python -m uvicorn backend.main:app --host 127.0.0.1 --port 8000
```

El ciclo de vida de FastAPI inicia la descarga en segundo plano. Sus endpoints
son:

- `GET /health`
- `GET /api/models/status`
- `GET /api/models/events` (Server-Sent Events)
- `POST /api/models/retry`

El progreso aparece en la misma consola de Uvicorn y se conserva en:

```text
AppData/Local/MEMORIÓN/logs/model-download.log
```

`GET /api/models/status` también devuelve `manifest_path`, `models_directory` y
`log_path`, para que las rutas reales nunca tengan que adivinarse.

La carga acepta ajustes genéricos mediante `MEMORION_CHAT_N_CTX`,
`MEMORION_EMBEDDING_N_CTX` y `MEMORION_MODEL_THREADS`. Ninguna variable contiene
el nombre comercial de un modelo.

Para distribución desktop, el backend Python deberá empaquetarse como sidecar de
Tauri incluyendo `manifest.json`. Esa fase de empaquetado es independiente del
gestor y no cambia su API.
