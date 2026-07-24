BEGIN IMMEDIATE;

DROP TABLE IF EXISTS folder_extension;

UPDATE folder SET recursive_scan = 1;

PRAGMA user_version = 7;
COMMIT;
