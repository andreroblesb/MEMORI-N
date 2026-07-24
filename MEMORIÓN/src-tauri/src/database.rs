use rusqlite::{ffi::sqlite3_auto_extension, Connection, OpenFlags};
use serde::Serialize;
use sqlite_vec::sqlite3_vec_init;
use std::{
    path::PathBuf,
    sync::{Mutex, Once},
};
use tauri::{AppHandle, Manager};

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");
const VECTOR_INTEGRITY_MIGRATION: &str = include_str!("../migrations/0002_vector_integrity.sql");
const DOCUMENT_SOURCES_MIGRATION: &str = include_str!("../migrations/0003_document_sources.sql");
const SESSION_MESSAGES_MIGRATION: &str = include_str!("../migrations/0004_session_messages.sql");
const EMBEDDING_1024_MIGRATION: &str = include_str!("../migrations/0005_embedding_1024.sql");
const FOLDER_INDEXING_MIGRATION: &str = include_str!("../migrations/0006_folder_indexing.sql");
const FIXED_DOCUMENT_FORMATS_MIGRATION: &str = include_str!("../migrations/0007_fixed_document_formats.sql");
pub const EMBEDDING_DIMENSIONS: u32 = 1024;

static REGISTER_SQLITE_VEC: Once = Once::new();

pub struct Database {
    connection: Mutex<Connection>,
    path: PathBuf,
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Ok(connection) = self.connection.get_mut() {
            let _ = connection.execute("DELETE FROM session_message", []);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatus {
    pub path: String,
    pub schema_version: u32,
    pub sqlite_version: String,
    pub sqlite_vec_version: String,
    pub embedding_dimensions: u32,
}

impl Database {
    pub fn open(app: &AppHandle) -> Result<Self, String> {
        let data_dir = app
            .path()
            .local_data_dir()
            .map_err(|error| format!("No fue posible resolver el directorio de datos: {error}"))?;
        let data_dir = data_dir.join("MEMORIÓN").join("data");
        std::fs::create_dir_all(&data_dir)
            .map_err(|error| format!("No fue posible crear el directorio de datos: {error}"))?;
        let database_path = data_dir.join("memorion.sqlite3");
        eprintln!("SQLite de MEMORIÓN: {}", database_path.display());
        Self::open_path(database_path)
    }

    fn open_path(path: PathBuf) -> Result<Self, String> {
        register_sqlite_vec();
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("No fue posible abrir SQLite: {error}"))?;

        configure_connection(&connection)?;
        migrate(&connection)?;
        verify_database(&connection)?;
        connection
            .execute("DELETE FROM session_message", [])
            .map_err(|error| format!("No fue posible limpiar el historial de sesión: {error}"))?;

        Ok(Self {
            connection: Mutex::new(connection),
            path,
        })
    }

    pub fn status(&self) -> Result<DatabaseStatus, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "No fue posible bloquear la conexión SQLite".to_string())?;
        let (schema_version, sqlite_version, sqlite_vec_version) = connection
            .query_row(
                "SELECT (SELECT user_version FROM pragma_user_version),
                        sqlite_version(),
                        vec_version()",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| format!("No fue posible consultar el estado de SQLite: {error}"))?;

        Ok(DatabaseStatus {
            path: self.path.to_string_lossy().into_owned(),
            schema_version,
            sqlite_version,
            sqlite_vec_version,
            embedding_dimensions: EMBEDDING_DIMENSIONS,
        })
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "No fue posible bloquear la conexión SQLite".to_string())
    }
}

fn register_sqlite_vec() {
    REGISTER_SQLITE_VEC.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|error| format!("No fue posible configurar SQLite: {error}"))
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("No fue posible leer la versión del esquema: {error}"))?;

    let migrations_result = match version {
        0 => {
            connection
                .execute_batch(INITIAL_MIGRATION)
                .map_err(|error| format!("Falló la migración inicial: {error}"))?;
            connection
                .execute_batch(VECTOR_INTEGRITY_MIGRATION)
                .map_err(|error| format!("Falló la migración de integridad vectorial: {error}"))?;
            connection
                .execute_batch(DOCUMENT_SOURCES_MIGRATION)
                .map_err(|error| format!("Falló la migración de fuentes documentales: {error}"))
        }
        1 => {
            connection
                .execute_batch(VECTOR_INTEGRITY_MIGRATION)
                .map_err(|error| format!("Falló la migración de integridad vectorial: {error}"))?;
            connection
                .execute_batch(DOCUMENT_SOURCES_MIGRATION)
                .map_err(|error| format!("Falló la migración de fuentes documentales: {error}"))
        }
        2 => connection
            .execute_batch(DOCUMENT_SOURCES_MIGRATION)
            .map_err(|error| format!("Falló la migración de fuentes documentales: {error}")),
        3 | 4 | 5 | 6 | 7 => Ok(()),
        other => Err(format!(
            "La base de datos usa una versión de esquema no compatible: {other}"
        )),
    };
    migrations_result?;
    if version < 4 {
        connection
            .execute_batch(SESSION_MESSAGES_MIGRATION)
            .map_err(|error| format!("Falló la migración del historial de sesión: {error}"))?;
    }
    if version < 5 {
        connection
            .execute_batch(EMBEDDING_1024_MIGRATION)
            .map_err(|error| format!("Falló la migración de vectores a 1024: {error}"))?;
    }
    if version < 6 {
        connection
            .execute_batch(FOLDER_INDEXING_MIGRATION)
            .map_err(|error| format!("Falló la migración de configuración de carpetas: {error}"))?;
    }
    if version < 7 {
        connection
            .execute_batch(FIXED_DOCUMENT_FORMATS_MIGRATION)
            .map_err(|error| format!("Falló la migración de formatos documentales: {error}"))?;
    }
    Ok(())
}

