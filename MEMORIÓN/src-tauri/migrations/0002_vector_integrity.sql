BEGIN IMMEDIATE;

CREATE TRIGGER IF NOT EXISTS delete_knowledge_vector
AFTER DELETE ON knowledge_item
BEGIN
    DELETE FROM knowledge_vector WHERE knowledge_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS protect_embedding_model
BEFORE DELETE ON ai_model
WHEN EXISTS (
    SELECT 1 FROM knowledge_vector
    WHERE embedding_model_id = OLD.id
)
BEGIN
    SELECT RAISE(ABORT, 'model is used by knowledge vectors');
END;

PRAGMA user_version = 2;

COMMIT;
