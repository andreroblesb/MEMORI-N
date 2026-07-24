mod database;

use database::{Database, DatabaseStatus};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};
use sysinfo::System;
use tauri::{Manager, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderChat {
    id: u64,
    name: String,
    count: usize,
    color: String,
}

#[derive(Clone, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemMetrics {
    cpu_percent: f32,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
}

struct MemoryState {
    folders: Vec<FolderChat>,
    messages: HashMap<String, Vec<ChatMessage>>,
    next_folder_id: u64,
}

struct AppState {
    memory: Mutex<MemoryState>,
    system: Mutex<System>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            memory: Mutex::new(MemoryState {
                folders: vec![
                    FolderChat {
                        id: 1,
                        name: "Investigación".into(),
                        count: 0,
                        color: "violet".into(),
                    },
                    FolderChat {
                        id: 2,
                        name: "Universidad".into(),
                        count: 0,
                        color: "blue".into(),
                    },
                    FolderChat {
                        id: 3,
                        name: "Ideas de producto".into(),
                        count: 0,
                        color: "teal".into(),
                    },
                ],
                messages: HashMap::new(),
                next_folder_id: 4,
            }),
            system: Mutex::new(System::new_all()),
        }
    }
}

fn chat_key(folder_id: Option<u64>) -> String {
    folder_id.map_or_else(|| "general".into(), |id| format!("folder:{id}"))
}

#[tauri::command]
fn list_folder_chats(state: State<'_, AppState>) -> Result<Vec<FolderChat>, String> {
    let memory = state
        .memory
        .lock()
        .map_err(|_| "No fue posible acceder al estado".to_string())?;
    Ok(memory.folders.clone())
}

#[tauri::command]
fn create_folder_chat(name: String, state: State<'_, AppState>) -> Result<FolderChat, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }
    let mut memory = state
        .memory
        .lock()
        .map_err(|_| "No fue posible acceder al estado".to_string())?;
    let folder = FolderChat {
        id: memory.next_folder_id,
        name: clean_name.into(),
        count: 0,
        color: "grape".into(),
    };
    memory.next_folder_id += 1;
    memory.folders.push(folder.clone());
    Ok(folder)
}

#[tauri::command]
fn delete_folder_chat(id: u64, state: State<'_, AppState>) -> Result<bool, String> {
    let mut memory = state
        .memory
        .lock()
        .map_err(|_| "No fue posible acceder al estado".to_string())?;
    let previous_len = memory.folders.len();
    memory.folders.retain(|folder| folder.id != id);
    memory.messages.remove(&chat_key(Some(id)));
    Ok(memory.folders.len() != previous_len)
}

#[tauri::command]
fn get_messages(
    folder_id: Option<u64>,
    state: State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let memory = state
        .memory
        .lock()
        .map_err(|_| "No fue posible acceder al estado".to_string())?;
    Ok(memory
        .messages
        .get(&chat_key(folder_id))
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
fn send_message(
    folder_id: Option<u64>,
    content: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let clean_content = content.trim();
    if clean_content.is_empty() {
        return Err("El mensaje no puede estar vacío".into());
    }
    let mut memory = state
        .memory
        .lock()
        .map_err(|_| "No fue posible acceder al estado".to_string())?;
    let messages = memory.messages.entry(chat_key(folder_id)).or_default();
    messages.push(ChatMessage {
        role: "user".into(),
        content: clean_content.into(),
    });
    messages.push(ChatMessage {
        role: "assistant".into(),
        content: "Recibí tu mensaje desde el backend de MEMORIÓN. En esta primera fase puedo conservar la conversación mientras la aplicación permanezca abierta.".into(),
    });
    let result = messages.clone();
    if let Some(id) = folder_id {
        if let Some(folder) = memory.folders.iter_mut().find(|folder| folder.id == id) {
            folder.count = result.len() / 2;
        }
    }
    Ok(result)
}

#[tauri::command]
fn get_system_metrics(state: State<'_, AppState>) -> Result<SystemMetrics, String> {
    let mut system = state
        .system
        .lock()
        .map_err(|_| "No fue posible leer los recursos del sistema".to_string())?;
    system.refresh_cpu_usage();
    system.refresh_memory();
    Ok(SystemMetrics {
        cpu_percent: system.global_cpu_usage(),
        ram_used_bytes: system.used_memory(),
        ram_total_bytes: system.total_memory(),
    })
}

#[tauri::command]
fn get_database_status(database: State<'_, Database>) -> Result<DatabaseStatus, String> {
    database.status()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let database = Database::open(app.handle())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            app.manage(database);
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_folder_chats,
            create_folder_chat,
            delete_folder_chat,
            get_messages,
            send_message,
            get_system_metrics,
            get_database_status,
            database::list_session_messages,
            database::append_session_message,
            database::clear_session_messages,
            database::get_activity_metrics,
            database::create_folder,
            database::get_folder,
            database::list_folders,
            database::update_folder,
            database::delete_folder,
            database::create_document,
            database::get_document,
            database::list_documents,
            database::update_document,
            database::delete_document,
            database::attach_document,
            database::prepare_folder_scan,
            database::store_document_chunk,
            database::finish_document_indexing,
            database::finish_folder_scan,
            database::create_knowledge_origin,
            database::get_knowledge_origin,
            database::list_knowledge_origins,
            database::update_knowledge_origin,
            database::delete_knowledge_origin,
            database::create_knowledge_item,
            database::get_knowledge_item,
            database::list_knowledge_items,
            database::update_knowledge_item,
            database::delete_knowledge_item,
            database::list_recent_knowledge_logs,
            database::revise_knowledge_item,
            database::create_ai_model,
            database::get_ai_model,
            database::list_ai_models,
            database::update_ai_model,
            database::delete_ai_model,
            database::upsert_model_capability,
            database::get_model_capability,
            database::list_model_capabilities,
            database::delete_model_capability,
            database::create_model_assignment,
            database::get_model_assignment,
            database::list_model_assignments,
            database::update_model_assignment,
            database::delete_model_assignment,
            database::upsert_knowledge_vector,
            database::get_knowledge_vector,
            database::delete_knowledge_vector,
            database::search_knowledge,
            database::store_general_chat_knowledge
        ])
        .run(tauri::generate_context!())
        .expect("error while running MEMORIÓN");
}
