/*
 * MEMORIÓN — Nuke de datos de chats
 *
 * ELIMINA:
 *   - historial temporal de todos los chats;
 *   - vectores y knowledge items;
 *   - orígenes de conocimiento;
 *   - documentos registrados;
 *   - configuración de formatos por carpeta;
 *   - chats de carpeta.
 *
 * CONSERVA:
 *   - ai_model;
 *   - model_capability;
 *   - model_assignment;
 *   - los archivos físicos de las carpetas;
 *   - los modelos GGUF y sus metadata.
 *
 * IMPORTANTE:
 * Debe ejecutarse desde una conexión que tenga sqlite-vec cargado.
 * La transacción completa se revierte si cualquier DELETE falla.
 */

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM session_message;

/*
 * Vaciar primero la tabla virtual también elimina cualquier vector huérfano.
 * El trigger de knowledge_item intentará eliminar de nuevo cada vector;
 * esos DELETE adicionales son seguros.
 */
DELETE FROM knowledge_vector;
DELETE FROM knowledge_item;

DELETE FROM knowledge_origin;
DELETE FROM document;

/*
 * folder_extension se elimina por ON DELETE CASCADE, pero el DELETE explícito
 * mantiene el objetivo del script claro y cubre bases reparadas manualmente.
 */
DELETE FROM folder_extension;
DELETE FROM folder;

COMMIT;

/*
 * Comprobación esperada: todos los valores deben ser cero.
 * Las tres tablas de modelos no se incluyen porque deben conservarse.
 */
SELECT 'session_message' AS table_name, COUNT(*) AS remaining FROM session_message
UNION ALL
SELECT 'knowledge_vector', COUNT(*) FROM knowledge_vector
UNION ALL
SELECT 'knowledge_item', COUNT(*) FROM knowledge_item
UNION ALL
SELECT 'knowledge_origin', COUNT(*) FROM knowledge_origin
UNION ALL
SELECT 'document', COUNT(*) FROM document
UNION ALL
SELECT 'folder_extension', COUNT(*) FROM folder_extension
UNION ALL
SELECT 'folder', COUNT(*) FROM folder;
