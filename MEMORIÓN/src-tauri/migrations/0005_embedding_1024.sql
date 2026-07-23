BEGIN IMMEDIATE;
DROP TRIGGER IF EXISTS delete_knowledge_vector;
DROP TRIGGER IF EXISTS protect_embedding_model;
DROP TABLE IF EXISTS knowledge_vector;
CREATE VIRTUAL TABLE knowledge_vector USING vec0(
    knowledge_id INTEGER PRIMARY KEY,
    embedding FLOAT[1024] distance_metric=cosine,
    embedding_model_id INTEGER,
    scope TEXT,
    folder_id INTEGER
);
CREATE TRIGGER delete_knowledge_vector AFTER DELETE ON knowledge_item BEGIN
    DELETE FROM knowledge_vector WHERE knowledge_id = OLD.id;
END;
CREATE TRIGGER protect_embedding_model BEFORE DELETE ON ai_model
WHEN EXISTS (SELECT 1 FROM knowledge_vector WHERE embedding_model_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'model is used by knowledge vectors');
END;
PRAGMA user_version = 5;
COMMIT;
