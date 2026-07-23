BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS folder (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    canonical_path  TEXT NOT NULL UNIQUE,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_scanned_at TEXT,
    scan_status     TEXT NOT NULL DEFAULT 'pending',
    last_error      TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS document (
    id               INTEGER PRIMARY KEY,
    folder_id        INTEGER NOT NULL REFERENCES folder(id) ON DELETE CASCADE,
    relative_path    TEXT NOT NULL,
    mime_type        TEXT,
    size_bytes       INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_at      TEXT NOT NULL,
    content_hash     TEXT NOT NULL,
    indexing_status  TEXT NOT NULL DEFAULT 'pending',
    indexed_at       TEXT,
    last_error       TEXT,
    UNIQUE (folder_id, relative_path)
) STRICT;

CREATE TABLE IF NOT EXISTS knowledge_origin (
    id          INTEGER PRIMARY KEY,
    scope       TEXT NOT NULL CHECK (scope IN ('general', 'folder')),
    folder_id   INTEGER REFERENCES folder(id) ON DELETE CASCADE,
    user_input  TEXT NOT NULL CHECK (length(trim(user_input)) > 0),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (scope = 'general' AND folder_id IS NULL)
        OR (scope = 'folder' AND folder_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE IF NOT EXISTS ai_model (
    id             INTEGER PRIMARY KEY,
    provider       TEXT NOT NULL,
    model_key      TEXT NOT NULL,
    display_name   TEXT NOT NULL,
    version        TEXT,
    endpoint       TEXT NOT NULL DEFAULT '',
    metadata_json  TEXT CHECK (metadata_json IS NULL OR json_valid(metadata_json)),
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (provider, model_key, endpoint)
) STRICT;

CREATE TABLE IF NOT EXISTS model_capability (
    model_id              INTEGER NOT NULL REFERENCES ai_model(id) ON DELETE CASCADE,
    capability            TEXT NOT NULL,
    embedding_dimensions  INTEGER CHECK (embedding_dimensions IS NULL OR embedding_dimensions > 0),
    distance_metric       TEXT,
    context_window        INTEGER CHECK (context_window IS NULL OR context_window > 0),
    configuration_json    TEXT CHECK (configuration_json IS NULL OR json_valid(configuration_json)),
    PRIMARY KEY (model_id, capability)
) STRICT;

CREATE TABLE IF NOT EXISTS model_assignment (
    id             INTEGER PRIMARY KEY,
    model_id       INTEGER NOT NULL REFERENCES ai_model(id) ON DELETE CASCADE,
    task           TEXT NOT NULL,
    settings_json  TEXT CHECK (settings_json IS NULL OR json_valid(settings_json)),
    active         INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS uq_active_model_assignment
    ON model_assignment(task)
    WHERE active = 1;

CREATE TABLE IF NOT EXISTS knowledge_item (
    id                  INTEGER PRIMARY KEY,
    origin_id           INTEGER REFERENCES knowledge_origin(id) ON DELETE CASCADE,
    document_id         INTEGER REFERENCES document(id) ON DELETE CASCADE,
    folder_id           INTEGER REFERENCES folder(id) ON DELETE CASCADE,
    generator_model_id  INTEGER REFERENCES ai_model(id) ON DELETE SET NULL,
    scope               TEXT NOT NULL CHECK (scope IN ('general', 'folder')),
    source_type         TEXT NOT NULL,
    content             TEXT NOT NULL CHECK (length(trim(content)) > 0),
    content_hash        TEXT NOT NULL,
    is_confirmed        INTEGER NOT NULL DEFAULT 0 CHECK (is_confirmed IN (0, 1)),
    chunk_index         INTEGER CHECK (chunk_index IS NULL OR chunk_index >= 0),
    token_count         INTEGER CHECK (token_count IS NULL OR token_count >= 0),
    location_metadata   TEXT CHECK (location_metadata IS NULL OR json_valid(location_metadata)),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (scope = 'general' AND folder_id IS NULL)
        OR (scope = 'folder' AND folder_id IS NOT NULL)
    ),
    CHECK (
        document_id IS NULL
        OR folder_id IS NOT NULL
    )
) STRICT;

CREATE INDEX IF NOT EXISTS idx_document_folder
    ON document(folder_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_origin_folder
    ON knowledge_origin(folder_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_item_origin
    ON knowledge_item(origin_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_item_document
    ON knowledge_item(document_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_item_scope_folder
    ON knowledge_item(scope, folder_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_item_hash
    ON knowledge_item(content_hash);

/*
 * sqlite-vec metadata columns do not reliably accept NULL values. In this
 * virtual table folder_id=0 represents the general scope; canonical data in
 * knowledge_item continues to use NULL for general memories.
 */
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_vector USING vec0(
    knowledge_id INTEGER PRIMARY KEY,
    embedding FLOAT[768] distance_metric=cosine,
    embedding_model_id INTEGER,
    scope TEXT,
    folder_id INTEGER
);

PRAGMA user_version = 1;

COMMIT;
