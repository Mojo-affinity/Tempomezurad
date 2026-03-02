use chrono::{DateTime, Local, NaiveDate};
use csv::Writer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

// ============================================================
// データ構造
// ============================================================

/// トランザクションデータ: 1 計測セッション分のログ
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeLog {
    pub task_id: String,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
}

/// マスターデータ: タスク定義（計測ログは含まない）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Operation {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub hidden: bool,
}

/// マスターデータファイル (master.json) に対応
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MasterData {
    pub operations: Vec<Operation>,
}

/// 日別ログファイル (logs/YYYY-MM-DD.json) に対応
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DailyLog {
    pub logs: Vec<TimeLog>,
}

// ============================================================
// アプリ状態
// ============================================================

pub struct AppState {
    pub data: Mutex<MasterData>,
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

/// ランチャーに渡す「最近使ったタスク」情報
#[derive(Serialize, Clone)]
pub struct RecentTaskInfo {
    pub task_id: String,
    pub task_name: String,
    pub operation_name: String,
    pub tag: String,
}

// ============================================================
// 永続化ヘルパー
// ============================================================

fn master_file_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app_data_dir を取得できません")
        .join("master.json")
}

fn log_file_path(app: &AppHandle, date: NaiveDate) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app_data_dir を取得できません")
        .join("logs")
        .join(format!("{}.json", date.format("%Y-%m-%d")))
}

fn load_master(app: &AppHandle) -> MasterData {
    let path = master_file_path(app);
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        MasterData::default()
    }
}

fn save_master(app: &AppHandle, data: &MasterData) -> Result<(), String> {
    let path = master_file_path(app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

fn load_daily_log(app: &AppHandle, date: NaiveDate) -> DailyLog {
    let path = log_file_path(app, date);
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        DailyLog::default()
    }
}

fn save_daily_log(app: &AppHandle, date: NaiveDate, log: &DailyLog) -> Result<(), String> {
    let path = log_file_path(app, date);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(log).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

// ============================================================
// 共通ヘルパー: アクティブタスク停止
// ============================================================

fn stop_task_inner(app: &AppHandle) -> Result<(), String> {
    let now = Local::now();
    let state = app.state::<AppState>();
    let mut active_task_id = state.active_task_id.lock().unwrap();
    let mut active_task_start = state.active_task_start.lock().unwrap();

    if let Some(current_id) = active_task_id.clone() {
        let today = now.date_naive();
        let mut daily_log = load_daily_log(app, today);
        if let Some(log) = daily_log
            .logs
            .iter_mut()
            .find(|l| l.task_id == current_id && l.end_time.is_none())
        {
            log.end_time = Some(now);
        }
        save_daily_log(app, today, &daily_log)?;
    }

    *active_task_id = None;
    *active_task_start = None;

    Ok(())
}

// ============================================================
// 共通ヘルパー: ランチャーウィンドウの表示
// ============================================================

fn show_launcher_window(app: &AppHandle) {
    // 既にランチャーウィンドウが存在する場合はフォーカスのみ
    if let Some(win) = app.get_webview_window("launcher") {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    // 透明・全画面・常時最前面のランチャーウィンドウを動的生成
    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "launcher",
        tauri::WebviewUrl::App(std::path::PathBuf::from("/")),
    )
    .transparent(true)
    .fullscreen(true)
    .always_on_top(true)
    .decorations(false)
    .skip_taskbar(true)
    .focused(true)
    .build();
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
        let elapsed_seconds = active_task_start.map(|start| (Local::now() - start).num_seconds());
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

/// ログ履歴を元に「最近使ったタスク順」でタスクリストを返す（ランチャー用）
#[tauri::command]
fn get_recent_tasks(app: AppHandle, state: State<'_, AppState>) -> Vec<RecentTaskInfo> {
    let data = state.data.lock().unwrap().clone();

    // task_id -> (task_name, operation_name, tag) のルックアップマップ
    let mut task_map: HashMap<String, (String, String, String)> = HashMap::new();
    for op in &data.operations {
        if op.hidden {
            continue;
        }
        for task in &op.tasks {
            if task.hidden {
                continue;
            }
            task_map.insert(
                task.id.clone(),
                (task.name.clone(), op.name.clone(), task.tag.clone()),
            );
        }
    }

    // logs/ ディレクトリを走査して task_id ごとの最終使用時刻を収集
    let app_data_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return build_fallback_list(&data),
    };
    let logs_dir = app_data_dir.join("logs");

    let mut task_last_used: HashMap<String, DateTime<Local>> = HashMap::new();
    if logs_dir.exists() {
        if let Ok(entries) = fs::read_dir(&logs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    if let Ok(daily_log) = serde_json::from_str::<DailyLog>(&content) {
                        for log in daily_log.logs {
                            let recorded = task_last_used
                                .entry(log.task_id.clone())
                                .or_insert(log.start_time);
                            if log.start_time > *recorded {
                                *recorded = log.start_time;
                            }
                        }
                    }
                }
            }
        }
    }

    // 最終使用時刻で降順ソート
    let mut sorted: Vec<(String, DateTime<Local>)> = task_last_used.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    // ログ履歴があるタスクを先に並べる
    let mut result: Vec<RecentTaskInfo> = sorted
        .into_iter()
        .filter_map(|(task_id, _)| {
            task_map.get(&task_id).map(|(name, op_name, tag)| RecentTaskInfo {
                task_id: task_id.clone(),
                task_name: name.clone(),
                operation_name: op_name.clone(),
                tag: tag.clone(),
            })
        })
        .collect();

    // ログがまだないタスクをマスター定義順で末尾に追加
    for op in &data.operations {
        if op.hidden {
            continue;
        }
        for task in &op.tasks {
            if task.hidden {
                continue;
            }
            if !result.iter().any(|r| r.task_id == task.id) {
                result.push(RecentTaskInfo {
                    task_id: task.id.clone(),
                    task_name: task.name.clone(),
                    operation_name: op.name.clone(),
                    tag: task.tag.clone(),
                });
            }
        }
    }

    result
}

