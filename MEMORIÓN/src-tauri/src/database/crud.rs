use super::{Database, EMBEDDING_DIMENSIONS};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::State;

type CrudResult<T> = Result<T, String>;

fn db_error(context: &str, error: rusqlite::Error) -> String {
    format!("{context}: {error}")
}

fn required_text(value: &str, field: &str) -> CrudResult<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} no puede estar vacío"))
    } else {
        Ok(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: i64,
    pub scope: String,
    pub folder_id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

fn session_message_from_row(row: &Row<'_>) -> rusqlite::Result<SessionMessage> {
    Ok(SessionMessage {
        id: row.get(0)?,
        scope: row.get(1)?,
        folder_id: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[tauri::command]
pub fn list_session_messages(
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<Vec<SessionMessage>> {
    let scope = if folder_id.is_some() {
        "folder"
    } else {
        "general"
    };
    let connection = database.connection()?;
    let mut statement = connection
        .prepare(
            "SELECT id,scope,folder_id,role,content,created_at
             FROM session_message
             WHERE scope=?1 AND folder_id IS ?2
             ORDER BY id",
        )
        .map_err(|error| db_error("No fue posible preparar el historial de sesión", error))?;
    let result = statement
        .query_map(params![scope, folder_id], session_message_from_row)
        .map_err(|error| db_error("No fue posible consultar el historial de sesión", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer el historial de sesión", error));
    result
}

#[tauri::command]
pub fn append_session_message(
    folder_id: Option<i64>,
    role: String,
    content: String,
    database: State<'_, Database>,
) -> CrudResult<SessionMessage> {
    let role = required_text(&role, "role")?;
    if role != "user" && role != "assistant" {
        return Err("role debe ser user o assistant".into());
    }
    let content = required_text(&content, "content")?;
    let scope = if folder_id.is_some() {
        "folder"
    } else {
        "general"
    };
    let connection = database.connection()?;
    connection
        .execute(
            "INSERT INTO session_message(scope,folder_id,role,content)
             VALUES(?1,?2,?3,?4)",
            params![scope, folder_id, role, content],
        )
        .map_err(|error| db_error("No fue posible guardar el mensaje de sesión", error))?;
    connection
        .query_row(
            "SELECT id,scope,folder_id,role,content,created_at
             FROM session_message WHERE id=?1",
            [connection.last_insert_rowid()],
            session_message_from_row,
        )
        .map_err(|error| db_error("No fue posible recuperar el mensaje de sesión", error))
}

#[tauri::command]
pub fn clear_session_messages(
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<usize> {
    let scope = if folder_id.is_some() {
        "folder"
    } else {
        "general"
    };
    database
        .connection()?
        .execute(
            "DELETE FROM session_message WHERE scope=?1 AND folder_id IS ?2",
            params![scope, folder_id],
        )
        .map_err(|error| db_error("No fue posible limpiar el historial de sesión", error))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityMetrics {
    pub folder_chat_count: i64,
    pub session_message_count: i64,
    pub session_text_bytes: i64,
    pub mapped_file_count: u64,
    pub mapped_directory_count: u64,
    pub mapped_folder_bytes: u64,
    pub inaccessible_entry_count: u64,
}

fn measure_folder_tree(paths: Vec<PathBuf>) -> (u64, u64, u64, u64) {
    let mut directory_count = paths.len() as u64;
    let mut stack = paths;
    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;
    let mut inaccessible = 0_u64;
    while let Some(path) = stack.pop() {
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    inaccessible += 1;
                    continue;
                }
            };
            let metadata = match entry.path().symlink_metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    inaccessible += 1;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                directory_count += 1;
                stack.push(entry.path());
            } else if metadata.is_file() {
                file_count += 1;
                total_bytes = total_bytes.saturating_add(metadata.len());
            }
        }
    }
    (file_count, directory_count, total_bytes, inaccessible)
}

#[tauri::command]
pub async fn get_activity_metrics(database: State<'_, Database>) -> CrudResult<ActivityMetrics> {
    let (folder_chat_count, session_message_count, session_text_bytes, paths) = {
        let connection = database.connection()?;
        let folder_chat_count = connection
            .query_row("SELECT count(*) FROM folder", [], |row| row.get(0))
            .map_err(|error| db_error("No fue posible contar los chats", error))?;
        let (session_message_count, session_text_bytes) = connection
            .query_row(
                "SELECT count(*),coalesce(sum(length(CAST(content AS BLOB))),0)
                 FROM session_message",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| db_error("No fue posible medir la sesión", error))?;
        let mut statement = connection
            .prepare("SELECT canonical_path FROM folder")
            .map_err(|error| db_error("No fue posible consultar carpetas mapeadas", error))?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| db_error("No fue posible recorrer carpetas mapeadas", error))?
            .filter_map(Result::ok)
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        (
            folder_chat_count,
            session_message_count,
            session_text_bytes,
            paths,
        )
    };
    let (mapped_file_count, mapped_directory_count, mapped_folder_bytes, inaccessible_entry_count) =
        tauri::async_runtime::spawn_blocking(move || measure_folder_tree(paths))
            .await
            .map_err(|error| format!("No fue posible medir las carpetas: {error}"))?;
    Ok(ActivityMetrics {
        folder_chat_count,
        session_message_count,
        session_text_bytes,
        mapped_file_count,
        mapped_directory_count,
        mapped_folder_bytes,
        inaccessible_entry_count,
    })
}

fn validate_scope(scope: &str, folder_id: Option<i64>) -> CrudResult<()> {
    match (scope, folder_id) {
        ("general", None) | ("folder", Some(_)) => Ok(()),
        ("general", Some(_)) => Err("El alcance general no admite folderId".into()),
        ("folder", None) => Err("El alcance folder requiere folderId".into()),
        _ => Err("scope debe ser general o folder".into()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub canonical_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_scanned_at: Option<String>,
    pub scan_status: String,
    pub last_error: Option<String>,
    pub extensions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFolder {
    pub name: String,
    pub canonical_path: String,
    pub extensions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFolder {
    pub id: i64,
    pub name: String,
    pub canonical_path: String,
    pub last_scanned_at: Option<String>,
    pub scan_status: String,
    pub last_error: Option<String>,
    pub extensions: Vec<String>,
}

const SUPPORTED_DOCUMENT_EXTENSIONS: [&str; 8] =
    ["pdf", "docx", "json", "md", "txt", "pptx", "rtf", "xml"];

fn validate_folder_extensions(mut extensions: Vec<String>) -> CrudResult<Vec<String>> {
    extensions.iter_mut().for_each(|value| {
        *value = value.trim().trim_start_matches('.').to_ascii_lowercase();
    });
    extensions.sort();
    extensions.dedup();
    if extensions.is_empty() {
        return Err("Debe permanecer habilitado al menos un formato".into());
    }
    if extensions.iter().any(|value| !SUPPORTED_DOCUMENT_EXTENSIONS.contains(&value.as_str())) {
        return Err("La selección contiene un formato no admitido".into());
    }
    Ok(extensions)
}

fn get_folder_extensions(
    connection: &rusqlite::Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT extension FROM folder_extension WHERE folder_id=?1 ORDER BY extension",
    )?;
    let result = statement.query_map([folder_id], |row| row.get(0))?.collect();
    result
}

fn folder_from_row(connection: &rusqlite::Connection, row: &Row<'_>) -> rusqlite::Result<Folder> {
    let id = row.get(0)?;
    Ok(Folder {
        id,
        name: row.get(1)?,
        canonical_path: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        last_scanned_at: row.get(5)?,
        scan_status: row.get(6)?,
        last_error: row.get(7)?,
        extensions: get_folder_extensions(connection, id)?,
    })
}

fn get_folder_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<Folder> {
    connection
        .query_row(
            "SELECT id,name,canonical_path,created_at,updated_at,last_scanned_at,scan_status,last_error
             FROM folder WHERE id=?1",
            [id],
            |row| folder_from_row(connection, row),
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar la carpeta", error))?
        .ok_or_else(|| format!("No existe la carpeta {id}"))
}

#[tauri::command]
pub fn create_folder(input: CreateFolder, database: State<'_, Database>) -> CrudResult<Folder> {
    let name = required_text(&input.name, "name")?;
    let path = required_text(&input.canonical_path, "canonicalPath")?;
    let extensions = validate_folder_extensions(input.extensions)?;
    if !std::path::Path::new(&path).is_dir() {
        return Err("La ruta seleccionada no existe o no es una carpeta".into());
    }
    let mut connection = database.connection()?;
    let transaction = connection.transaction()
        .map_err(|error| db_error("No fue posible iniciar la carpeta", error))?;
    transaction
        .execute(
            "INSERT INTO folder(name,canonical_path,recursive_scan) VALUES(?1,?2,1)",
            params![name, path],
        )
        .map_err(|error| db_error("No fue posible crear la carpeta", error))?;
    let id = transaction.last_insert_rowid();
    for extension in extensions {
        transaction.execute(
            "INSERT INTO folder_extension(folder_id,extension) VALUES(?1,?2)",
            params![id, extension],
        ).map_err(|error| db_error("No fue posible guardar el formato", error))?;
    }
    transaction.commit().map_err(|error| db_error("No fue posible confirmar la carpeta", error))?;
    get_folder_conn(&connection, id)
}

#[tauri::command]
pub fn get_folder(id: i64, database: State<'_, Database>) -> CrudResult<Folder> {
    let connection = database.connection()?;
    get_folder_conn(&connection, id)
}

#[tauri::command]
pub fn list_folders(database: State<'_, Database>) -> CrudResult<Vec<Folder>> {
    let connection = database.connection()?;
    let mut statement = connection
        .prepare(
            "SELECT id,name,canonical_path,created_at,updated_at,last_scanned_at,scan_status,last_error
             FROM folder ORDER BY name COLLATE NOCASE",
        )
        .map_err(|error| db_error("No fue posible preparar la consulta de carpetas", error))?;
    let result = statement
        .query_map([], |row| folder_from_row(&connection, row))
        .map_err(|error| db_error("No fue posible listar carpetas", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer las carpetas", error));
    result
}

#[tauri::command]
pub fn update_folder(input: UpdateFolder, database: State<'_, Database>) -> CrudResult<Folder> {
    let name = required_text(&input.name, "name")?;
    let path = required_text(&input.canonical_path, "canonicalPath")?;
    let status = required_text(&input.scan_status, "scanStatus")?;
    let extensions = validate_folder_extensions(input.extensions)?;
    let mut connection = database.connection()?;
    let transaction = connection.transaction()
        .map_err(|error| db_error("No fue posible iniciar la actualización", error))?;
    let changed = transaction
        .execute(
            "UPDATE folder SET name=?1,canonical_path=?2,last_scanned_at=?3,scan_status=?4,
             last_error=?5,recursive_scan=1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?6",
            params![
                name,
                path,
                input.last_scanned_at,
                status,
                input.last_error,
                input.id
            ],
        )
        .map_err(|error| db_error("No fue posible actualizar la carpeta", error))?;
    if changed == 0 {
        return Err(format!("No existe la carpeta {}", input.id));
    }
    transaction.execute("DELETE FROM folder_extension WHERE folder_id=?1", [input.id])
        .map_err(|error| db_error("No fue posible reemplazar formatos", error))?;
    for extension in extensions {
        transaction.execute(
            "INSERT INTO folder_extension(folder_id,extension) VALUES(?1,?2)",
            params![input.id, extension],
        ).map_err(|error| db_error("No fue posible guardar el formato", error))?;
    }
    transaction.commit().map_err(|error| db_error("No fue posible confirmar los formatos", error))?;
    get_folder_conn(&connection, input.id)
}

#[tauri::command]
pub fn delete_folder(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM folder WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar la carpeta", error))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: i64,
    pub scope: String,
    pub folder_id: Option<i64>,
    pub relative_path: String,
    pub canonical_path: String,
    pub volume_id: Option<String>,
    pub file_id: Option<String>,
    pub managed_copy: bool,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub modified_at: String,
    pub content_hash: String,
    pub indexing_status: String,
    pub indexed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDocument {
    pub id: Option<i64>,
    pub scope: String,
    pub folder_id: Option<i64>,
    pub relative_path: String,
    pub canonical_path: String,
    pub volume_id: Option<String>,
    pub file_id: Option<String>,
    pub managed_copy: bool,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub modified_at: String,
    pub content_hash: String,
    pub indexing_status: String,
    pub indexed_at: Option<String>,
    pub last_error: Option<String>,
}

fn document_from_row(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        scope: row.get(1)?,
        folder_id: row.get(2)?,
        relative_path: row.get(3)?,
        canonical_path: row.get(4)?,
        volume_id: row.get(5)?,
        file_id: row.get(6)?,
        managed_copy: row.get::<_, i64>(7)? != 0,
        mime_type: row.get(8)?,
        size_bytes: row.get(9)?,
        modified_at: row.get(10)?,
        content_hash: row.get(11)?,
        indexing_status: row.get(12)?,
        indexed_at: row.get(13)?,
        last_error: row.get(14)?,
    })
}

fn get_document_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<Document> {
    connection
        .query_row(
            "SELECT id,scope,folder_id,relative_path,canonical_path,volume_id,file_id,managed_copy,
             mime_type,size_bytes,modified_at,content_hash,indexing_status,indexed_at,last_error
             FROM document WHERE id=?1",
            [id],
            document_from_row,
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar el documento", error))?
        .ok_or_else(|| format!("No existe el documento {id}"))
}

#[tauri::command]
pub fn create_document(input: SaveDocument, database: State<'_, Database>) -> CrudResult<Document> {
    if input.size_bytes < 0 {
        return Err("sizeBytes no puede ser negativo".into());
    }
    validate_scope(&input.scope, input.folder_id)?;
    let path = required_text(&input.relative_path, "relativePath")?;
    let modified = required_text(&input.modified_at, "modifiedAt")?;
    let hash = required_text(&input.content_hash, "contentHash")?;
    let status = required_text(&input.indexing_status, "indexingStatus")?;
    let connection = database.connection()?;
    connection
        .execute(
            "INSERT INTO document(scope,folder_id,relative_path,canonical_path,volume_id,file_id,
             managed_copy,mime_type,size_bytes,modified_at,content_hash,indexing_status,indexed_at,last_error)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![input.scope,input.folder_id,path,required_text(&input.canonical_path,"canonicalPath")?,
            input.volume_id,input.file_id,input.managed_copy as i64,input.mime_type,input.size_bytes,
            modified,hash,status,input.indexed_at,input.last_error],
        )
        .map_err(|error| db_error("No fue posible crear el documento", error))?;
    get_document_conn(&connection, connection.last_insert_rowid())
}

#[tauri::command]
pub fn get_document(id: i64, database: State<'_, Database>) -> CrudResult<Document> {
    let connection = database.connection()?;
    get_document_conn(&connection, id)
}

#[tauri::command]
pub fn list_documents(
    scope: Option<String>,
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<Vec<Document>> {
    let connection = database.connection()?;
    let mut statement = connection
        .prepare(
            "SELECT id,scope,folder_id,relative_path,canonical_path,volume_id,file_id,managed_copy,
             mime_type,size_bytes,modified_at,content_hash,indexing_status,indexed_at,last_error
             FROM document WHERE (?1 IS NULL OR scope=?1) AND
             (?2 IS NULL OR folder_id=?2) ORDER BY relative_path",
        )
        .map_err(|error| db_error("No fue posible preparar la consulta de documentos", error))?;
    let result = statement
        .query_map(params![scope, folder_id], document_from_row)
        .map_err(|error| db_error("No fue posible listar documentos", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer documentos", error));
    result
}

#[tauri::command]
pub fn update_document(input: SaveDocument, database: State<'_, Database>) -> CrudResult<Document> {
    let id = input.id.ok_or("id es obligatorio para actualizar")?;
    if input.size_bytes < 0 {
        return Err("sizeBytes no puede ser negativo".into());
    }
    validate_scope(&input.scope, input.folder_id)?;
    let connection = database.connection()?;
    let changed = connection.execute(
        "UPDATE document SET scope=?1,folder_id=?2,relative_path=?3,canonical_path=?4,volume_id=?5,
         file_id=?6,managed_copy=?7,mime_type=?8,size_bytes=?9,modified_at=?10,content_hash=?11,
         indexing_status=?12,indexed_at=?13,last_error=?14 WHERE id=?15",
        params![input.scope,input.folder_id,required_text(&input.relative_path,"relativePath")?,
        required_text(&input.canonical_path,"canonicalPath")?,input.volume_id,input.file_id,input.managed_copy as i64,
        input.mime_type,input.size_bytes,required_text(&input.modified_at,"modifiedAt")?,
        required_text(&input.content_hash,"contentHash")?,required_text(&input.indexing_status,"indexingStatus")?,
        input.indexed_at,input.last_error,id],
    ).map_err(|error| db_error("No fue posible actualizar el documento", error))?;
    if changed == 0 {
        return Err(format!("No existe el documento {id}"));
    }
    get_document_conn(&connection, id)
}

#[tauri::command]
pub fn delete_document(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM document WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar el documento", error))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCandidate {
    pub document_id: i64,
    pub canonical_path: String,
    pub relative_path: String,
    pub extension: String,
}

fn collect_scan_paths(
    root: &Path,
    recursive: bool,
    extensions: &[&str],
) -> CrudResult<Vec<PathBuf>> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("No fue posible leer {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("No fue posible leer una entrada: {error}"))?;
            let path = entry.path();
            let metadata = path.symlink_metadata()
                .map_err(|error| format!("No fue posible leer {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if recursive {
                    directories.push(path);
                }
                continue;
            }
            let extension = path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            if metadata.is_file() && extension.as_ref().is_some_and(|value| extensions.contains(&value.as_str())) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[tauri::command]
pub fn prepare_folder_scan(
    folder_id: i64,
    database: State<'_, Database>,
) -> CrudResult<Vec<ScanCandidate>> {
    let connection = database.connection()?;
    let folder = get_folder_conn(&connection, folder_id)?;
    connection.execute(
        "UPDATE folder SET scan_status='scanning',last_error=NULL WHERE id=?1",
        [folder_id],
    ).map_err(|error| db_error("No fue posible iniciar el escaneo", error))?;
    let root = std::fs::canonicalize(&folder.canonical_path)
        .map_err(|error| format!("No fue posible resolver la carpeta: {error}"))?;
    let extensions = folder.extensions.iter().map(String::as_str).collect::<Vec<_>>();
    let paths = match collect_scan_paths(&root, true, &extensions) {
        Ok(paths) => paths,
        Err(error) => {
            let _ = connection.execute(
                "UPDATE folder SET scan_status='failed',last_error=?1 WHERE id=?2",
                params![error, folder_id],
            );
            return Err(error);
        }
    };
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for path in paths {
        let canonical = std::fs::canonicalize(&path)
            .map_err(|error| format!("No fue posible resolver {}: {error}", path.display()))?;
        let canonical_text = canonical.to_string_lossy().into_owned();
        seen.insert(canonical_text.clone());
        let relative = canonical.strip_prefix(&root)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = std::fs::metadata(&canonical)
            .map_err(|error| format!("No fue posible leer {}: {error}", canonical.display()))?;
        let modified = metadata.modified().ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| format!("unix:{}", value.as_secs()))
            .unwrap_or_else(|| "unknown".into());
        let existing: Option<(i64, String, String, i64, String)> = connection.query_row(
            "SELECT id,content_hash,indexing_status,size_bytes,modified_at FROM document
             WHERE scope='folder' AND folder_id=?1 AND relative_path=?2",
            params![folder_id, relative],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional().map_err(|error| db_error("No fue posible consultar el documento", error))?;
        let (document_id, requires_indexing) = if let Some((id, old_hash, status, old_size, old_modified)) = existing {
            if status == "completed"
                && old_size == metadata.len() as i64
                && old_modified == modified
            {
                connection.execute(
                    "UPDATE document SET canonical_path=?1 WHERE id=?2",
                    params![canonical_text, id],
                ).map_err(|error| db_error("No fue posible actualizar la ruta", error))?;
                (id, false)
            } else {
            let hash = hash_file(&canonical)?;
            let changed = old_hash != hash || status != "completed";
            if changed {
                connection.execute("DELETE FROM knowledge_item WHERE document_id=?1", [id])
                    .map_err(|error| db_error("No fue posible limpiar chunks anteriores", error))?;
                connection.execute(
                    "UPDATE document SET canonical_path=?1,size_bytes=?2,modified_at=?3,
                     content_hash=?4,mime_type=?5,indexing_status='pending',indexed_at=NULL,last_error=NULL
                     WHERE id=?6",
                    params![canonical_text, metadata.len() as i64, modified, hash, mime_from_path(&canonical), id],
                ).map_err(|error| db_error("No fue posible actualizar el documento", error))?;
            } else {
                connection.execute(
                    "UPDATE document SET canonical_path=?1,size_bytes=?2,modified_at=?3,last_error=NULL
                     WHERE id=?4",
                    params![canonical_text, metadata.len() as i64, modified, id],
                ).map_err(|error| db_error("No fue posible actualizar metadatos", error))?;
            }
            (id, changed)
            }
        } else {
            let hash = hash_file(&canonical)?;
            let (volume_id, file_id) = file_identity(&canonical)?;
            connection.execute(
                "INSERT INTO document(scope,folder_id,relative_path,canonical_path,volume_id,file_id,
                 managed_copy,mime_type,size_bytes,modified_at,content_hash,indexing_status)
                 VALUES('folder',?1,?2,?3,?4,?5,1,?6,?7,?8,?9,'pending')",
                params![folder_id, relative, canonical_text, volume_id, file_id,
                    mime_from_path(&canonical), metadata.len() as i64, modified, hash],
            ).map_err(|error| db_error("No fue posible registrar el documento", error))?;
            (connection.last_insert_rowid(), true)
        };
        if requires_indexing {
            candidates.push(ScanCandidate {
                document_id,
                canonical_path: canonical_text,
                relative_path: relative,
                extension: canonical.extension().and_then(|value| value.to_str())
                    .unwrap_or_default().to_ascii_lowercase(),
            });
        }
    }
    let mut statement = connection.prepare(
        "SELECT id,canonical_path FROM document WHERE scope='folder' AND folder_id=?1",
    ).map_err(|error| db_error("No fue posible reconciliar documentos", error))?;
    let stale = statement.query_map([folder_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| db_error("No fue posible consultar documentos previos", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer documentos previos", error))?;
    drop(statement);
    for (id, path) in stale {
        if !seen.contains(&path) {
            connection.execute("DELETE FROM document WHERE id=?1", [id])
                .map_err(|error| db_error("No fue posible retirar un documento eliminado", error))?;
        }
    }
    Ok(candidates)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDocumentChunk {
    pub folder_id: i64,
    pub document_id: i64,
    pub content: String,
    pub chunk_index: i64,
    pub token_count: i64,
    pub source_chunk_hash: String,
    pub source_excerpt: String,
    pub embedding: Vec<f32>,
}

#[tauri::command]
pub fn store_document_chunk(
    input: StoreDocumentChunk,
    database: State<'_, Database>,
) -> CrudResult<i64> {
    let content = required_text(&input.content, "content")?;
    let source_chunk_hash = required_text(&input.source_chunk_hash, "sourceChunkHash")?;
    let source_excerpt = required_text(&input.source_excerpt, "sourceExcerpt")?;
    let embedding_json = vector_json(&input.embedding)?;
    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let mut connection = database.connection()?;
    let transaction = connection.transaction()
        .map_err(|error| db_error("No fue posible iniciar el guardado del chunk", error))?;
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM document WHERE id=?1 AND folder_id=?2)",
        params![input.document_id, input.folder_id],
        |row| row.get(0),
    ).map_err(|error| db_error("No fue posible validar el documento", error))?;
    if !valid {
        return Err("El documento no pertenece a la carpeta indicada".into());
    }
    transaction.execute(
        "INSERT OR IGNORE INTO ai_model(provider,model_key,display_name,version,endpoint,enabled)
         VALUES('local','chat','Modelo conversacional local','1','',1)",
        [],
    ).map_err(|error| db_error("No fue posible registrar el modelo conversacional", error))?;
    let generator_model_id: i64 = transaction.query_row(
        "SELECT id FROM ai_model WHERE provider='local' AND model_key='chat' AND endpoint=''",
        [], |row| row.get(0),
    ).map_err(|error| db_error("No fue posible consultar el modelo conversacional", error))?;
    transaction.execute(
        "INSERT OR IGNORE INTO ai_model(provider,model_key,display_name,version,endpoint,enabled)
         VALUES('local','embedding','Modelo de embeddings local','1','',1)",
        [],
    ).map_err(|error| db_error("No fue posible registrar el modelo", error))?;
    let model_id: i64 = transaction.query_row(
        "SELECT id FROM ai_model WHERE provider='local' AND model_key='embedding' AND endpoint=''",
        [], |row| row.get(0),
    ).map_err(|error| db_error("No fue posible consultar el modelo", error))?;
    transaction.execute(
        "INSERT INTO model_capability(model_id,capability,embedding_dimensions,distance_metric)
         VALUES(?1,'embedding',?2,'cosine')
         ON CONFLICT(model_id,capability) DO UPDATE SET embedding_dimensions=excluded.embedding_dimensions",
        params![model_id, EMBEDDING_DIMENSIONS],
    ).map_err(|error| db_error("No fue posible registrar la capacidad", error))?;
    transaction.execute(
        "INSERT INTO knowledge_item(document_id,folder_id,generator_model_id,scope,source_type,content,content_hash,
         is_confirmed,chunk_index,token_count,location_metadata)
         VALUES(?1,?2,?3,'folder','document_knowledge',?4,?5,1,?6,?7,?8)",
        params![input.document_id, input.folder_id, generator_model_id, content, content_hash, input.chunk_index,
            input.token_count, serde_json::json!({
                "document_id": input.document_id,
                "source_chunk_hash": source_chunk_hash,
                "source_excerpt": source_excerpt.chars().take(280).collect::<String>(),
                "refinement": "two_pass"
            }).to_string()],
    ).map_err(|error| db_error("No fue posible guardar el chunk", error))?;
    let knowledge_id = transaction.last_insert_rowid();
    transaction.execute(
        "INSERT INTO knowledge_vector(knowledge_id,embedding,embedding_model_id,scope,folder_id)
         VALUES(?1,vec_f32(?2),?3,'folder',?4)",
        params![knowledge_id, embedding_json, model_id, input.folder_id],
    ).map_err(|error| db_error("No fue posible guardar el vector del chunk", error))?;
    transaction.commit().map_err(|error| db_error("No fue posible confirmar el chunk", error))?;
    Ok(knowledge_id)
}

#[tauri::command]
pub fn finish_document_indexing(
    document_id: i64,
    error: Option<String>,
    database: State<'_, Database>,
) -> CrudResult<bool> {
    let (status, indexed_at) = if error.is_some() {
        ("failed", None)
    } else {
        ("completed", Some("now"))
    };
    let mut connection = database.connection()?;
    let transaction = connection.transaction()
        .map_err(|failure| db_error("No fue posible finalizar el documento", failure))?;
    if error.is_some() {
        transaction.execute(
            "DELETE FROM knowledge_item WHERE document_id=?1",
            [document_id],
        ).map_err(|failure| db_error("No fue posible retirar chunks parciales", failure))?;
    }
    let changed = transaction.execute(
        "UPDATE document SET indexing_status=?1,indexed_at=CASE WHEN ?2 IS NULL THEN NULL
         ELSE strftime('%Y-%m-%dT%H:%M:%fZ','now') END,last_error=?3 WHERE id=?4",
        params![status, indexed_at, error, document_id],
    ).map_err(|failure| db_error("No fue posible finalizar el documento", failure))?;
    transaction.commit()
        .map_err(|failure| db_error("No fue posible confirmar el documento", failure))?;
    Ok(changed > 0)
}

#[tauri::command]
pub fn finish_folder_scan(
    folder_id: i64,
    error: Option<String>,
    database: State<'_, Database>,
) -> CrudResult<Folder> {
    let status = if error.is_some() { "failed" } else { "completed" };
    let connection = database.connection()?;
    connection.execute(
        "UPDATE folder SET scan_status=?1,last_error=?2,
         last_scanned_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),
         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3",
        params![status, error, folder_id],
    ).map_err(|error| db_error("No fue posible finalizar el escaneo", error))?;
    get_folder_conn(&connection, folder_id)
}

fn hash_file(path: &Path) -> CrudResult<String> {
    let mut file =
        File::open(path).map_err(|error| format!("No fue posible abrir el archivo: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("No fue posible leer el archivo: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(windows)]
fn file_identity(path: &Path) -> CrudResult<(Option<String>, Option<String>)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let file =
        File::open(path).map_err(|error| format!("No fue posible abrir el archivo: {error}"))?;
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
    if success == 0 {
        return Err(format!(
            "No fue posible obtener el File ID: {}",
            std::io::Error::last_os_error()
        ));
    }
    let file_index = ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64;
    Ok((
        Some(format!("{:08X}", information.dwVolumeSerialNumber)),
        Some(format!("{file_index:016X}")),
    ))
}

#[cfg(unix)]
fn file_identity(path: &Path) -> CrudResult<(Option<String>, Option<String>)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("No fue posible leer metadatos: {error}"))?;
    Ok((
        Some(metadata.dev().to_string()),
        Some(metadata.ino().to_string()),
    ))
}

#[cfg(not(any(windows, unix)))]
fn file_identity(_path: &Path) -> CrudResult<(Option<String>, Option<String>)> {
    Ok((None, None))
}

fn mime_from_path(path: &Path) -> Option<String> {
    let mime = match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "md" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "json" | "toml" | "yaml"
        | "yml" => "text/plain",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    };
    Some(mime.into())
}

fn available_destination(folder: &Path, file_name: &str, source_hash: &str) -> CrudResult<PathBuf> {
    let initial = folder.join(file_name);
    if !initial.exists() || hash_file(&initial)? == source_hash {
        return Ok(initial);
    }
    let source = Path::new(file_name);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("documento");
    let extension = source.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let name = extension.map_or_else(
            || format!("{stem} ({index})"),
            |ext| format!("{stem} ({index}).{ext}"),
        );
        let candidate = folder.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("No fue posible asignar un nombre al documento copiado".into())
}

#[tauri::command]
pub fn attach_document(
    file_path: String,
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<Document> {
    let source = PathBuf::from(required_text(&file_path, "filePath")?);
    if !source.is_file() {
        return Err("La ruta seleccionada no es un archivo".into());
    }
    let source_hash = hash_file(&source)?;
    let connection = database.connection()?;
    let scope = if folder_id.is_some() {
        "folder"
    } else {
        "general"
    };
    let existing_id: Option<i64> = connection
        .query_row(
            "SELECT id FROM document WHERE scope=?1 AND content_hash=?2 AND
         ((?3 IS NULL AND folder_id IS NULL) OR folder_id=?3)",
            params![scope, source_hash, folder_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| db_error("No fue posible buscar documentos duplicados", error))?;
    if let Some(id) = existing_id {
        return get_document_conn(&connection, id);
    }

    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("El nombre del archivo no es válido")?;
    let (stored_path, managed_copy) = if let Some(id) = folder_id {
        let folder_path: String = connection
            .query_row(
                "SELECT canonical_path FROM folder WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| db_error("No fue posible consultar la carpeta", error))?
            .ok_or_else(|| format!("No existe la carpeta {id}"))?;
        let destination = available_destination(Path::new(&folder_path), file_name, &source_hash)?;
        if !destination.exists() {
            std::fs::copy(&source, &destination)
                .map_err(|error| format!("No fue posible copiar el documento: {error}"))?;
        }
        (destination, true)
    } else {
        (
            std::fs::canonicalize(&source)
                .map_err(|error| format!("No fue posible resolver la ruta: {error}"))?,
            false,
        )
    };
    let metadata = std::fs::metadata(&stored_path)
        .map_err(|error| format!("No fue posible leer metadatos: {error}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| format!("unix:{}", value.as_secs()))
        .unwrap_or_else(|| "unknown".into());
    let (volume_id, file_id) = file_identity(&stored_path)?;
    let canonical = stored_path.to_string_lossy().into_owned();
    let relative = stored_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name);
    connection.execute(
        "INSERT INTO document(scope,folder_id,relative_path,canonical_path,volume_id,file_id,managed_copy,
         mime_type,size_bytes,modified_at,content_hash,indexing_status)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'pending')",
        params![scope,folder_id,relative,canonical,volume_id,file_id,managed_copy as i64,
        mime_from_path(&stored_path),metadata.len() as i64,modified,source_hash])
        .map_err(|error|db_error("No fue posible registrar el documento",error))?;
    get_document_conn(&connection, connection.last_insert_rowid())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeOrigin {
    pub id: i64,
    pub scope: String,
    pub folder_id: Option<i64>,
    pub user_input: String,
    pub created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveKnowledgeOrigin {
    pub id: Option<i64>,
    pub scope: String,
    pub folder_id: Option<i64>,
    pub user_input: String,
}

fn origin_from_row(row: &Row<'_>) -> rusqlite::Result<KnowledgeOrigin> {
    Ok(KnowledgeOrigin {
        id: row.get(0)?,
        scope: row.get(1)?,
        folder_id: row.get(2)?,
        user_input: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn get_origin_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<KnowledgeOrigin> {
    connection
        .query_row(
            "SELECT id,scope,folder_id,user_input,created_at FROM knowledge_origin WHERE id=?1",
            [id],
            origin_from_row,
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar el origen", error))?
        .ok_or_else(|| format!("No existe el origen {id}"))
}

#[tauri::command]
pub fn create_knowledge_origin(
    input: SaveKnowledgeOrigin,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeOrigin> {
    validate_scope(&input.scope, input.folder_id)?;
    let connection = database.connection()?;
    connection
        .execute(
            "INSERT INTO knowledge_origin(scope,folder_id,user_input) VALUES(?1,?2,?3)",
            params![
                input.scope,
                input.folder_id,
                required_text(&input.user_input, "userInput")?
            ],
        )
        .map_err(|error| db_error("No fue posible crear el origen", error))?;
    get_origin_conn(&connection, connection.last_insert_rowid())
}

#[tauri::command]
pub fn get_knowledge_origin(id: i64, database: State<'_, Database>) -> CrudResult<KnowledgeOrigin> {
    let connection = database.connection()?;
    get_origin_conn(&connection, id)
}

#[tauri::command]
pub fn list_knowledge_origins(
    scope: Option<String>,
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<Vec<KnowledgeOrigin>> {
    let connection = database.connection()?;
    let mut statement = connection
        .prepare(
            "SELECT id,scope,folder_id,user_input,created_at FROM knowledge_origin
         WHERE (?1 IS NULL OR scope=?1) AND (?2 IS NULL OR folder_id=?2) ORDER BY id DESC",
        )
        .map_err(|error| db_error("No fue posible preparar la consulta de orígenes", error))?;
    let result = statement
        .query_map(params![scope, folder_id], origin_from_row)
        .map_err(|error| db_error("No fue posible listar orígenes", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer orígenes", error));
    result
}

#[tauri::command]
pub fn update_knowledge_origin(
    input: SaveKnowledgeOrigin,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeOrigin> {
    let id = input.id.ok_or("id es obligatorio para actualizar")?;
    validate_scope(&input.scope, input.folder_id)?;
    let connection = database.connection()?;
    let changed = connection
        .execute(
            "UPDATE knowledge_origin SET scope=?1,folder_id=?2,user_input=?3 WHERE id=?4",
            params![
                input.scope,
                input.folder_id,
                required_text(&input.user_input, "userInput")?,
                id
            ],
        )
        .map_err(|error| db_error("No fue posible actualizar el origen", error))?;
    if changed == 0 {
        return Err(format!("No existe el origen {id}"));
    }
    get_origin_conn(&connection, id)
}

#[tauri::command]
pub fn delete_knowledge_origin(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM knowledge_origin WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar el origen", error))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeItem {
    pub id: i64,
    pub origin_id: Option<i64>,
    pub document_id: Option<i64>,
    pub folder_id: Option<i64>,
    pub generator_model_id: Option<i64>,
    pub scope: String,
    pub source_type: String,
    pub content: String,
    pub content_hash: String,
    pub is_confirmed: bool,
    pub chunk_index: Option<i64>,
    pub token_count: Option<i64>,
    pub location_metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveKnowledgeItem {
    pub id: Option<i64>,
    pub origin_id: Option<i64>,
    pub document_id: Option<i64>,
    pub folder_id: Option<i64>,
    pub generator_model_id: Option<i64>,
    pub scope: String,
    pub source_type: String,
    pub content: String,
    pub content_hash: String,
    pub is_confirmed: bool,
    pub chunk_index: Option<i64>,
    pub token_count: Option<i64>,
    pub location_metadata: Option<String>,
}

fn knowledge_from_row(row: &Row<'_>) -> rusqlite::Result<KnowledgeItem> {
    Ok(KnowledgeItem {
        id: row.get(0)?,
        origin_id: row.get(1)?,
        document_id: row.get(2)?,
        folder_id: row.get(3)?,
        generator_model_id: row.get(4)?,
        scope: row.get(5)?,
        source_type: row.get(6)?,
        content: row.get(7)?,
        content_hash: row.get(8)?,
        is_confirmed: row.get::<_, i64>(9)? != 0,
        chunk_index: row.get(10)?,
        token_count: row.get(11)?,
        location_metadata: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

const KNOWLEDGE_SELECT: &str = "SELECT id,origin_id,document_id,folder_id,generator_model_id,scope,
 source_type,content,content_hash,is_confirmed,chunk_index,token_count,location_metadata,created_at,updated_at
 FROM knowledge_item";

fn get_knowledge_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<KnowledgeItem> {
    connection
        .query_row(
            &format!("{KNOWLEDGE_SELECT} WHERE id=?1"),
            [id],
            knowledge_from_row,
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar el conocimiento", error))?
        .ok_or_else(|| format!("No existe el conocimiento {id}"))
}

fn validate_knowledge(input: &SaveKnowledgeItem) -> CrudResult<()> {
    validate_scope(&input.scope, input.folder_id)?;
    required_text(&input.source_type, "sourceType")?;
    required_text(&input.content, "content")?;
    required_text(&input.content_hash, "contentHash")?;
    if input.chunk_index.is_some_and(|value| value < 0)
        || input.token_count.is_some_and(|value| value < 0)
    {
        return Err("chunkIndex y tokenCount no pueden ser negativos".into());
    }
    if let Some(metadata) = &input.location_metadata {
        serde_json::from_str::<serde_json::Value>(metadata)
            .map_err(|error| format!("locationMetadata no es JSON válido: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn create_knowledge_item(
    input: SaveKnowledgeItem,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeItem> {
    validate_knowledge(&input)?;
    let connection = database.connection()?;
    connection.execute(
        "INSERT INTO knowledge_item(origin_id,document_id,folder_id,generator_model_id,scope,source_type,
         content,content_hash,is_confirmed,chunk_index,token_count,location_metadata)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![input.origin_id,input.document_id,input.folder_id,input.generator_model_id,input.scope,
        input.source_type,input.content,input.content_hash,input.is_confirmed as i64,input.chunk_index,
        input.token_count,input.location_metadata],
    ).map_err(|error| db_error("No fue posible crear el conocimiento", error))?;
    get_knowledge_conn(&connection, connection.last_insert_rowid())
}

#[tauri::command]
pub fn get_knowledge_item(id: i64, database: State<'_, Database>) -> CrudResult<KnowledgeItem> {
    let connection = database.connection()?;
    get_knowledge_conn(&connection, id)
}

#[tauri::command]
pub fn list_knowledge_items(
    scope: Option<String>,
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<Vec<KnowledgeItem>> {
    let connection = database.connection()?;
    let mut statement = connection.prepare(&format!(
        "{KNOWLEDGE_SELECT} WHERE (?1 IS NULL OR scope=?1) AND (?2 IS NULL OR folder_id=?2) ORDER BY id DESC"
    )).map_err(|error| db_error("No fue posible preparar la consulta de conocimientos", error))?;
    let result = statement
        .query_map(params![scope, folder_id], knowledge_from_row)
        .map_err(|error| db_error("No fue posible listar conocimientos", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer conocimientos", error));
    result
}

#[tauri::command]
pub fn update_knowledge_item(
    input: SaveKnowledgeItem,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeItem> {
    let id = input.id.ok_or("id es obligatorio para actualizar")?;
    validate_knowledge(&input)?;
    let connection = database.connection()?;
    let changed = connection.execute(
        "UPDATE knowledge_item SET origin_id=?1,document_id=?2,folder_id=?3,generator_model_id=?4,
         scope=?5,source_type=?6,content=?7,content_hash=?8,is_confirmed=?9,chunk_index=?10,
         token_count=?11,location_metadata=?12,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?13",
        params![input.origin_id,input.document_id,input.folder_id,input.generator_model_id,input.scope,
        input.source_type,input.content,input.content_hash,input.is_confirmed as i64,input.chunk_index,
        input.token_count,input.location_metadata,id],
    ).map_err(|error| db_error("No fue posible actualizar el conocimiento", error))?;
    if changed == 0 {
        return Err(format!("No existe el conocimiento {id}"));
    }
    get_knowledge_conn(&connection, id)
}

#[tauri::command]
pub fn delete_knowledge_item(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM knowledge_item WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar el conocimiento", error))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeLog {
    pub id: i64,
    pub content: String,
    pub scope: String,
    pub source_type: String,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub document_id: Option<i64>,
    pub document_path: Option<String>,
    pub origin_id: Option<i64>,
    pub user_input: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub fn list_recent_knowledge_logs(
    limit: u32,
    database: State<'_, Database>,
) -> CrudResult<Vec<KnowledgeLog>> {
    if limit == 0 || limit > 200 {
        return Err("limit debe estar entre 1 y 200".into());
    }
    let connection = database.connection()?;
    list_recent_knowledge_logs_conn(&connection, limit)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseKnowledgeItem {
    pub id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
}

#[tauri::command]
pub fn revise_knowledge_item(
    input: ReviseKnowledgeItem,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeLog> {
    let content = required_text(&input.content, "content")?;
    let embedding_json = vector_json(&input.embedding)?;
    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let mut connection = database.connection()?;
    let transaction = connection.transaction()
        .map_err(|error| db_error("No fue posible iniciar la corrección", error))?;
    let vector: (i64, String, i64) = transaction.query_row(
        "SELECT embedding_model_id,scope,folder_id
         FROM knowledge_vector WHERE knowledge_id=?1",
        [input.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional()
        .map_err(|error| db_error("No fue posible consultar el vector", error))?
        .ok_or_else(|| format!("El conocimiento {} no tiene un vector asociado", input.id))?;
    let metadata: Option<String> = transaction.query_row(
        "SELECT location_metadata FROM knowledge_item WHERE id=?1",
        [input.id],
        |row| row.get(0),
    ).optional()
        .map_err(|error| db_error("No fue posible consultar el conocimiento", error))?
        .ok_or_else(|| format!("No existe el conocimiento {}", input.id))?;
    let mut metadata_json = metadata
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = metadata_json.as_object_mut() {
        object.insert("manual_revision".into(), serde_json::json!(true));
    }
    transaction.execute(
        "UPDATE knowledge_item
         SET content=?1,content_hash=?2,is_confirmed=1,token_count=?3,
             location_metadata=?4,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id=?5",
        params![content, content_hash, (content.chars().count() / 4).max(1) as i64,
            metadata_json.to_string(), input.id],
    ).map_err(|error| db_error("No fue posible actualizar el conocimiento", error))?;
    transaction.execute("DELETE FROM knowledge_vector WHERE knowledge_id=?1", [input.id])
        .map_err(|error| db_error("No fue posible retirar el vector anterior", error))?;
    transaction.execute(
        "INSERT INTO knowledge_vector(knowledge_id,embedding,embedding_model_id,scope,folder_id)
         VALUES(?1,vec_f32(?2),?3,?4,?5)",
        params![input.id, embedding_json, vector.0, vector.1, vector.2],
    ).map_err(|error| db_error("No fue posible guardar el vector corregido", error))?;
    transaction.commit()
        .map_err(|error| db_error("No fue posible confirmar la corrección", error))?;

    let logs = list_recent_knowledge_logs_conn(&connection, 200)?;
    logs.into_iter().find(|item| item.id == input.id)
        .ok_or_else(|| format!("No fue posible recuperar el conocimiento {}", input.id))
}

fn list_recent_knowledge_logs_conn(
    connection: &rusqlite::Connection,
    limit: u32,
) -> CrudResult<Vec<KnowledgeLog>> {
    let mut statement = connection.prepare(
        "SELECT knowledge_item.id,knowledge_item.content,knowledge_item.scope,
                knowledge_item.source_type,knowledge_item.folder_id,folder.name,
                knowledge_item.document_id,document.relative_path,
                knowledge_item.origin_id,knowledge_origin.user_input,
                knowledge_item.created_at,knowledge_item.updated_at
         FROM knowledge_item
         LEFT JOIN folder ON folder.id=knowledge_item.folder_id
         LEFT JOIN document ON document.id=knowledge_item.document_id
         LEFT JOIN knowledge_origin ON knowledge_origin.id=knowledge_item.origin_id
         ORDER BY knowledge_item.id DESC LIMIT ?1",
    ).map_err(|error| db_error("No fue posible preparar los knowledge logs", error))?;
    let result = statement.query_map([limit], |row| Ok(KnowledgeLog {
        id: row.get(0)?, content: row.get(1)?, scope: row.get(2)?,
        source_type: row.get(3)?, folder_id: row.get(4)?, folder_name: row.get(5)?,
        document_id: row.get(6)?, document_path: row.get(7)?, origin_id: row.get(8)?,
        user_input: row.get(9)?, created_at: row.get(10)?, updated_at: row.get(11)?,
    })).map_err(|error| db_error("No fue posible consultar los knowledge logs", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer los knowledge logs", error));
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: i64,
    pub provider: String,
    pub model_key: String,
    pub display_name: String,
    pub version: Option<String>,
    pub endpoint: String,
    pub metadata_json: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiModel {
    pub id: Option<i64>,
    pub provider: String,
    pub model_key: String,
    pub display_name: String,
    pub version: Option<String>,
    pub endpoint: String,
    pub metadata_json: Option<String>,
    pub enabled: bool,
}

fn model_from_row(row: &Row<'_>) -> rusqlite::Result<AiModel> {
    Ok(AiModel {
        id: row.get(0)?,
        provider: row.get(1)?,
        model_key: row.get(2)?,
        display_name: row.get(3)?,
        version: row.get(4)?,
        endpoint: row.get(5)?,
        metadata_json: row.get(6)?,
        enabled: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

const MODEL_SELECT: &str = "SELECT id,provider,model_key,display_name,version,endpoint,metadata_json,enabled,created_at,updated_at FROM ai_model";

fn get_model_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<AiModel> {
    connection
        .query_row(&format!("{MODEL_SELECT} WHERE id=?1"), [id], model_from_row)
        .optional()
        .map_err(|error| db_error("No fue posible consultar el modelo", error))?
        .ok_or_else(|| format!("No existe el modelo {id}"))
}

fn validate_json(value: &Option<String>, field: &str) -> CrudResult<()> {
    if let Some(value) = value {
        serde_json::from_str::<serde_json::Value>(value)
            .map_err(|error| format!("{field} no es JSON válido: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn create_ai_model(input: SaveAiModel, database: State<'_, Database>) -> CrudResult<AiModel> {
    validate_json(&input.metadata_json, "metadataJson")?;
    let connection = database.connection()?;
    connection.execute("INSERT INTO ai_model(provider,model_key,display_name,version,endpoint,metadata_json,enabled) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![required_text(&input.provider,"provider")?,required_text(&input.model_key,"modelKey")?,
        required_text(&input.display_name,"displayName")?,input.version,input.endpoint.trim(),input.metadata_json,input.enabled as i64])
        .map_err(|error|db_error("No fue posible crear el modelo",error))?;
    get_model_conn(&connection, connection.last_insert_rowid())
}

#[tauri::command]
pub fn get_ai_model(id: i64, database: State<'_, Database>) -> CrudResult<AiModel> {
    let connection = database.connection()?;
    get_model_conn(&connection, id)
}

#[tauri::command]
pub fn list_ai_models(database: State<'_, Database>) -> CrudResult<Vec<AiModel>> {
    let connection = database.connection()?;
    let mut statement = connection
        .prepare(&format!(
            "{MODEL_SELECT} ORDER BY display_name COLLATE NOCASE"
        ))
        .map_err(|error| db_error("No fue posible preparar la consulta de modelos", error))?;
    let result = statement
        .query_map([], model_from_row)
        .map_err(|error| db_error("No fue posible listar modelos", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer modelos", error));
    result
}

#[tauri::command]
pub fn update_ai_model(input: SaveAiModel, database: State<'_, Database>) -> CrudResult<AiModel> {
    let id = input.id.ok_or("id es obligatorio para actualizar")?;
    validate_json(&input.metadata_json, "metadataJson")?;
    let connection = database.connection()?;
    let changed = connection
        .execute(
            "UPDATE ai_model SET provider=?1,model_key=?2,display_name=?3,version=?4,endpoint=?5,
        metadata_json=?6,enabled=?7,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?8",
            params![
                required_text(&input.provider, "provider")?,
                required_text(&input.model_key, "modelKey")?,
                required_text(&input.display_name, "displayName")?,
                input.version,
                input.endpoint.trim(),
                input.metadata_json,
                input.enabled as i64,
                id
            ],
        )
        .map_err(|error| db_error("No fue posible actualizar el modelo", error))?;
    if changed == 0 {
        return Err(format!("No existe el modelo {id}"));
    }
    get_model_conn(&connection, id)
}

#[tauri::command]
pub fn delete_ai_model(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM ai_model WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar el modelo", error))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapability {
    pub model_id: i64,
    pub capability: String,
    pub embedding_dimensions: Option<i64>,
    pub distance_metric: Option<String>,
    pub context_window: Option<i64>,
    pub configuration_json: Option<String>,
}

#[tauri::command]
pub fn upsert_model_capability(
    input: ModelCapability,
    database: State<'_, Database>,
) -> CrudResult<ModelCapability> {
    validate_json(&input.configuration_json, "configurationJson")?;
    if input.capability == "embedding"
        && input.embedding_dimensions != Some(EMBEDDING_DIMENSIONS as i64)
    {
        return Err(format!(
            "El índice actual requiere embeddingDimensions={EMBEDDING_DIMENSIONS}"
        ));
    }
    database.connection()?.execute(
        "INSERT INTO model_capability(model_id,capability,embedding_dimensions,distance_metric,context_window,configuration_json)
         VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(model_id,capability) DO UPDATE SET
         embedding_dimensions=excluded.embedding_dimensions,distance_metric=excluded.distance_metric,
         context_window=excluded.context_window,configuration_json=excluded.configuration_json",
        params![input.model_id,required_text(&input.capability,"capability")?,input.embedding_dimensions,
        input.distance_metric,input.context_window,input.configuration_json])
        .map_err(|error|db_error("No fue posible guardar la capacidad",error))?;
    Ok(input)
}

#[tauri::command]
pub fn list_model_capabilities(
    model_id: i64,
    database: State<'_, Database>,
) -> CrudResult<Vec<ModelCapability>> {
    let connection = database.connection()?;
    let mut statement=connection.prepare("SELECT model_id,capability,embedding_dimensions,distance_metric,context_window,configuration_json FROM model_capability WHERE model_id=?1 ORDER BY capability")
        .map_err(|error|db_error("No fue posible preparar capacidades",error))?;
    let result = statement
        .query_map([model_id], |row| {
            Ok(ModelCapability {
                model_id: row.get(0)?,
                capability: row.get(1)?,
                embedding_dimensions: row.get(2)?,
                distance_metric: row.get(3)?,
                context_window: row.get(4)?,
                configuration_json: row.get(5)?,
            })
        })
        .map_err(|error| db_error("No fue posible listar capacidades", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer capacidades", error));
    result
}

#[tauri::command]
pub fn get_model_capability(
    model_id: i64,
    capability: String,
    database: State<'_, Database>,
) -> CrudResult<ModelCapability> {
    database
        .connection()?
        .query_row(
            "SELECT model_id,capability,embedding_dimensions,distance_metric,context_window,
             configuration_json FROM model_capability WHERE model_id=?1 AND capability=?2",
            params![model_id, capability],
            |row| {
                Ok(ModelCapability {
                    model_id: row.get(0)?,
                    capability: row.get(1)?,
                    embedding_dimensions: row.get(2)?,
                    distance_metric: row.get(3)?,
                    context_window: row.get(4)?,
                    configuration_json: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar la capacidad", error))?
        .ok_or_else(|| "No existe la capacidad solicitada".to_string())
}

#[tauri::command]
pub fn delete_model_capability(
    model_id: i64,
    capability: String,
    database: State<'_, Database>,
) -> CrudResult<bool> {
    database
        .connection()?
        .execute(
            "DELETE FROM model_capability WHERE model_id=?1 AND capability=?2",
            params![model_id, capability],
        )
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar la capacidad", error))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAssignment {
    pub id: Option<i64>,
    pub model_id: i64,
    pub task: String,
    pub settings_json: Option<String>,
    pub active: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

fn assignment_from_row(row: &Row<'_>) -> rusqlite::Result<ModelAssignment> {
    Ok(ModelAssignment {
        id: Some(row.get(0)?),
        model_id: row.get(1)?,
        task: row.get(2)?,
        settings_json: row.get(3)?,
        active: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn get_assignment_conn(connection: &rusqlite::Connection, id: i64) -> CrudResult<ModelAssignment> {
    connection.query_row("SELECT id,model_id,task,settings_json,active,created_at,updated_at FROM model_assignment WHERE id=?1",[id],assignment_from_row)
        .optional().map_err(|error|db_error("No fue posible consultar la asignación",error))?.ok_or_else(||format!("No existe la asignación {id}"))
}

#[tauri::command]
pub fn get_model_assignment(id: i64, database: State<'_, Database>) -> CrudResult<ModelAssignment> {
    let connection = database.connection()?;
    get_assignment_conn(&connection, id)
}

#[tauri::command]
pub fn create_model_assignment(
    input: ModelAssignment,
    database: State<'_, Database>,
) -> CrudResult<ModelAssignment> {
    validate_json(&input.settings_json, "settingsJson")?;
    let connection = database.connection()?;
    connection
        .execute(
            "INSERT INTO model_assignment(model_id,task,settings_json,active) VALUES(?1,?2,?3,?4)",
            params![
                input.model_id,
                required_text(&input.task, "task")?,
                input.settings_json,
                input.active as i64
            ],
        )
        .map_err(|error| db_error("No fue posible crear la asignación", error))?;
    get_assignment_conn(&connection, connection.last_insert_rowid())
}

#[tauri::command]
pub fn list_model_assignments(database: State<'_, Database>) -> CrudResult<Vec<ModelAssignment>> {
    let connection = database.connection()?;
    let mut statement=connection.prepare(
        "SELECT id,model_id,task,settings_json,active,created_at,updated_at FROM model_assignment ORDER BY task")
        .map_err(|error|db_error("No fue posible preparar asignaciones",error))?;
    let result = statement
        .query_map([], assignment_from_row)
        .map_err(|error| db_error("No fue posible listar asignaciones", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer asignaciones", error));
    result
}

#[tauri::command]
pub fn update_model_assignment(
    input: ModelAssignment,
    database: State<'_, Database>,
) -> CrudResult<ModelAssignment> {
    let id = input.id.ok_or("id es obligatorio para actualizar")?;
    validate_json(&input.settings_json, "settingsJson")?;
    let connection = database.connection()?;
    let changed = connection
        .execute(
            "UPDATE model_assignment SET model_id=?1,task=?2,settings_json=?3,active=?4,
         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?5",
            params![
                input.model_id,
                required_text(&input.task, "task")?,
                input.settings_json,
                input.active as i64,
                id
            ],
        )
        .map_err(|error| db_error("No fue posible actualizar la asignación", error))?;
    if changed == 0 {
        return Err(format!("No existe la asignación {id}"));
    }
    get_assignment_conn(&connection, id)
}

#[tauri::command]
pub fn delete_model_assignment(id: i64, database: State<'_, Database>) -> CrudResult<bool> {
    database
        .connection()?
        .execute("DELETE FROM model_assignment WHERE id=?1", [id])
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar la asignación", error))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeVector {
    pub knowledge_id: i64,
    pub embedding_model_id: i64,
    pub embedding: Vec<f32>,
    pub scope: String,
    pub folder_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct KnowledgeContextMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreChatKnowledgeItem {
    pub content: String,
    pub embedding: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreChatKnowledge {
    pub user_input: String,
    pub context_messages: Vec<KnowledgeContextMessage>,
    pub items: Vec<StoreChatKnowledgeItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatKnowledge {
    pub knowledge_ids: Vec<i64>,
    pub created_count: usize,
}

#[tauri::command]
pub fn store_general_chat_knowledge(
    input: StoreChatKnowledge,
    folder_id: Option<i64>,
    database: State<'_, Database>,
) -> CrudResult<StoredChatKnowledge> {
    let user_input = required_text(&input.user_input, "userInput")?;
    if input.items.is_empty() || input.items.len() > 20 {
        return Err("items debe contener entre uno y veinte conocimientos".into());
    }
    if input.context_messages.len() > 4 {
        return Err("contextMessages admite como máximo cuatro mensajes".into());
    }
    for message in &input.context_messages {
        if message.role != "user" && message.role != "assistant" {
            return Err("El contexto contiene un role inválido".into());
        }
        required_text(&message.content, "contextMessages.content")?;
    }
    let prepared = input.items.iter().map(|item| {
        let content = required_text(&item.content, "items.content")?;
        let embedding_json = vector_json(&item.embedding)?;
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        Ok((content, content_hash, embedding_json))
    }).collect::<CrudResult<Vec<_>>>()?;
    let mut connection = database.connection()?;
    let scope = if folder_id.is_some() { "folder" } else { "general" };
    let transaction = connection
        .transaction()
        .map_err(|error| db_error("No fue posible iniciar la memoria", error))?;

    transaction
        .execute(
            "INSERT OR IGNORE INTO ai_model(provider,model_key,display_name,version,endpoint,enabled)
             VALUES('local','chat','Modelo conversacional local','1','',1)",
            [],
        )
        .map_err(|error| db_error("No fue posible registrar el modelo conversacional", error))?;
    let chat_model_id: i64 = transaction
        .query_row(
            "SELECT id FROM ai_model WHERE provider='local' AND model_key='chat' AND endpoint=''",
            [],
            |row| row.get(0),
        )
        .map_err(|error| db_error("No fue posible consultar el modelo conversacional", error))?;

    transaction
        .execute(
            "INSERT OR IGNORE INTO ai_model(provider,model_key,display_name,version,endpoint,enabled)
             VALUES('local','embedding','Modelo de embeddings local','1','',1)",
            [],
        )
        .map_err(|error| db_error("No fue posible registrar el modelo de embeddings", error))?;
    let embedding_model_id: i64 = transaction
        .query_row(
            "SELECT id FROM ai_model WHERE provider='local' AND model_key='embedding' AND endpoint=''",
            [],
            |row| row.get(0),
        )
        .map_err(|error| db_error("No fue posible consultar el modelo de embeddings", error))?;
    transaction
        .execute(
            "INSERT INTO model_capability(model_id,capability,embedding_dimensions,distance_metric)
             VALUES(?1,'embedding',?2,'cosine')
             ON CONFLICT(model_id,capability) DO UPDATE SET
             embedding_dimensions=excluded.embedding_dimensions,
             distance_metric=excluded.distance_metric",
            params![embedding_model_id, EMBEDDING_DIMENSIONS],
        )
        .map_err(|error| db_error("No fue posible registrar la capacidad embedding", error))?;

    transaction
        .execute(
            "INSERT INTO knowledge_origin(scope,folder_id,user_input)
             VALUES(?1,?2,?3)",
            params![scope, folder_id, user_input],
        )
        .map_err(|error| db_error("No fue posible guardar el origen del conocimiento", error))?;
    let origin_id = transaction.last_insert_rowid();
    let metadata = serde_json::json!({
        "context_messages": input.context_messages,
        "capture": "automatic_chat_statement",
        "refinement": "two_pass"
    })
    .to_string();
    let mut knowledge_ids = Vec::new();
    let mut created_count = 0_usize;
    for (content, content_hash, embedding_json) in prepared {
        let existing: Option<i64> = transaction.query_row(
            "SELECT id FROM knowledge_item
             WHERE scope=?1 AND folder_id IS ?2 AND content_hash=?3 LIMIT 1",
            params![scope, folder_id, content_hash],
            |row| row.get(0),
        ).optional().map_err(|error| db_error("No fue posible buscar conocimiento duplicado", error))?;
        if let Some(id) = existing {
            knowledge_ids.push(id);
            continue;
        }
        transaction.execute(
            "INSERT INTO knowledge_item(
                origin_id,generator_model_id,scope,source_type,content,content_hash,
                folder_id,is_confirmed,location_metadata
             ) VALUES(?1,?2,?3,'chat_statement',?4,?5,?6,1,?7)",
            params![origin_id, chat_model_id, scope, content, content_hash, folder_id, metadata],
        ).map_err(|error| db_error("No fue posible guardar el conocimiento", error))?;
        let knowledge_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO knowledge_vector(
                knowledge_id,embedding,embedding_model_id,scope,folder_id
             ) VALUES(?1,vec_f32(?2),?3,?4,?5)",
            params![knowledge_id, embedding_json, embedding_model_id, scope, folder_id.unwrap_or(0)],
        ).map_err(|error| db_error("No fue posible guardar el vector del conocimiento", error))?;
        knowledge_ids.push(knowledge_id);
        created_count += 1;
    }
    if created_count == 0 {
        transaction.execute("DELETE FROM knowledge_origin WHERE id=?1", [origin_id])
            .map_err(|error| db_error("No fue posible retirar un origen duplicado", error))?;
    }
    transaction
        .commit()
        .map_err(|error| db_error("No fue posible confirmar la memoria", error))?;
    Ok(StoredChatKnowledge {
        knowledge_ids,
        created_count,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMatch {
    pub knowledge: KnowledgeItem,
    pub distance: f64,
}

fn vector_json(embedding: &[f32]) -> CrudResult<String> {
    if embedding.len() != EMBEDDING_DIMENSIONS as usize {
        return Err(format!(
            "El embedding debe tener {EMBEDDING_DIMENSIONS} dimensiones y recibió {}",
            embedding.len()
        ));
    }
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err("El embedding contiene valores no finitos".into());
    }
    serde_json::to_string(embedding)
        .map_err(|error| format!("No fue posible serializar el embedding: {error}"))
}

#[tauri::command]
pub fn upsert_knowledge_vector(
    input: KnowledgeVector,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeVector> {
    validate_scope(&input.scope, input.folder_id)?;
    let embedding_json = vector_json(&input.embedding)?;
    let mut connection = database.connection()?;
    let transaction = connection
        .transaction()
        .map_err(|error| db_error("No fue posible iniciar la transacción vectorial", error))?;
    let item: (String, Option<i64>) = transaction
        .query_row(
            "SELECT scope,folder_id FROM knowledge_item WHERE id=?1",
            [input.knowledge_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| db_error("No fue posible validar el conocimiento", error))?
        .ok_or_else(|| format!("No existe el conocimiento {}", input.knowledge_id))?;
    if item != (input.scope.clone(), input.folder_id) {
        return Err("El scope o folderId del vector no coincide con knowledge_item".into());
    }
    let embedding_capability:bool=transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_capability WHERE model_id=?1 AND capability='embedding')",
        [input.embedding_model_id],|row|row.get(0)).map_err(|error|db_error("No fue posible validar el modelo",error))?;
    if !embedding_capability {
        return Err("El modelo no tiene la capacidad embedding".into());
    }
    transaction
        .execute(
            "DELETE FROM knowledge_vector WHERE knowledge_id=?1",
            [input.knowledge_id],
        )
        .map_err(|error| db_error("No fue posible reemplazar el vector", error))?;
    transaction.execute("INSERT INTO knowledge_vector(knowledge_id,embedding,embedding_model_id,scope,folder_id)
        VALUES(?1,vec_f32(?2),?3,?4,?5)",params![input.knowledge_id,embedding_json,input.embedding_model_id,
        input.scope,input.folder_id.unwrap_or(0)]).map_err(|error|db_error("No fue posible guardar el vector",error))?;
    transaction
        .commit()
        .map_err(|error| db_error("No fue posible confirmar el vector", error))?;
    Ok(input)
}

#[tauri::command]
pub fn get_knowledge_vector(
    knowledge_id: i64,
    database: State<'_, Database>,
) -> CrudResult<KnowledgeVector> {
    let connection = database.connection()?;
    let record = connection
        .query_row(
            "SELECT knowledge_id,embedding_model_id,vec_to_json(embedding),scope,folder_id
        FROM knowledge_vector WHERE knowledge_id=?1",
            [knowledge_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| db_error("No fue posible consultar el vector", error))?
        .ok_or_else(|| format!("No existe el vector para knowledgeId {knowledge_id}"))?;
    Ok(KnowledgeVector {
        knowledge_id: record.0,
        embedding_model_id: record.1,
        embedding: serde_json::from_str(&record.2)
            .map_err(|error| format!("Vector inválido en SQLite: {error}"))?,
        scope: record.3,
        folder_id: if record.4 == 0 { None } else { Some(record.4) },
    })
}

#[tauri::command]
pub fn delete_knowledge_vector(
    knowledge_id: i64,
    database: State<'_, Database>,
) -> CrudResult<bool> {
    database
        .connection()?
        .execute(
            "DELETE FROM knowledge_vector WHERE knowledge_id=?1",
            [knowledge_id],
        )
        .map(|changed| changed > 0)
        .map_err(|error| db_error("No fue posible eliminar el vector", error))
}

#[tauri::command]
pub fn search_knowledge(
    embedding: Vec<f32>,
    scope: String,
    folder_id: Option<i64>,
    limit: u32,
    database: State<'_, Database>,
) -> CrudResult<Vec<KnowledgeMatch>> {
    validate_scope(&scope, folder_id)?;
    if limit == 0 || limit > 100 {
        return Err("limit debe estar entre 1 y 100".into());
    }
    let embedding_json = vector_json(&embedding)?;
    let connection = database.connection()?;
    let sql = "WITH matches AS (
        SELECT knowledge_id,distance FROM knowledge_vector
        WHERE embedding MATCH vec_f32(?1) AND k=?2 AND scope=?3 AND folder_id=?4
    ) SELECT knowledge_item.id,knowledge_item.origin_id,knowledge_item.document_id,
       knowledge_item.folder_id,knowledge_item.generator_model_id,knowledge_item.scope,
       knowledge_item.source_type,knowledge_item.content,knowledge_item.content_hash,
       knowledge_item.is_confirmed,knowledge_item.chunk_index,knowledge_item.token_count,
       knowledge_item.location_metadata,knowledge_item.created_at,knowledge_item.updated_at,
       matches.distance
       FROM matches JOIN knowledge_item ON knowledge_item.id=matches.knowledge_id
       ORDER BY matches.distance";
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| db_error("No fue posible preparar la búsqueda", error))?;
    let result = statement
        .query_map(
            params![embedding_json, limit, scope, folder_id.unwrap_or(0)],
            |row| {
                let knowledge = knowledge_from_row(row)?;
                let distance = row.get(15)?;
                Ok(KnowledgeMatch {
                    knowledge,
                    distance,
                })
            },
        )
        .map_err(|error| db_error("No fue posible buscar conocimientos", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_error("No fue posible leer los resultados", error));
    result
}