fn verify_database(connection: &Connection) -> Result<(), String> {
    let foreign_keys: u8 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| format!("No fue posible verificar claves foráneas: {error}"))?;
    if foreign_keys != 1 {
        return Err("SQLite abrió sin claves foráneas habilitadas".into());
    }

    let _: String = connection
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .map_err(|error| format!("sqlite-vec no está disponible: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn temporary_database(label: &str) -> (Database, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "memorion-{label}-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos()
        ));
        let database = Database::open_path(path.clone()).expect("database should initialize");
        (database, path)
    }

    fn remove_database_files(path: PathBuf) {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
    }

    #[test]
    fn initial_schema_and_vec_extension_are_available() {
        let (database, path) = temporary_database("schema-test");
        let status = database.status().expect("status should be readable");
        assert_eq!(status.schema_version, 7);
        assert_eq!(status.embedding_dimensions, 1024);

        drop(database);
        remove_database_files(path);
    }

    #[test]
    fn relational_crud_vector_search_and_cascade_work() {
        let (database, path) = temporary_database("crud-test");
        {
            let connection = database.connection().expect("connection should lock");
            connection
                .execute(
                    "INSERT INTO folder(name,canonical_path) VALUES('Proyecto','C:/Proyecto')",
                    [],
                )
                .expect("folder should insert");
            let folder_id = connection.last_insert_rowid();
            connection
                .execute(
                    "UPDATE folder SET name='Proyecto actualizado' WHERE id=?1",
                    [folder_id],
                )
                .expect("folder should update");

            connection.execute(
                "INSERT INTO document(scope,folder_id,relative_path,canonical_path,managed_copy,
                 size_bytes,modified_at,content_hash)
                 VALUES('folder',?1,'README.md','C:/Proyecto/README.md',1,120,
                 '2026-07-22T00:00:00Z','doc-hash')",
                [folder_id],
            ).expect("document should insert");
            let document_id = connection.last_insert_rowid();

            connection
                .execute(
                    "INSERT INTO knowledge_origin(scope,folder_id,user_input)
                 VALUES('folder',?1,'¿Qué contiene este proyecto?')",
                    [folder_id],
                )
                .expect("origin should insert");
            let origin_id = connection.last_insert_rowid();

            connection
                .execute(
                    "INSERT INTO ai_model(provider,model_key,display_name)
                 VALUES('ollama','nomic-embed-text','Nomic Embed Text')",
                    [],
                )
                .expect("model should insert");
            let model_id = connection.last_insert_rowid();
            connection.execute(
                "INSERT INTO model_capability(model_id,capability,embedding_dimensions,distance_metric)
                 VALUES(?1,'embedding',768,'cosine')",
                [model_id],
            ).expect("capability should insert");
            connection
                .execute(
                    "INSERT INTO model_assignment(model_id,task) VALUES(?1,'knowledge_embedding')",
                    [model_id],
                )
                .expect("assignment should insert");

            connection.execute(
                "INSERT INTO knowledge_item(origin_id,document_id,folder_id,scope,source_type,
                 content,content_hash,is_confirmed)
                 VALUES(?1,?2,?3,'folder','derived_knowledge','MEMORIÓN usa Tauri.','knowledge-hash',1)",
                params![origin_id, document_id, folder_id],
            ).expect("knowledge should insert");
            let knowledge_id = connection.last_insert_rowid();

            let mut embedding = vec![0.0_f32; EMBEDDING_DIMENSIONS as usize];
            embedding[0] = 1.0;
            let embedding_json =
                serde_json::to_string(&embedding).expect("embedding should serialize");
            connection.execute(
                "INSERT INTO knowledge_vector(knowledge_id,embedding,embedding_model_id,scope,folder_id)
                 VALUES(?1,vec_f32(?2),?3,'folder',?4)",
                params![knowledge_id, embedding_json, model_id, folder_id],
            ).expect("vector should insert");

            let nearest: i64 = connection
                .query_row(
                    "SELECT knowledge_id FROM knowledge_vector
                 WHERE embedding MATCH vec_f32(?1) AND k=1 AND scope='folder' AND folder_id=?2",
                    params![embedding_json, folder_id],
                    |row| row.get(0),
                )
                .expect("vector search should work");
            assert_eq!(nearest, knowledge_id);

            connection
                .execute("DELETE FROM folder WHERE id=?1", [folder_id])
                .expect("folder should delete");
            let remaining_vectors: i64 = connection
                .query_row(
                    "SELECT count(*) FROM knowledge_vector WHERE knowledge_id=?1",
                    [knowledge_id],
                    |row| row.get(0),
                )
                .expect("vector count should be readable");
            assert_eq!(remaining_vectors, 0);

            connection
                .execute("DELETE FROM ai_model WHERE id=?1", [model_id])
                .expect("unused model should delete");
        }
        drop(database);
        remove_database_files(path);
    }
}
mod crud;

pub use crud::*;
