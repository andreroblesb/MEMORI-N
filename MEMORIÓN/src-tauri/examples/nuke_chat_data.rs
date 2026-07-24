use rusqlite::{ffi::sqlite3_auto_extension, Connection, OpenFlags};
use sqlite_vec::sqlite3_vec_init;
use std::{
    env,
    path::{Path, PathBuf},
    sync::Once,
    time::{SystemTime, UNIX_EPOCH},
};

const NUKE_SQL: &str = include_str!("../maintenance/nuke_chat_data.sql");
static REGISTER_SQLITE_VEC: Once = Once::new();

fn register_sqlite_vec() {
    REGISTER_SQLITE_VEC.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}

fn application_data_directory() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        return env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("No existe LOCALAPPDATA".into());
    }
    #[cfg(target_os = "macos")]
    {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library").join("Application Support"))
            .ok_or("No existe HOME".into());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(path) = env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(path));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".local").join("share"))
            .ok_or("No existe HOME".into())
    }
}

fn backup_path(database_path: &Path) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("El reloj del sistema no es válido: {error}"))?
        .as_secs();
    Ok(database_path.with_file_name(format!(
        "memorion.before-nuke-{timestamp}.sqlite3"
    )))
}

fn run() -> Result<(), String> {
    let confirmed = env::args().any(|argument| argument == "--confirm=NUKE_CHAT_DATA");
    if !confirmed {
        return Err(
            "Confirmación faltante. Usa exactamente --confirm=NUKE_CHAT_DATA".into(),
        );
    }

    let database_path = application_data_directory()?
        .join("MEMORIÓN")
        .join("data")
        .join("memorion.sqlite3");
    if !database_path.is_file() {
        return Err(format!("No existe {}", database_path.display()));
    }

    register_sqlite_vec();
    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("No fue posible abrir SQLite: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .map_err(|error| {
            format!(
                "No fue posible preparar la base. Confirma que MEMORIÓN esté cerrado: {error}"
            )
        })?;

    let backup = backup_path(&database_path)?;
    std::fs::copy(&database_path, &backup)
        .map_err(|error| format!("No fue posible crear el respaldo: {error}"))?;

    connection
        .execute_batch(NUKE_SQL)
        .map_err(|error| format!("El nuke falló y la transacción fue revertida: {error}"))?;
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| format!("El nuke terminó, pero falló el checkpoint: {error}"))?;

    let models: i64 = connection
        .query_row("SELECT count(*) FROM ai_model", [], |row| row.get(0))
        .map_err(|error| format!("No fue posible contar los modelos: {error}"))?;
    println!("Nuke completado.");
    println!("Base: {}", database_path.display());
    println!("Respaldo: {}", backup.display());
    println!("Modelos conservados en ai_model: {models}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