/// ログ履歴がない場合のフォールバック（マスター定義順）
fn build_fallback_list(data: &MasterData) -> Vec<RecentTaskInfo> {
    let mut result = vec![];
    for op in &data.operations {
        if op.hidden {
            continue;
        }
        for task in &op.tasks {
            if task.hidden {
                continue;
            }
            result.push(RecentTaskInfo {
                task_id: task.id.clone(),
                task_name: task.name.clone(),
                operation_name: op.name.clone(),
                tag: task.tag.clone(),
            });
        }
    }
    result
}

/// タスク計測を開始する（既存アクティブタスクがあれば自動終了）
#[tauri::command]
fn start_task(
    app: AppHandle,
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let now = Local::now();

    // タスク存在確認 (data ロックは短期間で解放)
    {
        let data = state.data.lock().unwrap();
        let found = data
            .operations
            .iter()
            .any(|op| op.tasks.iter().any(|t| t.id == task_id));
        if !found {
            return Err(format!("タスク '{}' が見つかりません", task_id));
        }
    }

    let mut active_task_id = state.active_task_id.lock().unwrap();
    let mut active_task_start = state.active_task_start.lock().unwrap();

    let today = now.date_naive();
    let mut daily_log = load_daily_log(&app, today);

    // 既存アクティブタスクを終了
    if let Some(current_id) = active_task_id.clone() {
        if let Some(log) = daily_log
            .logs
            .iter_mut()
            .find(|l| l.task_id == current_id && l.end_time.is_none())
        {
            log.end_time = Some(now);
        }
    }

    // 新しいログを追加
    daily_log.logs.push(TimeLog {
        task_id: task_id.clone(),
        start_time: now,
        end_time: None,
    });

    save_daily_log(&app, today, &daily_log)?;

    *active_task_id = Some(task_id);
    *active_task_start = Some(now);

    // 全ウィンドウに状態変更を通知（メインウィンドウが自動 sync する）
    let _ = app.emit("state-changed", ());

    Ok(())
}

/// アクティブタスクを停止する
#[tauri::command]
fn stop_active_task(app: AppHandle) -> Result<(), String> {
    stop_task_inner(&app)?;
    let _ = app.emit("state-changed", ());
    Ok(())
}

/// ランチャーウィンドウを閉じる
#[tauri::command]
fn close_launcher(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("launcher") {
        win.close().map_err(|e| e.to_string())?;
    }
    Ok(())
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
        hidden: false,
    });
    save_master(&app, &data)?;
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
        hidden: false,
    });
    save_master(&app, &data)?;
    Ok(id)
}

/// オペレーションの順序を変更する（隣接要素とスワップ）
#[tauri::command]
fn reorder_operation(
    app: AppHandle,
    op_id: String,
    direction: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut data = state.data.lock().unwrap();
    let ops = &mut data.operations;
    let idx = ops
        .iter()
        .position(|o| o.id == op_id)
        .ok_or_else(|| format!("オペレーション '{}' が見つかりません", op_id))?;
    match direction.as_str() {
        "up" if idx > 0 => ops.swap(idx, idx - 1),
        "down" if idx + 1 < ops.len() => ops.swap(idx, idx + 1),
        _ => {}
    }
    save_master(&app, &data)
}

