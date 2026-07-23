BEGIN IMMEDIATE;

CREATE TABLE session_message (
    id          INTEGER PRIMARY KEY,
    scope       TEXT NOT NULL CHECK (scope IN ('general', 'folder')),
    folder_id   INTEGER REFERENCES folder(id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT NOT NULL CHECK (length(trim(content)) > 0),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (scope = 'general' AND folder_id IS NULL)
        OR (scope = 'folder' AND folder_id IS NOT NULL)
    )
) STRICT;

CREATE INDEX idx_session_message_chat ON session_message(scope, folder_id, id);

PRAGMA user_version = 4;
COMMIT;
