BEGIN IMMEDIATE;

CREATE TABLE folder_extension (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER NOT NULL REFERENCES folder(id) ON DELETE CASCADE,
    extension   TEXT NOT NULL CHECK (
        extension IN ('pdf', 'docx', 'json', 'md', 'txt', 'pptx', 'rtf', 'xml')
    ),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(folder_id, extension)
) STRICT;

INSERT INTO folder_extension(folder_id, extension)
SELECT folder.id, formats.extension
FROM folder
CROSS JOIN (
    SELECT 'pdf' AS extension
    UNION ALL SELECT 'docx'
    UNION ALL SELECT 'json'
    UNION ALL SELECT 'md'
    UNION ALL SELECT 'txt'
    UNION ALL SELECT 'pptx'
    UNION ALL SELECT 'rtf'
    UNION ALL SELECT 'xml'
) AS formats;

CREATE INDEX idx_folder_extension_folder ON folder_extension(folder_id);

PRAGMA user_version = 8;
COMMIT;
