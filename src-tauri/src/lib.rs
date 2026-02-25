use chrono::{DateTime, Local};
use csv::Writer;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

// ============================================================
// データ構造 (Claude.md §4 準拠)
// ============================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeLog {
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub time_logs: Vec<TimeLog>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Operation {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AppData {
    pub operations: Vec<Operation>,
}

// ============================================================
// アプリ状態
// ============================================================

pub struct AppState {
    pub data: Mutex<AppData>,
    pub active_task_id: Mutex<Option<String>>,
    pub active_task_start: Mutex<Option<DateTime<Local>>>,
}

// ============================================================
// フロントエンドに返すビュー型
// ============================================================

#[derive(Serialize, Clone)]
pub struct ActiveTaskInfo {
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    pub operation_name: Option<String>,
    pub elapsed_seconds: Option<i64>,
}

#[derive(Serialize)]
pub struct AppStateView {
    pub operations: Vec<Operation>,
    pub active: ActiveTaskInfo,
}

// ============================================================
// 永続化ヘルパー
// ============================================================

fn data_file_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app_data_dir を取得できません")
        .join("data.json")
}

fn load_data(app: &AppHandle) -> AppData {
    let path = data_file_path(app);
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppData::default()
    }
}

fn save_data(app: &AppHandle, data: &AppData) -> Result<(), String> {
    let path = data_file_path(app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

// ============================================================
// 共通ヘルパー: アクティブタスク停止
// コマンドとグローバルショートカットハンドラで共用
// ============================================================

fn stop_task_inner(app: &AppHandle) -> Result<(), String> {
    let now = Local::now();
    let state = app.state::<AppState>();
    let mut data = state.data.lock().unwrap();
    let mut active_task_id = state.active_task_id.lock().unwrap();
    let mut active_task_start = state.active_task_start.lock().unwrap();

    if let Some(ref task_id) = active_task_id.clone() {
        for op in &mut data.operations {
            for task in &mut op.tasks {
                if &task.id == task_id {
                    if let Some(log) = task.time_logs.last_mut() {
                        if log.end_time.is_none() {
                            log.end_time = Some(now);
                        }
                    }
                }
            }
        }
    }

    *active_task_id = None;
    *active_task_start = None;

    save_data(app, &data)
}

// ============================================================
// Tauri コマンド
// ============================================================

/// 現在の全状態を返す
#[tauri::command]
fn get_state(state: State<'_, AppState>) -> AppStateView {
    let data = state.data.lock().unwrap();
    let active_task_id = state.active_task_id.lock().unwrap().clone();
    let active_task_start = state.active_task_start.lock().unwrap().clone();

    let active = if let Some(ref task_id) = active_task_id {
        let mut task_name = None;
        let mut operation_name = None;
        for op in &data.operations {
            if let Some(task) = op.tasks.iter().find(|t| &t.id == task_id) {
                task_name = Some(task.name.clone());
                operation_name = Some(op.name.clone());
                break;
            }
        }
        let elapsed_seconds = active_task_start
            .map(|start| (Local::now() - start).num_seconds());
        ActiveTaskInfo {
            task_id: Some(task_id.clone()),
            task_name,
            operation_name,
            elapsed_seconds,
        }
    } else {
        ActiveTaskInfo {
            task_id: None,
            task_name: None,
            operation_name: None,
            elapsed_seconds: None,
        }
    };

    AppStateView {
        operations: data.operations.clone(),
        active,
    }
}

/// タスク計測を開始する（既存アクティブタスクがあれば自動終了）
#[tauri::command]
fn start_task(
    app: AppHandle,
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let now = Local::now();
    let mut data = state.data.lock().unwrap();
    let mut active_task_id = state.active_task_id.lock().unwrap();
    let mut active_task_start = state.active_task_start.lock().unwrap();

    if let Some(ref current_id) = active_task_id.clone() {
        for op in &mut data.operations {
            for task in &mut op.tasks {
                if &task.id == current_id {
                    if let Some(log) = task.time_logs.last_mut() {
                        if log.end_time.is_none() {
                            log.end_time = Some(now);
                        }
                    }
                }
            }
        }
    }

    let mut found = false;
    for op in &mut data.operations {
        for task in &mut op.tasks {
            if task.id == task_id {
                task.time_logs.push(TimeLog {
                    start_time: now,
                    end_time: None,
                });
                found = true;
                break;
            }
        }
        if found {
            break;
        }
    }

    if !found {
        return Err(format!("タスク '{}' が見つかりません", task_id));
    }

    *active_task_id = Some(task_id);
    *active_task_start = Some(now);

    save_data(&app, &data)
}

/// アクティブタスクを停止する
#[tauri::command]
fn stop_active_task(app: AppHandle) -> Result<(), String> {
    stop_task_inner(&app)
}

/// オペレーションを追加する
#[tauri::command]
fn add_operation(
    app: AppHandle,
    name: String,
    description: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let id = format!("op_{}", Local::now().timestamp_millis());
    let mut data = state.data.lock().unwrap();
    data.operations.push(Operation {
        id: id.clone(),
        name,
        description,
        tasks: vec![],
    });
    save_data(&app, &data)?;
    Ok(id)
}

/// タスクをオペレーションに追加する
#[tauri::command]
fn add_task(
    app: AppHandle,
    operation_id: String,
    name: String,
    tag: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let id = format!("task_{}", Local::now().timestamp_millis());
    let mut data = state.data.lock().unwrap();
    let op = data
        .operations
        .iter_mut()
        .find(|o| o.id == operation_id)
        .ok_or_else(|| format!("オペレーション '{}' が見つかりません", operation_id))?;
    op.tasks.push(Task {
        id: id.clone(),
        name,
        tag,
        time_logs: vec![],
    });
    save_data(&app, &data)?;
    Ok(id)
}

/// 全タイムログを CSV に書き出してパスを返す
#[tauri::command]
fn export_csv(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    // ロック時間を最小化するためにデータをクローン
    let data = {
        let lock = state.data.lock().unwrap();
        lock.clone()
    };

    let export_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("exports");
    fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    let filename = format!("export_{}.csv", Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let file_path = export_dir.join(&filename);

    let mut wtr = Writer::from_path(&file_path).map_err(|e| e.to_string())?;

    wtr.write_record([
        "オペレーション",
        "タスク名",
        "タグ",
        "日付",
        "開始時刻",
        "終了時刻",
        "作業時間(分)",
    ])
    .map_err(|e| e.to_string())?;

    for op in &data.operations {
        for task in &op.tasks {
            for log in &task.time_logs {
                if let Some(end_time) = log.end_time {
                    let duration_min =
                        (end_time - log.start_time).num_seconds() as f64 / 60.0;
                    wtr.write_record([
                        op.name.as_str(),
                        task.name.as_str(),
                        task.tag.as_str(),
                        &log.start_time.format("%Y-%m-%d").to_string(),
                        &log.start_time.format("%H:%M:%S").to_string(),
                        &end_time.format("%H:%M:%S").to_string(),
                        &format!("{:.1}", duration_min),
                    ])
                    .map_err(|e| e.to_string())?;
                }
            }
        }
    }

    wtr.flush().map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().to_string())
}

/// ウィンドウの常時最前面を切り替える
#[tauri::command]
fn set_always_on_top(app: AppHandle, value: bool) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or_else(|| "ウィンドウが見つかりません".to_string())?
        .set_always_on_top(value)
        .map_err(|e| e.to_string())
}

// ============================================================
// エントリーポイント
// ============================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            // グローバルショートカット: Ctrl+Shift+S でアクティブタスクを停止
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if matches!(event.state, ShortcutState::Pressed) {
                        let _ = stop_task_inner(app);
                        let _ = app.emit("task-stopped", ());
                    }
                })
                .build(),
        )
        .setup(|app| {
            let data = load_data(app.handle());
            app.manage(AppState {
                data: Mutex::new(data),
                active_task_id: Mutex::new(None),
                active_task_start: Mutex::new(None),
            });

            // ショートカット登録 (失敗してもアプリは起動継続)
            if let Err(e) = app.global_shortcut().register("Ctrl+Shift+S") {
                eprintln!("グローバルショートカット登録失敗: {e}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            start_task,
            stop_active_task,
            add_operation,
            add_task,
            export_csv,
            set_always_on_top,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