/// タスクの順序を変更する（隣接要素とスワップ）
#[tauri::command]
fn reorder_task(
    app: AppHandle,
    op_id: String,
    task_id: String,
    direction: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut data = state.data.lock().unwrap();
    let op = data
        .operations
        .iter_mut()
        .find(|o| o.id == op_id)
        .ok_or_else(|| format!("オペレーション '{}' が見つかりません", op_id))?;
    let idx = op
        .tasks
        .iter()
        .position(|t| t.id == task_id)
        .ok_or_else(|| format!("タスク '{}' が見つかりません", task_id))?;
    match direction.as_str() {
        "up" if idx > 0 => op.tasks.swap(idx, idx - 1),
        "down" if idx + 1 < op.tasks.len() => op.tasks.swap(idx, idx + 1),
        _ => {}
    }
    save_master(&app, &data)
}

/// オペレーションの表示/非表示を切り替える
#[tauri::command]
fn toggle_operation_visibility(
    app: AppHandle,
    op_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut data = state.data.lock().unwrap();
    let op = data
        .operations
        .iter_mut()
        .find(|o| o.id == op_id)
        .ok_or_else(|| format!("オペレーション '{}' が見つかりません", op_id))?;
    op.hidden = !op.hidden;
    save_master(&app, &data)
}

/// タスクの表示/非表示を切り替える
#[tauri::command]
fn toggle_task_visibility(
    app: AppHandle,
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut data = state.data.lock().unwrap();
    for op in &mut data.operations {
        if let Some(task) = op.tasks.iter_mut().find(|t| t.id == task_id) {
            task.hidden = !task.hidden;
            return save_master(&app, &data);
        }
    }
    Err(format!("タスク '{}' が見つかりません", task_id))
}

/// logs/ 内の全ログを CSV に書き出してパスを返す
#[tauri::command]
fn export_csv(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let data = {
        let lock = state.data.lock().unwrap();
        lock.clone()
    };

    // タスク検索マップを構築: task_id -> (operation_name, task_name, tag)
    let mut task_map: HashMap<String, (String, String, String)> = HashMap::new();
    for op in &data.operations {
        for task in &op.tasks {
            task_map.insert(
                task.id.clone(),
                (op.name.clone(), task.name.clone(), task.tag.clone()),
            );
        }
    }

    // logs/ ディレクトリのすべての JSON ファイルを読み込む
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let logs_dir = app_data_dir.join("logs");
    let mut all_logs: Vec<TimeLog> = vec![];
    if logs_dir.exists() {
        for entry in fs::read_dir(&logs_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(&path).unwrap_or_default();
                if let Ok(daily_log) = serde_json::from_str::<DailyLog>(&content) {
                    all_logs.extend(daily_log.logs);
                }
            }
        }
    }

    // 開始時刻でソート
    all_logs.sort_by_key(|l| l.start_time);

    let export_dir = app_data_dir.join("exports");
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

    for log in &all_logs {
        if let Some(end_time) = log.end_time {
            let duration_min = (end_time - log.start_time).num_seconds() as f64 / 60.0;
            let (op_name, task_name, tag) = match task_map.get(&log.task_id) {
                Some(t) => (t.0.as_str(), t.1.as_str(), t.2.as_str()),
                None => ("(不明)", "(不明)", ""),
            };
            wtr.write_record([
                op_name,
                task_name,
                tag,
                &log.start_time.format("%Y-%m-%d").to_string(),
                &log.start_time.format("%H:%M:%S").to_string(),
                &end_time.format("%H:%M:%S").to_string(),
                &format!("{:.1}", duration_min),
            ])
            .map_err(|e| e.to_string())?;
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

/// カスタムタイトルバーからのドラッグ開始
#[tauri::command]
fn start_dragging(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or_else(|| "ウィンドウが見つかりません".to_string())?
        .start_dragging()
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
            // グローバルショートカット: Ctrl+Shift+Space でランチャーを表示
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if matches!(event.state, ShortcutState::Pressed) {
                        show_launcher_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let data = load_master(app.handle());
            app.manage(AppState {
                data: Mutex::new(data),
                active_task_id: Mutex::new(None),
                active_task_start: Mutex::new(None),
            });

            // ショートカット登録 (失敗してもアプリは起動継続)
            if let Err(e) = app.global_shortcut().register("Ctrl+Shift+Space") {
                eprintln!("グローバルショートカット登録失敗: {e}");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_recent_tasks,
            start_task,
            stop_active_task,
            close_launcher,
            add_operation,
            add_task,
            export_csv,
            set_always_on_top,
            start_dragging,
            reorder_operation,
            reorder_task,
            toggle_operation_visibility,
            toggle_task_visibility,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
