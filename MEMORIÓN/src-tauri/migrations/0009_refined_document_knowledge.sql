BEGIN IMMEDIATE;

/*
 * Los registros document_chunk anteriores contenían texto fuente sin una
 * segunda revisión semántica. Se retiran y sus documentos se dejan pendientes
 * para que el escaneo de arranque genere conocimientos refinados.
 */
DELETE FROM knowledge_item WHERE source_type = 'document_chunk';

UPDATE document
SET indexing_status = 'pending',
    indexed_at = NULL,
    last_error = NULL
WHERE scope = 'folder' AND indexing_status = 'completed';

UPDATE folder
SET scan_status = 'pending',
    last_error = NULL
WHERE EXISTS (
    SELECT 1 FROM document
    WHERE document.folder_id = folder.id
      AND document.indexing_status = 'pending'
);

PRAGMA user_version = 9;
COMMIT;
