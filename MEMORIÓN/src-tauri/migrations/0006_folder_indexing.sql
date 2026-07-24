BEGIN IMMEDIATE;

ALTER TABLE folder
ADD COLUMN recursive_scan INTEGER NOT NULL DEFAULT 1 CHECK (recursive_scan IN (0, 1));

CREATE TABLE folder_extension (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER NOT NULL REFERENCES folder(id) ON DELETE CASCADE,
    extension   TEXT NOT NULL CHECK (
        length(extension) > 0
        AND extension = lower(extension)
        AND instr(extension, '.') = 0
    ),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(folder_id, extension)
) STRICT;

INSERT INTO folder_extension(folder_id, extension)
SELECT folder.id, defaults.extension
FROM folder
CROSS JOIN (
    SELECT 'pdf' AS extension
    UNION ALL SELECT 'txt'
    UNION ALL SELECT 'md'
    UNION ALL SELECT 'docx'
    UNION ALL SELECT 'csv'
    UNION ALL SELECT 'json'
) AS defaults;

CREATE INDEX idx_folder_extension_folder ON folder_extension(folder_id);

PRAGMA user_version = 6;
COMMIT;
