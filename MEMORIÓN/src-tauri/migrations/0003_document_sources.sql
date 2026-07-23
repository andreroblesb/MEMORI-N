PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

DROP TRIGGER IF EXISTS delete_knowledge_vector;
DROP INDEX IF EXISTS idx_document_folder;
DROP INDEX IF EXISTS idx_knowledge_item_origin;
DROP INDEX IF EXISTS idx_knowledge_item_document;
DROP INDEX IF EXISTS idx_knowledge_item_scope_folder;
DROP INDEX IF EXISTS idx_knowledge_item_hash;

ALTER TABLE knowledge_item RENAME TO knowledge_item_old;
ALTER TABLE document RENAME TO document_old;

CREATE TABLE document (
    id               INTEGER PRIMARY KEY,
    scope            TEXT NOT NULL CHECK (scope IN ('general', 'folder')),
    folder_id        INTEGER REFERENCES folder(id) ON DELETE CASCADE,
    relative_path    TEXT NOT NULL,
    canonical_path   TEXT NOT NULL,
    volume_id        TEXT,
    file_id          TEXT,
    managed_copy     INTEGER NOT NULL DEFAULT 0 CHECK (managed_copy IN (0, 1)),
    mime_type        TEXT,
    size_bytes       INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_at      TEXT NOT NULL,
    content_hash     TEXT NOT NULL,
    indexing_status  TEXT NOT NULL DEFAULT 'pending',
    indexed_at       TEXT,
    last_error       TEXT,
    CHECK (
        (scope = 'general' AND folder_id IS NULL AND managed_copy = 0)
        OR (scope = 'folder' AND folder_id IS NOT NULL AND managed_copy = 1)
    )
) STRICT;

INSERT INTO document(
    id,scope,folder_id,relative_path,canonical_path,managed_copy,mime_type,size_bytes,
    modified_at,content_hash,indexing_status,indexed_at,last_error
)
SELECT
    document_old.id,'folder',document_old.folder_id,document_old.relative_path,
    folder.canonical_path || '/' || document_old.relative_path,1,document_old.mime_type,
    document_old.size_bytes,document_old.modified_at,document_old.content_hash,
    document_old.indexing_status,document_old.indexed_at,document_old.last_error
FROM document_old
JOIN folder ON folder.id = document_old.folder_id;

CREATE UNIQUE INDEX uq_document_general_hash
    ON document(content_hash) WHERE scope = 'general';
CREATE UNIQUE INDEX uq_document_folder_hash
    ON document(folder_id,content_hash) WHERE scope = 'folder';
CREATE UNIQUE INDEX uq_document_folder_path
    ON document(folder_id,relative_path) WHERE scope = 'folder';
CREATE INDEX idx_document_folder ON document(folder_id);
CREATE INDEX idx_document_file_identity ON document(volume_id,file_id);

CREATE TABLE knowledge_item (
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
    )
) STRICT;

INSERT INTO knowledge_item SELECT * FROM knowledge_item_old;

DROP TABLE knowledge_item_old;
DROP TABLE document_old;

CREATE INDEX idx_knowledge_item_origin ON knowledge_item(origin_id);
CREATE INDEX idx_knowledge_item_document ON knowledge_item(document_id);
CREATE INDEX idx_knowledge_item_scope_folder ON knowledge_item(scope,folder_id);
CREATE INDEX idx_knowledge_item_hash ON knowledge_item(content_hash);

CREATE TRIGGER delete_knowledge_vector
AFTER DELETE ON knowledge_item
BEGIN
    DELETE FROM knowledge_vector WHERE knowledge_id = OLD.id;
END;

PRAGMA user_version = 3;
COMMIT;
PRAGMA foreign_keys = ON;
