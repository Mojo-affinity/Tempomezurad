use chrono::{DateTime, Duration, Local, NaiveDate};
use csv::Writer;
use reqwest::{redirect::Policy, Client, Url};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error as StdError;
use std::fs::{self, OpenOptions};
use std::io::Write;
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
    #[serde(default)]
    pub manhour_project_code: String,
    #[serde(default)]
    pub manhour_task_code: String,
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
    /// task_id -> 当日の合計計測秒数（実行中セッションを含む）
    pub today_seconds: HashMap<String, i64>,
}

/// ランチャーに渡す「最近使ったタスク」情報
#[derive(Serialize, Clone)]
pub struct RecentTaskInfo {
    pub task_id: String,
    pub task_name: String,
    pub operation_name: String,
    pub tag: String,
}

/// 履歴画面に返す、タスク情報で補完済みの計測ログ
#[derive(Serialize, Clone)]
pub struct HistoryEntry {
    pub id: String,
    pub task_id: String,
    pub task_name: String,
    pub operation_name: String,
    pub tag: String,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
    pub duration_seconds: Option<i64>,
    pub is_active: bool,
}

#[derive(Serialize, Clone)]
pub struct ManhourPreviewEntry {
    pub operation_name: String,
    pub project_code: String,
    pub task_code: String,
    pub minutes: i64,
    pub time_text: String,
    pub comment: String,
}

#[derive(Serialize)]
pub struct ManhourPreview {
    pub date: String,
    pub entries: Vec<ManhourPreviewEntry>,
    pub total_minutes: i64,
    pub attendance_work_minutes: Option<i64>,
    pub difference_minutes: Option<i64>,
    pub unmapped_operations: Vec<String>,
    pub has_unfinished_logs: bool,
}

#[derive(Deserialize, Clone)]
pub struct ManhourSubmissionEntry {
    pub operation_name: String,
    pub project_code: String,
    pub task_code: String,
    pub minutes: i64,
    #[serde(default)]
    pub comment: String,
}

#[derive(Serialize)]
pub struct ManhourSubmissionResult {
    pub date: String,
    pub submitted_count: usize,
    pub total_minutes: i64,
}

#[derive(Serialize)]
pub struct ConnectionDiagnosticsView {
    pub path: String,
    pub content: String,
}

/// 勤怠サイトから取得した日別の勤務実績。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AttendanceDay {
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub break_minutes: Option<i64>,
    pub work_minutes: Option<i64>,
    pub status: Option<String>,
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

fn attendance_file_path(app: &AppHandle, date: NaiveDate) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app_data_dir を取得できません")
        .join("attendance")
        .join(format!("{}.json", date.format("%Y-%m-%d")))
}

fn save_attendance_day(app: &AppHandle, day: &AttendanceDay) -> Result<(), String> {
    let date = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
        .map_err(|error| format!("勤怠の日付を解釈できません: {error}"))?;
    let path = attendance_file_path(app, date);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(day).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn load_attendance_days(app: &AppHandle) -> BTreeMap<String, AttendanceDay> {
    let Ok(app_data_dir) = app.path().app_data_dir() else {
        return BTreeMap::new();
    };
    let dir = app_data_dir.join("attendance");
    let Ok(entries) = fs::read_dir(dir) else {
        return BTreeMap::new();
    };

    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|content| serde_json::from_str::<AttendanceDay>(&content).ok())
        .map(|day| (day.date.clone(), day))
        .collect()
}

struct AttendanceConfig {
    login_url: String,
    company_id: String,
    employee_id: String,
    password: String,
    attendance_url: String,
    manhour_url: String,
    certificate_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredAttendanceSettings {
    login_url: String,
    company_id: String,
    employee_id: String,
    attendance_url: String,
    #[serde(default)]
    manhour_url: String,
    #[serde(default)]
    certificate_path: String,
}

#[derive(Serialize)]
struct AttendanceSettingsView {
    login_url: String,
    company_id: String,
    employee_id: String,
    attendance_url: String,
    manhour_url: String,
    certificate_path: String,
    password_saved: bool,
    source: String,
}

const ATTENDANCE_CREDENTIAL_SERVICE: &str = "com.ubuntu.tempomezurado";
const ATTENDANCE_CREDENTIAL_USER: &str = "attendance-login";
const CONNECTION_LOG_MAX_BYTES: u64 = 512 * 1024;

#[derive(Clone)]
struct ConnectionLogger {
    path: PathBuf,
}

impl ConnectionLogger {
    fn new(app: &AppHandle) -> Result<Self, String> {
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("診断ログの保存先を取得できません: {error}"))?
            .join("diagnostics");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("診断ログの保存先を作成できません: {error}"))?;
        Ok(Self {
            path: directory.join("connection.log"),
        })
    }

    fn write(&self, level: &str, message: impl AsRef<str>) {
        if fs::metadata(&self.path)
            .map(|metadata| metadata.len() >= CONNECTION_LOG_MAX_BYTES)
            .unwrap_or(false)
        {
            let _ = fs::write(&self.path, "");
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(file, "[{timestamp}] [{level}] {}", message.as_ref());
        }
    }

    fn info(&self, message: impl AsRef<str>) {
        self.write("INFO", message);
    }

    fn error(&self, message: impl AsRef<str>) {
        self.write("ERROR", message);
    }
}

fn endpoint_for_log(value: &str) -> String {
    Url::parse(value)
        .map(|url| {
            let host = url.host_str().unwrap_or("(hostなし)");
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            format!("{}://{host}{port}{}", url.scheme(), url.path())
        })
        .unwrap_or_else(|_| "(URLを解釈できません)".to_string())
}

fn reqwest_error_for_log(error: &reqwest::Error) -> String {
    let mut properties = Vec::new();
    if error.is_connect() {
        properties.push("connect");
    }
    if error.is_timeout() {
        properties.push("timeout");
    }
    if error.is_redirect() {
        properties.push("redirect");
    }
    if error.is_status() {
        properties.push("status");
    }
    let kind = if properties.is_empty() {
        "other".to_string()
    } else {
        properties.join(",")
    };
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !causes.iter().any(|existing| existing == &text) {
            causes.push(text);
        }
        source = cause.source();
    }
    if causes.is_empty() {
        format!("種別={kind}; {error}")
    } else {
        format!("種別={kind}; {error}; 原因={}", causes.join(" -> "))
    }
}

fn request_error(logger: Option<&ConnectionLogger>, stage: &str, error: reqwest::Error) -> String {
    let endpoint = error
        .url()
        .map(|url| endpoint_for_log(url.as_str()))
        .unwrap_or_else(|| "(URLなし)".to_string());
    let error = error.without_url();
    let details = reqwest_error_for_log(&error);
    if let Some(logger) = logger {
        logger.error(format!("{stage}: endpoint={endpoint}; {details}"));
    }
    format!("{stage}。接続診断ログを確認してください: {error}")
}

fn attendance_settings_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?
        .join("attendance.json"))
}

fn credential_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(ATTENDANCE_CREDENTIAL_SERVICE, ATTENDANCE_CREDENTIAL_USER)
        .map_err(|error| format!("Windows資格情報を開けません: {error}"))
}

fn load_stored_attendance_settings(
    app: &AppHandle,
) -> Result<Option<StoredAttendanceSettings>, String> {
    let path = attendance_settings_file_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("勤怠設定を読み込めません: {error}"))?;
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| format!("勤怠設定を解釈できません: {error}"))
}

fn parse_attendance_config(content: &str) -> Result<AttendanceConfig, String> {
    let values: HashMap<String, String> = content
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=')
                .or_else(|| line.split_once(':'))
                .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        })
        .collect();

    let required = |key: &str| {
        values
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| format!("login.txt に「{key}」がありません"))
    };

    Ok(AttendanceConfig {
        login_url: required("url")?,
        company_id: required("企業ID")?,
        employee_id: required("従業員番号")?,
        password: required("パスワード")?,
        attendance_url: required("出勤簿")?,
        manhour_url: required("工数")?,
        certificate_path: values
            .get("証明書")
            .or_else(|| values.get("証明書パス"))
            .cloned()
            .unwrap_or_default(),
    })
}

fn load_attendance_config(app: &AppHandle) -> Result<AttendanceConfig, String> {
    if let Some(settings) = load_stored_attendance_settings(app)? {
        let manhour_url = if settings.manhour_url.is_empty() {
            find_login_file(app)
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|content| parse_attendance_config(&content).ok())
                .map(|config| config.manhour_url)
                .unwrap_or_default()
        } else {
            settings.manhour_url.clone()
        };
        let password = credential_entry()?.get_password().map_err(|_| {
            "保存済みのパスワードを取得できません。勤怠設定から再入力してください".to_string()
        })?;
        return Ok(AttendanceConfig {
            login_url: settings.login_url,
            company_id: settings.company_id,
            employee_id: settings.employee_id,
            password,
            attendance_url: settings.attendance_url,
            manhour_url,
            certificate_path: settings.certificate_path,
        });
    }

    let config_path = find_login_file(app)?;
    let config_content = fs::read_to_string(config_path)
        .map_err(|error| format!("login.txt を読み込めません: {error}"))?;
    parse_attendance_config(&config_content)
}

fn find_login_file(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("TEMPOMEZURADO_LOGIN_FILE") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(path) = std::env::current_dir() {
        candidates.push(path.join("login.txt"));
    }
    if let Ok(path) = std::env::current_exe() {
        if let Some(parent) = path.parent() {
            candidates.push(parent.join("login.txt"));
        }
    }
    if let Ok(path) = app.path().app_config_dir() {
        candidates.push(path.join("login.txt"));
    }

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "login.txt が見つかりません。アプリの作業フォルダー、実行ファイルと同じフォルダー、またはアプリ設定フォルダーに配置してください".to_string()
        })
}

fn text_of(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_duration_minutes(value: &str) -> Option<i64> {
    let (hours, minutes) = value.trim().split_once(':')?;
    Some(hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?)
}

fn parse_clock_minutes(value: &str) -> Option<i64> {
    if value == "--:--" {
        return None;
    }
    parse_duration_minutes(value)
}

fn parse_manhour_operation(name: &str) -> Option<(String, String)> {
    let (prefix, number) = name.trim().split_once('-')?;
    if prefix.is_empty()
        || number.len() != 3
        || !number.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let project_digit = number.chars().next()?;
    Some((format!("{prefix}-{project_digit}"), name.trim().to_string()))
}

fn operation_manhour_mapping(operation: &Operation) -> Option<(String, String)> {
    let project_code = operation.manhour_project_code.trim();
    let task_code = operation.manhour_task_code.trim();
    if !project_code.is_empty() && !task_code.is_empty() {
        Some((project_code.to_string(), task_code.to_string()))
    } else {
        parse_manhour_operation(&operation.name)
    }
}

fn format_manhour_minutes(minutes: i64) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn allocate_minutes_by_seconds(
    seconds_by_operation: &BTreeMap<String, i64>,
    target_minutes: i64,
) -> HashMap<String, i64> {
    let total_seconds: i64 = seconds_by_operation.values().sum();
    if total_seconds <= 0 || target_minutes <= 0 {
        return HashMap::new();
    }
    let mut allocations: Vec<(String, i64, i64)> = seconds_by_operation
        .iter()
        .map(|(operation, seconds)| {
            let weighted = target_minutes * *seconds;
            (
                operation.clone(),
                weighted / total_seconds,
                weighted % total_seconds,
            )
        })
        .collect();
    let allocated: i64 = allocations.iter().map(|(_, minutes, _)| *minutes).sum();
    let mut remaining = target_minutes - allocated;
    allocations.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
    for (_, minutes, _) in &mut allocations {
        if remaining <= 0 {
            break;
        }
        *minutes += 1;
        remaining -= 1;
    }
    allocations
        .into_iter()
        .map(|(operation, minutes, _)| (operation, minutes))
        .collect()
}

fn build_manhour_preview(
    master: &MasterData,
    daily_log: &DailyLog,
    date: NaiveDate,
    attendance: Option<&AttendanceDay>,
) -> ManhourPreview {
    let operation_by_task: HashMap<&str, (&str, &str)> = master
        .operations
        .iter()
        .flat_map(|operation| {
            operation.tasks.iter().map(move |task| {
                (
                    task.id.as_str(),
                    (operation.name.as_str(), task.name.as_str()),
                )
            })
        })
        .collect();
    let mapping_by_operation: HashMap<&str, Option<(String, String)>> = master
        .operations
        .iter()
        .map(|operation| {
            (
                operation.name.as_str(),
                operation_manhour_mapping(operation),
            )
        })
        .collect();
    let mut seconds_by_operation: BTreeMap<String, i64> = BTreeMap::new();
    let mut task_names_by_operation: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut has_unfinished_logs = false;
    for log in &daily_log.logs {
        let Some((operation_name, task_name)) = operation_by_task.get(log.task_id.as_str()) else {
            continue;
        };
        let Some(end_time) = log.end_time else {
            has_unfinished_logs = true;
            continue;
        };
        let measured_seconds = (end_time - log.start_time).num_seconds().max(0);
        *seconds_by_operation
            .entry((*operation_name).to_string())
            .or_insert(0) += measured_seconds;
        if measured_seconds > 0 {
            let task_names = task_names_by_operation
                .entry((*operation_name).to_string())
                .or_default();
            if !task_names.iter().any(|name| name.as_str() == *task_name) {
                task_names.push((*task_name).to_string());
            }
        }
    }

    let attendance_work_minutes = attendance.and_then(|day| day.work_minutes);
    let allocated_minutes = attendance_work_minutes
        .map(|work| allocate_minutes_by_seconds(&seconds_by_operation, work))
        .unwrap_or_default();
    let mut entries = Vec::new();
    let mut unmapped_operations = Vec::new();
    for (operation_name, seconds) in seconds_by_operation {
        if let Some((project_code, task_code)) = mapping_by_operation
            .get(operation_name.as_str())
            .cloned()
            .flatten()
        {
            let minutes = attendance_work_minutes
                .and_then(|_| allocated_minutes.get(&operation_name).copied())
                .unwrap_or_else(|| ((seconds + 30) / 60).max(0));
            if minutes > 0 {
                let comment = task_names_by_operation
                    .get(&operation_name)
                    .cloned()
                    .unwrap_or_default()
                    .join("\n");
                entries.push(ManhourPreviewEntry {
                    operation_name,
                    project_code,
                    task_code,
                    minutes,
                    time_text: format_manhour_minutes(minutes),
                    comment,
                });
            }
        } else if seconds > 0 {
            unmapped_operations.push(operation_name);
        }
    }
    let total_minutes = entries.iter().map(|entry| entry.minutes).sum();
    let difference_minutes = attendance_work_minutes.map(|work| work - total_minutes);

    ManhourPreview {
        date: date.format("%Y-%m-%d").to_string(),
        entries,
        total_minutes,
        attendance_work_minutes,
        difference_minutes,
        unmapped_operations,
        has_unfinished_logs,
    }
}

fn parse_attendance_html(html: &str, date: NaiveDate) -> Result<AttendanceDay, String> {
    let document = Html::parse_document(html);
    let table_selector = Selector::parse("table").unwrap();
    // ブラウザーは tbody を補完するが、サーバーHTMLには存在しない場合がある。
    let row_selector = Selector::parse("tr").unwrap();
    let cell_selector = Selector::parse("td").unwrap();

    let mut date_rows = None;
    let mut detail_rows = None;
    for table in document.select(&table_selector) {
        let table_text = text_of(table);
        let rows: Vec<Vec<String>> = table
            .select(&row_selector)
            .map(|row| row.select(&cell_selector).map(text_of).collect())
            .filter(|cells: &Vec<String>| !cells.is_empty())
            .collect();
        if table_text.contains("日付") && !table_text.contains("集計(出)") {
            date_rows = Some(rows);
        } else if table_text.contains("集計(出)") && table_text.contains("休憩時間") {
            detail_rows = Some(rows);
        }
    }

    let dates = date_rows.ok_or_else(|| "出勤簿の日付一覧を取得できませんでした".to_string())?;
    let details =
        detail_rows.ok_or_else(|| "出勤簿の勤務実績を取得できませんでした".to_string())?;
    let target = date.format("%m/%d").to_string();
    let index = dates
        .iter()
        .position(|cells| {
            cells
                .first()
                .is_some_and(|value| value.starts_with(&target))
        })
        .ok_or_else(|| format!("{target} の勤怠が見つかりません"))?;
    let cells = details
        .get(index)
        .ok_or_else(|| "出勤簿の日付と勤務実績の行数が一致しません".to_string())?;
    if cells.len() < 8 {
        return Err("出勤簿の列構成を解釈できませんでした".to_string());
    }

    let aggregate: Vec<&str> = cells[3]
        .split_whitespace()
        .filter(|value| value.contains(':'))
        .collect();
    let start_time = aggregate
        .first()
        .filter(|value| **value != "--:--")
        .map(|value| (*value).to_string());
    let end_time = aggregate
        .get(1)
        .filter(|value| **value != "--:--")
        .map(|value| (*value).to_string());
    let break_minutes = parse_duration_minutes(&cells[7]);
    let site_work_minutes = parse_duration_minutes(&cells[5]);
    let calculated_work_minutes = start_time
        .as_deref()
        .and_then(parse_clock_minutes)
        .zip(end_time.as_deref().and_then(parse_clock_minutes))
        .map(|(start, end)| (end - start).max(0) - break_minutes.unwrap_or(0))
        .map(|minutes| minutes.max(0));

    Ok(AttendanceDay {
        date: date.format("%Y-%m-%d").to_string(),
        start_time,
        end_time,
        break_minutes,
        work_minutes: site_work_minutes.or(calculated_work_minutes),
        status: cells
            .get(4)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

fn attendance_url_for_date(base: &str, date: NaiveDate) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|error| format!("出勤簿URLが不正です: {error}"))?;
    let mut segments: Vec<String> = url
        .path_segments()
        .ok_or_else(|| "出勤簿URLを解釈できません".to_string())?
        .map(str::to_string)
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments
        .last()
        .is_some_and(|segment| segment.len() == 6 && segment.chars().all(|c| c.is_ascii_digit()))
    {
        segments.pop();
    }
    segments.push(date.format("%Y%m").to_string());
    url.set_path(&format!("/{}", segments.join("/")));
    Ok(url)
}

fn decode_javascript_string(script: &str) -> Option<String> {
    let (position, call_len) = script
        .find(".html(")
        .map(|position| (position, ".html(".len()))
        .or_else(|| {
            script
                .find(".replaceWith(")
                .map(|position| (position, ".replaceWith(".len()))
        })?;
    let start = position + call_len;
    let input = script[start..].trim_start();
    let quote = input.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let mut chars = input[quote.len_utf8()..].chars();
    let mut output = String::new();
    while let Some(ch) = chars.next() {
        if ch == quote {
            return Some(output);
        }
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next()? {
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'b' => output.push('\u{0008}'),
            'f' => output.push('\u{000c}'),
            'u' => {
                let digits: String = chars.by_ref().take(4).collect();
                output.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            'x' => {
                let digits: String = chars.by_ref().take(2).collect();
                output.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            escaped => output.push(escaped),
        }
    }
    None
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

/// 完了ログと現在アクティブなログだけに、安全な計測秒数を与える。
/// end_time のない孤立ログは None とし、現在時刻まで増え続けないようにする。
fn measured_duration_seconds(
    log: &TimeLog,
    active_task_id: Option<&str>,
    active_task_start: Option<&DateTime<Local>>,
    now: DateTime<Local>,
) -> Option<i64> {
    let end = match log.end_time {
        Some(end) => end,
        None if active_task_id == Some(log.task_id.as_str())
            && active_task_start == Some(&log.start_time) =>
        {
            now
        }
        None => return None,
    };
    Some((end - log.start_time).num_seconds().max(0))
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
        // 日付をまたいだ場合も、開始日のログファイルを確実に閉じる。
        let log_date = active_task_start
            .as_ref()
            .map(|start| start.date_naive())
            .unwrap_or_else(|| now.date_naive());
        let mut daily_log = load_daily_log(app, log_date);
        if let Some(log) = daily_log
            .logs
            .iter_mut()
            .rev()
            .find(|l| l.task_id == current_id && l.end_time.is_none())
        {
            log.end_time = Some(now);
        }
        save_daily_log(app, log_date, &daily_log)?;
    }

    *active_task_id = None;
    *active_task_start = None;

    Ok(())
}

// ============================================================
// 共通ヘルパー: ランチャーウィンドウの表示
// ============================================================

fn show_launcher_window(app: &AppHandle) {
    // 1. 先に show() + set_focus() でウィンドウを可視化・OS フォーカスを付与する。
    //    hidden な WebView へのイベント送信は到達が不安定なため、
    //    表示してから emit することで JS が確実にイベントを受け取れるようにする。
    // 2. emit("show-launcher") を受けた JS 側がタスクを取得し、
    //    rootRef.focus() で JS ドキュメントレベルのフォーカスを完成させる。
    if let Some(win) = app.get_webview_window("launcher") {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit("show-launcher", ());
    }
}

// ============================================================
// Tauri コマンド
// ============================================================

/// 現在の全状態を返す
#[tauri::command]
fn get_state(app: AppHandle, state: State<'_, AppState>) -> AppStateView {
    let now = Local::now();
    let data = state.data.lock().unwrap();
    let active_task_id = state.active_task_id.lock().unwrap().clone();
    let active_task_start = state.active_task_start.lock().unwrap().clone();

    // 今日の日別ログからタスクごとの合計秒数を集計する。
    // 未終了ログのうち「現在のアクティブログ」だけを now まで積算し、
    // 過去の孤立ログが時間を増やし続けることを防ぐ。
    let daily_log = load_daily_log(&app, now.date_naive());
    let mut today_seconds: HashMap<String, i64> = HashMap::new();
    for log in &daily_log.logs {
        if let Some(seconds) = measured_duration_seconds(
            log,
            active_task_id.as_deref(),
            active_task_start.as_ref(),
            now,
        ) {
            *today_seconds.entry(log.task_id.clone()).or_insert(0) += seconds;
        }
    }

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
        let elapsed_seconds = active_task_start.map(|start| (now - start).num_seconds());
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
        today_seconds,
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
            task_map
                .get(&task_id)
                .map(|(name, op_name, tag)| RecentTaskInfo {
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

/// 計測履歴を開始時刻の新しい順で返す。
#[tauri::command]
fn get_history(
    app: AppHandle,
    days: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryEntry>, String> {
    let data = state.data.lock().unwrap().clone();
    let active_task_id = state.active_task_id.lock().unwrap().clone();
    let active_task_start = state.active_task_start.lock().unwrap().clone();
    let now = Local::now();
    let cutoff = days
        .filter(|value| *value > 0)
        .map(|value| now - Duration::days(value));

    let mut task_map: HashMap<String, (String, String, String)> = HashMap::new();
    for operation in &data.operations {
        for task in &operation.tasks {
            task_map.insert(
                task.id.clone(),
                (task.name.clone(), operation.name.clone(), task.tag.clone()),
            );
        }
    }

    let logs_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("logs");
    let mut history = Vec::new();

    if logs_dir.exists() {
        for entry in fs::read_dir(&logs_dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap_or_default();
            let daily_log = match serde_json::from_str::<DailyLog>(&content) {
                Ok(log) => log,
                Err(_) => continue,
            };

            for log in daily_log.logs {
                // 未終了ログは期間外でも必ず返し、ユーザーが見失わないようにする。
                if log.end_time.is_some() && cutoff.is_some_and(|value| log.start_time < value) {
                    continue;
                }
                let is_active = active_task_id.as_ref() == Some(&log.task_id)
                    && active_task_start.as_ref() == Some(&log.start_time);
                let duration_seconds = measured_duration_seconds(
                    &log,
                    active_task_id.as_deref(),
                    active_task_start.as_ref(),
                    now,
                );
                let (task_name, operation_name, tag) =
                    task_map.get(&log.task_id).cloned().unwrap_or_else(|| {
                        (
                            "(削除済みタスク)".to_string(),
                            "(不明)".to_string(),
                            String::new(),
                        )
                    });

                history.push(HistoryEntry {
                    id: format!("{}|{}", log.task_id, log.start_time.to_rfc3339()),
                    task_id: log.task_id,
                    task_name,
                    operation_name,
                    tag,
                    start_time: log.start_time,
                    end_time: log.end_time,
                    duration_seconds,
                    is_active,
                });
            }
        }
    }

    history.sort_by(|left, right| right.start_time.cmp(&left.start_time));
    Ok(history)
}

/// 未終了の孤立ログを、ユーザーが選んだ方法で解消する。
#[tauri::command]
fn resolve_unfinished_log(
    app: AppHandle,
    task_id: String,
    start_time: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let parsed_start = DateTime::parse_from_rfc3339(&start_time)
        .map_err(|error| format!("開始時刻を解釈できません: {error}"))?
        .with_timezone(&Local);

    let active_task_id = state.active_task_id.lock().unwrap().clone();
    let active_task_start = state.active_task_start.lock().unwrap().clone();
    if active_task_id.as_ref() == Some(&task_id)
        && active_task_start.as_ref() == Some(&parsed_start)
    {
        return Err("計測中のログは履歴から変更できません".to_string());
    }

    let log_date = parsed_start.date_naive();
    let mut daily_log = load_daily_log(&app, log_date);
    let index = daily_log
        .logs
        .iter()
        .position(|log| {
            log.task_id == task_id && log.start_time == parsed_start && log.end_time.is_none()
        })
        .ok_or_else(|| "対象の未終了ログが見つかりません".to_string())?;

    match action.as_str() {
        "discard" => {
            daily_log.logs.remove(index);
        }
        "close_now" => {
            let now = Local::now();
            if now < parsed_start {
                return Err("開始時刻より前には終了できません".to_string());
            }
            daily_log.logs[index].end_time = Some(now);
        }
        _ => return Err("未対応の解消方法です".to_string()),
    }

    save_daily_log(&app, log_date, &daily_log)?;
    let _ = app.emit("history-changed", ());
    let _ = app.emit("state-changed", ());
    Ok(())
}

/// タスク計測を開始する（既存アクティブタスクがあれば自動終了）
#[tauri::command]
fn start_task(app: AppHandle, task_id: String, state: State<'_, AppState>) -> Result<(), String> {
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

    // 既存タスクは開始日側のログを閉じてから新しいセッションを作る。
    stop_task_inner(&app)?;

    let today = now.date_naive();
    let mut daily_log = load_daily_log(&app, today);

    // 新しいログを追加
    daily_log.logs.push(TimeLog {
        task_id: task_id.clone(),
        start_time: now,
        end_time: None,
    });

    save_daily_log(&app, today, &daily_log)?;

    let mut active_task_id = state.active_task_id.lock().unwrap();
    let mut active_task_start = state.active_task_start.lock().unwrap();
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

/// ランチャーを表示してキーボードフォーカスを当てる（フロントエンド主導の show 用）
/// show() だけではフォーカスが移らず Enter 等のキーイベントが届かないため、
/// set_focus() も合わせて Rust 側で実行する。
#[tauri::command]
fn show_launcher_self(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("launcher") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// ランチャーウィンドウを非表示にする（close ではなく hide で常駐維持）
#[tauri::command]
fn close_launcher(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("launcher") {
        win.hide().map_err(|e| e.to_string())?;
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
        manhour_project_code: String::new(),
        manhour_task_code: String::new(),
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

/// オペレーションの名称と説明を更新する
#[tauri::command]
fn update_operation(
    app: AppHandle,
    op_id: String,
    name: String,
    description: String,
    manhour_project_code: Option<String>,
    manhour_task_code: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("オペレーション名を入力してください".to_string());
    }

    let mut data = state.data.lock().unwrap();
    let operation = data
        .operations
        .iter_mut()
        .find(|operation| operation.id == op_id)
        .ok_or_else(|| format!("オペレーション '{}' が見つかりません", op_id))?;
    operation.name = trimmed_name.to_string();
    operation.description = description.trim().to_string();
    if let Some(project_code) = manhour_project_code {
        operation.manhour_project_code = project_code.trim().to_string();
    }
    if let Some(task_code) = manhour_task_code {
        operation.manhour_task_code = task_code.trim().to_string();
    }
    save_master(&app, &data)?;
    let _ = app.emit("state-changed", ());
    Ok(())
}

/// タスクの名称とタグを更新する
#[tauri::command]
fn update_task(
    app: AppHandle,
    task_id: String,
    name: String,
    tag: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("タスク名を入力してください".to_string());
    }

    let mut data = state.data.lock().unwrap();
    let task = data
        .operations
        .iter_mut()
        .flat_map(|operation| operation.tasks.iter_mut())
        .find(|task| task.id == task_id)
        .ok_or_else(|| format!("タスク '{}' が見つかりません", task_id))?;
    task.name = trimmed_name.to_string();
    task.tag = tag.trim().to_string();
    save_master(&app, &data)?;
    let _ = app.emit("state-changed", ());
    Ok(())
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

struct LoadedCertificates {
    certificates: Vec<reqwest::Certificate>,
    format: &'static str,
    bytes: usize,
}

fn load_root_certificates(path: &str) -> Result<LoadedCertificates, String> {
    let certificate_bytes =
        fs::read(path).map_err(|error| format!("証明書ファイルを読み込めません: {error}"))?;
    if certificate_bytes.is_empty() {
        return Err("証明書ファイルが空です".to_string());
    }
    let is_pem = certificate_bytes
        .windows(b"-----BEGIN CERTIFICATE-----".len())
        .any(|window| window == b"-----BEGIN CERTIFICATE-----");
    let certificates = if is_pem {
        reqwest::Certificate::from_pem_bundle(&certificate_bytes)
            .map_err(|error| format!("PEM証明書を解釈できません: {error}"))?
    } else {
        vec![reqwest::Certificate::from_der(&certificate_bytes).map_err(|error| {
            format!(
                "DER形式のX.509証明書を解釈できません: {error}。PKCS#7やPFXではなく、X.509のCERを指定してください"
            )
        })?]
    };
    if certificates.is_empty() {
        return Err("証明書ファイルにX.509証明書がありません".to_string());
    }
    Ok(LoadedCertificates {
        certificates,
        format: if is_pem { "PEM" } else { "DER" },
        bytes: certificate_bytes.len(),
    })
}

async fn login_attendance_client(
    config: &AttendanceConfig,
    logger: Option<&ConnectionLogger>,
) -> Result<Client, String> {
    if let Some(logger) = logger {
        logger.info(format!(
            "TLSクライアント初期化: backend=native-tls (Windows証明書ストア), login={}",
            endpoint_for_log(&config.login_url)
        ));
    }
    let mut builder = Client::builder()
        .cookie_store(true)
        .redirect(Policy::limited(10))
        .user_agent("Tempomezurado/0.1 attendance integration");
    if !config.certificate_path.trim().is_empty() {
        let loaded = load_root_certificates(&config.certificate_path).map_err(|error| {
            if let Some(logger) = logger {
                logger.error(format!("追加証明書の読込失敗: {error}"));
            }
            error
        })?;
        if let Some(logger) = logger {
            let filename = PathBuf::from(&config.certificate_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("(ファイル名不明)")
                .to_string();
            logger.info(format!(
                "追加証明書を信頼ルートとして読込: file={filename}, format={}, certificates={}, bytes={}",
                loaded.format,
                loaded.certificates.len(),
                loaded.bytes
            ));
            logger.info(
                "注意: クライアント証明書認証が必要な環境では、秘密鍵を含まない.cer単体は使用できません",
            );
        }
        for certificate in loaded.certificates {
            builder = builder.add_root_certificate(certificate);
        }
    } else if let Some(logger) = logger {
        logger.info("追加証明書なし: Windows証明書ストアのみ使用");
    }
    let client = builder
        .build()
        .map_err(|error| request_error(logger, "勤怠接続を初期化できません", error))?;

    let mut login_page_url =
        Url::parse(&config.login_url).map_err(|error| format!("ログインURLが不正です: {error}"))?;
    login_page_url
        .query_pairs_mut()
        .append_pair("login_company_code", &config.company_id);
    if let Some(logger) = logger {
        logger.info(format!(
            "ログイン画面へ接続開始: {}",
            endpoint_for_log(login_page_url.as_str())
        ));
    }
    let login_page = client
        .get(login_page_url)
        .send()
        .await
        .map_err(|error| request_error(logger, "ログイン画面へ接続できません", error))?
        .error_for_status()
        .map_err(|error| request_error(logger, "ログイン画面を取得できません", error))?
        .text()
        .await
        .map_err(|error| request_error(logger, "ログイン画面を読み取れません", error))?;
    if let Some(logger) = logger {
        logger.info("ログイン画面取得成功・認証トークンを検証");
    }

    let authenticity_token = {
        let document = Html::parse_document(&login_page);
        let selector = Selector::parse("input[name=\"authenticity_token\"]").unwrap();
        document
            .select(&selector)
            .next()
            .and_then(|element| element.value().attr("value"))
            .map(str::to_string)
            .ok_or_else(|| {
                if let Some(logger) = logger {
                    logger.error("ログイン画面にauthenticity_tokenが見つかりません");
                }
                "ログイン画面の認証トークンを取得できませんでした".to_string()
            })?
    };

    if let Some(logger) = logger {
        logger.info("認証情報を送信（企業ID・従業員番号・パスワードはログに記録しません）");
    }
    let login_response = client
        .post(&config.login_url)
        .form(&[
            ("authenticity_token", authenticity_token.as_str()),
            ("form[company_id]", config.company_id.as_str()),
            ("form[login_id]", config.employee_id.as_str()),
            ("form[password]", config.password.as_str()),
            ("form[next]", ""),
            ("form[fill_company_id_and_login_id]", "0"),
            ("commit", "ログイン"),
        ])
        .send()
        .await
        .map_err(|error| request_error(logger, "勤怠サイトへログインできません", error))?
        .error_for_status()
        .map_err(|error| request_error(logger, "勤怠サイトのログインに失敗しました", error))?;
    let remained_on_login = login_response.url().path().ends_with("/login");
    if let Some(logger) = logger {
        logger.info(format!(
            "ログイン応答受信: final={}",
            endpoint_for_log(login_response.url().as_str())
        ));
    }
    let login_result = login_response
        .text()
        .await
        .map_err(|error| request_error(logger, "ログイン結果を読み取れません", error))?;
    if remained_on_login && login_result.contains("submit-button") {
        if let Some(logger) = logger {
            logger.error("ログイン画面に留まりました（認証情報またはサイト側応答を確認）");
        }
        return Err(
            "勤怠サイトへログインできませんでした。接続設定の認証情報と診断ログを確認してください"
                .to_string(),
        );
    }
    if let Some(logger) = logger {
        logger.info("ログイン成功");
    }
    Ok(client)
}

async fn request_attendance_day(
    config: &AttendanceConfig,
    date: NaiveDate,
    logger: Option<&ConnectionLogger>,
) -> Result<AttendanceDay, String> {
    let client = login_attendance_client(config, logger).await?;
    let attendance_url = attendance_url_for_date(&config.attendance_url, date)?;
    if let Some(logger) = logger {
        logger.info(format!(
            "出勤簿へ接続開始: {}",
            endpoint_for_log(attendance_url.as_str())
        ));
    }
    let attendance_response = client
        .get(attendance_url)
        .send()
        .await
        .map_err(|error| request_error(logger, "出勤簿へ接続できません", error))?
        .error_for_status()
        .map_err(|error| request_error(logger, "出勤簿を取得できません", error))?;
    let attendance_page_url = attendance_response.url().clone();
    let attendance_path = attendance_page_url.path().to_string();
    let attendance_html = attendance_response
        .text()
        .await
        .map_err(|error| request_error(logger, "出勤簿を読み取れません", error))?;
    if attendance_html.contains("id=\"submit-button\"") {
        if let Some(logger) = logger {
            logger.error("出勤簿への遷移後にログイン画面が返されました");
        }
        return Err("出勤簿を開く前にログイン状態が失われました".to_string());
    }

    let records_path = {
        let document = Html::parse_document(&attendance_html);
        let selector = Selector::parse("[data-records-url]").unwrap();
        document
            .select(&selector)
            .next()
            .and_then(|element| element.value().attr("data-records-url"))
            .map(str::to_string)
            .ok_or_else(|| "出勤簿の勤務実績URLを取得できませんでした".to_string())?
    };
    let mut records_url = attendance_page_url
        .join(&records_path)
        .map_err(|error| format!("勤務実績URLを解釈できません: {error}"))?;
    let records_path = format!(
        "{}/{}",
        records_url.path().trim_end_matches('/'),
        date.format("%Y%m")
    );
    records_url.set_path(&records_path);
    if let Some(logger) = logger {
        logger.info(format!(
            "勤務実績へ接続開始: {}",
            endpoint_for_log(records_url.as_str())
        ));
    }
    let records_script = client
        .get(records_url)
        .header("X-Requested-With", "XMLHttpRequest")
        .header(
            "Accept",
            "text/javascript, application/javascript, application/ecmascript, application/x-ecmascript",
        )
        .send()
        .await
        .map_err(|error| request_error(logger, "勤務実績へ接続できません", error))?
        .error_for_status()
        .map_err(|error| request_error(logger, "勤務実績を取得できません", error))?
        .text()
        .await
        .map_err(|error| request_error(logger, "勤務実績を読み取れません", error))?;
    let records_html = decode_javascript_string(&records_script).ok_or_else(|| {
        if let Some(logger) = logger {
            logger.error(format!(
                "勤務実績の応答形式が想定外: bytes={}, html_call={}, replace_call={}",
                records_script.len(),
                records_script.contains(".html("),
                records_script.contains("replaceWith(")
            ));
        }
        format!(
            "勤務実績の応答を解釈できませんでした（長さ: {}、html呼出: {}、置換呼出: {}）",
            records_script.len(),
            records_script.contains(".html("),
            records_script.contains("replaceWith(")
        )
    })?;

    let day = parse_attendance_html(&records_html, date)
        .map_err(|error| format!("{error}（応答先: {attendance_path}）"))?;
    if let Some(logger) = logger {
        logger.info(format!(
            "勤怠取得成功: date={}, start={}, end={}, break_minutes={}, work_minutes={}",
            day.date,
            day.start_time.as_deref().unwrap_or("-"),
            day.end_time.as_deref().unwrap_or("-"),
            day.break_minutes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            day.work_minutes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ));
    }
    Ok(day)
}

fn manhour_report_url(base: &str, date: NaiveDate) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|error| format!("工数URLが不正です: {error}"))?;
    let path = url.path().trim_end_matches('/');
    let prefix = path
        .strip_suffix("/manhours")
        .ok_or_else(|| "工数URLは工数管理簿のURLを指定してください".to_string())?;
    url.set_path(&format!("{prefix}/manhour_reports/new"));
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("date", &date.format("%Y-%m-%d").to_string());
    Ok(url)
}

fn collect_remote_options(value: &serde_json::Value, options: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                collect_remote_options(value, options);
            }
        }
        serde_json::Value::Object(object) => {
            let id =
                object
                    .get("id")
                    .or_else(|| object.get("value"))
                    .and_then(|value| match value {
                        serde_json::Value::String(value) => Some(value.clone()),
                        serde_json::Value::Number(value) => Some(value.to_string()),
                        _ => None,
                    });
            let text = object
                .get("text")
                .or_else(|| object.get("label"))
                .or_else(|| object.get("name"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            if let (Some(id), Some(text)) = (id, text) {
                options.push((id, text));
            }
            for child in object.values() {
                collect_remote_options(child, options);
            }
        }
        _ => {}
    }
}

async fn resolve_remote_option(
    client: &Client,
    page_url: &Url,
    endpoint: &str,
    code: &str,
    staff_id: &str,
    date: Option<&str>,
    project_id: Option<&str>,
) -> Result<String, String> {
    let url = page_url
        .join(endpoint)
        .map_err(|error| format!("工数検索URLを解釈できません: {error}"))?;
    let mut query = vec![
        ("q".to_string(), code.to_string()),
        ("staff_id".to_string(), staff_id.to_string()),
    ];
    if let Some(date) = date {
        query.push(("date".to_string(), date.to_string()));
    }
    if let Some(project_id) = project_id {
        query.push(("project_id".to_string(), project_id.to_string()));
    }
    let body = client
        .get(url)
        .query(&query)
        .header("X-Requested-With", "XMLHttpRequest")
        .send()
        .await
        .map_err(|error| format!("{code}を検索できません: {error}"))?
        .error_for_status()
        .map_err(|error| format!("{code}の検索に失敗しました: {error}"))?
        .text()
        .await
        .map_err(|error| format!("{code}の検索結果を読み取れません: {error}"))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("{code}の検索結果を解釈できません: {error}"))?;
    let mut options = Vec::new();
    collect_remote_options(&value, &mut options);
    options
        .into_iter()
        .find(|(_, text)| {
            text.trim_start().starts_with(code)
                && (text == code
                    || text
                        .chars()
                        .nth(code.chars().count())
                        .is_some_and(|character| {
                            character.is_whitespace() || character == '(' || character == '（'
                        }))
        })
        .map(|(id, _)| id)
        .ok_or_else(|| format!("勤怠サイトに「{code}」が見つかりません"))
}

fn collect_form_fields(html: &str) -> Result<Vec<(String, String)>, String> {
    let document = Html::parse_document(html);
    let form_selector = Selector::parse("form#new_form").unwrap();
    let field_selector = Selector::parse("input[name], textarea[name], select[name]").unwrap();
    let option_selector = Selector::parse("option").unwrap();
    let form = document
        .select(&form_selector)
        .next()
        .ok_or_else(|| "工数入力フォームが見つかりません".to_string())?;
    let mut fields = Vec::new();
    for field in form.select(&field_selector) {
        let name = field.value().attr("name").unwrap_or_default();
        if name.is_empty() || name == "project_name" {
            continue;
        }
        let tag = field.value().name();
        let input_type = field.value().attr("type").unwrap_or_default();
        if matches!(input_type, "button" | "submit" | "reset" | "file")
            || (matches!(input_type, "checkbox" | "radio")
                && field.value().attr("checked").is_none())
        {
            continue;
        }
        let value = match tag {
            "textarea" => text_of(field),
            "select" => field
                .select(&option_selector)
                .find(|option| option.value().attr("selected").is_some())
                .or_else(|| field.select(&option_selector).next())
                .and_then(|option| option.value().attr("value"))
                .unwrap_or_default()
                .to_string(),
            _ => field.value().attr("value").unwrap_or_default().to_string(),
        };
        fields.push((name.to_string(), value));
    }
    Ok(fields)
}

fn set_form_value(fields: &mut Vec<(String, String)>, name: String, value: String) {
    if let Some(field) = fields
        .iter_mut()
        .find(|(field_name, _)| *field_name == name)
    {
        field.1 = value;
    } else {
        fields.push((name, value));
    }
}

struct PreparedManhourSubmission {
    client: Client,
    submit_url: Url,
    fields: Vec<(String, String)>,
}

async fn prepare_manhour_submission(
    config: &AttendanceConfig,
    date: NaiveDate,
    entries: &[ManhourSubmissionEntry],
    logger: Option<&ConnectionLogger>,
) -> Result<PreparedManhourSubmission, String> {
    if config.manhour_url.trim().is_empty() {
        return Err("接続設定に工数URLを入力してください".to_string());
    }
    if entries.is_empty() {
        return Err("送信する工数がありません".to_string());
    }
    for entry in entries {
        if entry.project_code.trim().is_empty() || entry.task_code.trim().is_empty() {
            return Err(format!(
                "{}の工数プロジェクト・タスクを設定してください",
                entry.operation_name
            ));
        }
        if entry.minutes <= 0 || entry.minutes > 24 * 60 {
            return Err(format!("{}の時間を確認してください", entry.operation_name));
        }
    }

    let client = login_attendance_client(config, logger).await?;
    let report_url = manhour_report_url(&config.manhour_url, date)?;
    if let Some(logger) = logger {
        logger.info(format!(
            "工数入力画面へ接続開始: {}",
            endpoint_for_log(report_url.as_str())
        ));
    }
    let response = client
        .get(report_url)
        .send()
        .await
        .map_err(|error| request_error(logger, "工数入力画面へ接続できません", error))?
        .error_for_status()
        .map_err(|error| request_error(logger, "工数入力画面を取得できません", error))?;
    let page_url = response.url().clone();
    let html = response
        .text()
        .await
        .map_err(|error| request_error(logger, "工数入力画面を読み取れません", error))?;
    let (submit_url, project_search_url) = {
        let document = Html::parse_document(&html);
        let form_selector = Selector::parse("form#new_form").unwrap();
        let project_selector = Selector::parse("#project_name").unwrap();
        let form = document
            .select(&form_selector)
            .next()
            .ok_or_else(|| "工数入力フォームが見つかりません".to_string())?;
        let submit_url = page_url
            .join(
                form.value()
                    .attr("action")
                    .unwrap_or("/ja/sp/manhour_reports"),
            )
            .map_err(|error| format!("工数送信URLを解釈できません: {error}"))?;
        let project_search_url = document
            .select(&project_selector)
            .next()
            .and_then(|element| element.value().attr("data-search-url"))
            .unwrap_or("/ja/sp/manhour_reports/search_projects")
            .to_string();
        (submit_url, project_search_url)
    };
    let task_search_url = "/ja/sp/manhour_reports/search_tasks";
    let mut fields = collect_form_fields(&html)?;
    let staff_id = fields
        .iter()
        .find(|(name, _)| name == "staff_id")
        .map(|(_, value)| value.clone())
        .ok_or_else(|| "工数入力対象の従業員を取得できません".to_string())?;
    let apply_date = fields
        .iter()
        .find(|(name, _)| name == "form[apply_dates][]")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| date.format("%Y-%m-%d").to_string());

    let existing_task_prefixes: HashMap<String, String> = fields
        .iter()
        .filter(|(name, _)| name.ends_with("[task_id]"))
        .map(|(name, value)| {
            (
                value.clone(),
                name.trim_end_matches("[task_id]").to_string(),
            )
        })
        .collect();
    let mut project_ids: HashMap<String, String> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let project_id = if let Some(id) = project_ids.get(&entry.project_code) {
            id.clone()
        } else {
            let id = resolve_remote_option(
                &client,
                &page_url,
                &project_search_url,
                &entry.project_code,
                &staff_id,
                Some(&apply_date),
                None,
            )
            .await?;
            project_ids.insert(entry.project_code.clone(), id.clone());
            id
        };
        let task_id = resolve_remote_option(
            &client,
            &page_url,
            task_search_url,
            &entry.task_code,
            &staff_id,
            None,
            Some(&project_id),
        )
        .await?;
        let prefix = existing_task_prefixes
            .get(&task_id)
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "form[manhour_attributes][{project_id}][manhours][{}]",
                    10_000 + index
                )
            });
        set_form_value(&mut fields, format!("{prefix}[task_id]"), task_id);
        set_form_value(
            &mut fields,
            format!("{prefix}[hour_text]"),
            format_manhour_minutes(entry.minutes),
        );
        set_form_value(
            &mut fields,
            format!("{prefix}[comment]"),
            entry.comment.clone(),
        );
    }
    fields.push(("submit".to_string(), "確定".to_string()));
    Ok(PreparedManhourSubmission {
        client,
        submit_url,
        fields,
    })
}

/// アプリ内の勤怠設定画面へ、パスワードを除いた現在値を返す。
#[tauri::command]
fn get_attendance_settings(app: AppHandle) -> Result<AttendanceSettingsView, String> {
    if let Some(settings) = load_stored_attendance_settings(&app)? {
        let manhour_url = if settings.manhour_url.is_empty() {
            find_login_file(&app)
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|content| parse_attendance_config(&content).ok())
                .map(|config| config.manhour_url)
                .unwrap_or_default()
        } else {
            settings.manhour_url.clone()
        };
        return Ok(AttendanceSettingsView {
            login_url: settings.login_url,
            company_id: settings.company_id,
            employee_id: settings.employee_id,
            attendance_url: settings.attendance_url,
            manhour_url,
            certificate_path: settings.certificate_path,
            password_saved: credential_entry()
                .and_then(|entry| entry.get_password().map_err(|error| error.to_string()))
                .is_ok(),
            source: "app".to_string(),
        });
    }

    if let Ok(path) = find_login_file(&app) {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = parse_attendance_config(&content) {
                return Ok(AttendanceSettingsView {
                    login_url: config.login_url,
                    company_id: config.company_id,
                    employee_id: config.employee_id,
                    attendance_url: config.attendance_url,
                    manhour_url: config.manhour_url,
                    certificate_path: config.certificate_path,
                    password_saved: true,
                    source: "login.txt".to_string(),
                });
            }
        }
    }

    Ok(AttendanceSettingsView {
        login_url: String::new(),
        company_id: String::new(),
        employee_id: String::new(),
        attendance_url: String::new(),
        manhour_url: String::new(),
        certificate_path: String::new(),
        password_saved: false,
        source: "none".to_string(),
    })
}

/// 勤怠の接続情報を保存する。パスワードだけはWindows資格情報マネージャーへ格納する。
#[tauri::command]
fn save_attendance_settings(
    app: AppHandle,
    login_url: String,
    company_id: String,
    employee_id: String,
    attendance_url: String,
    manhour_url: String,
    certificate_path: String,
    password: String,
) -> Result<AttendanceSettingsView, String> {
    let login_url = login_url.trim().to_string();
    let company_id = company_id.trim().to_string();
    let employee_id = employee_id.trim().to_string();
    let attendance_url = attendance_url.trim().to_string();
    let manhour_url = manhour_url.trim().to_string();
    let certificate_path = certificate_path.trim().to_string();
    if company_id.is_empty() || employee_id.is_empty() {
        return Err("企業IDと従業員番号を入力してください".to_string());
    }
    for (label, value) in [
        ("ログインURL", login_url.as_str()),
        ("出勤簿URL", attendance_url.as_str()),
        ("工数URL", manhour_url.as_str()),
    ] {
        let parsed = Url::parse(value).map_err(|_| format!("{label}を確認してください"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("{label}はhttpまたはhttpsで入力してください"));
        }
    }
    if !certificate_path.is_empty() {
        let loaded = load_root_certificates(&certificate_path)?;
        if let Ok(logger) = ConnectionLogger::new(&app) {
            logger.info(format!(
                "接続設定の証明書検証成功: format={}, certificates={}, bytes={}",
                loaded.format,
                loaded.certificates.len(),
                loaded.bytes
            ));
        }
    }

    let entry = credential_entry()?;
    let password = if password.is_empty() {
        entry.get_password().ok().or_else(|| {
            find_login_file(&app)
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|content| parse_attendance_config(&content).ok())
                .map(|config| config.password)
        })
    } else {
        Some(password)
    }
    .ok_or_else(|| "パスワードを入力してください".to_string())?;
    entry
        .set_password(&password)
        .map_err(|error| format!("パスワードを安全に保存できません: {error}"))?;

    let settings = StoredAttendanceSettings {
        login_url,
        company_id,
        employee_id,
        attendance_url,
        manhour_url,
        certificate_path,
    };
    let path = attendance_settings_file_path(&app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("勤怠設定を保存できません: {error}"))?;

    Ok(AttendanceSettingsView {
        login_url: settings.login_url,
        company_id: settings.company_id,
        employee_id: settings.employee_id,
        attendance_url: settings.attendance_url,
        manhour_url: settings.manhour_url,
        certificate_path: settings.certificate_path,
        password_saved: true,
        source: "app".to_string(),
    })
}

#[tauri::command]
fn get_connection_diagnostics(app: AppHandle) -> Result<ConnectionDiagnosticsView, String> {
    let logger = ConnectionLogger::new(&app)?;
    let content = if logger.path.is_file() {
        fs::read_to_string(&logger.path)
            .map_err(|error| format!("診断ログを読み込めません: {error}"))?
    } else {
        String::new()
    };
    Ok(ConnectionDiagnosticsView {
        path: logger.path.to_string_lossy().to_string(),
        content,
    })
}

#[tauri::command]
fn clear_connection_diagnostics(app: AppHandle) -> Result<(), String> {
    let logger = ConnectionLogger::new(&app)?;
    fs::write(&logger.path, "").map_err(|error| format!("診断ログを消去できません: {error}"))
}

#[tauri::command]
async fn test_attendance_connection(app: AppHandle) -> Result<String, String> {
    let logger = ConnectionLogger::new(&app)?;
    logger.info("=== 接続テスト開始 ===");
    let config = load_attendance_config(&app).map_err(|error| {
        logger.error(format!("接続設定の読込失敗: {error}"));
        error
    })?;
    login_attendance_client(&config, Some(&logger)).await?;
    logger.info("=== 接続テスト成功 ===");
    Ok("ログイン接続に成功しました".to_string())
}

/// login.txt の認証情報で勤怠サイトにログインし、指定日の勤務実績を取得する。
#[tauri::command]
async fn fetch_attendance_day(app: AppHandle, date: String) -> Result<AttendanceDay, String> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|error| format!("対象日を解釈できません: {error}"))?;
    let config = load_attendance_config(&app)?;
    let logger = ConnectionLogger::new(&app)?;
    logger.info(format!("=== 勤怠取得開始: date={date} ==="));
    let day = request_attendance_day(&config, date, Some(&logger)).await?;
    save_attendance_day(&app, &day)?;
    logger.info("=== 勤怠取得完了 ===");
    Ok(day)
}

/// 日別ログを、勤怠サイトの「親プロジェクト / タスク」形式へ変換して返す。
#[tauri::command]
fn get_manhour_preview(
    app: AppHandle,
    date: String,
    state: State<'_, AppState>,
) -> Result<ManhourPreview, String> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|error| format!("対象日を解釈できません: {error}"))?;
    let master = state.data.lock().unwrap().clone();
    let daily_log = load_daily_log(&app, date);
    let attendance_days = load_attendance_days(&app);
    Ok(build_manhour_preview(
        &master,
        &daily_log,
        date,
        attendance_days.get(&date.format("%Y-%m-%d").to_string()),
    ))
}

/// プレビューで確認済みの日別工数を勤怠サイトへ送信する。
#[tauri::command]
async fn submit_manhours(
    app: AppHandle,
    date: String,
    mut entries: Vec<ManhourSubmissionEntry>,
) -> Result<ManhourSubmissionResult, String> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|error| format!("対象日を解釈できません: {error}"))?;
    let total_minutes: i64 = entries.iter().map(|entry| entry.minutes).sum();
    let attendance = load_attendance_days(&app)
        .get(&date.format("%Y-%m-%d").to_string())
        .cloned()
        .ok_or_else(|| "先に対象日の勤怠を取得してください".to_string())?;
    let work_minutes = attendance
        .work_minutes
        .ok_or_else(|| "対象日の実働時間が確定していません".to_string())?;
    if total_minutes != work_minutes {
        return Err(format!(
            "工数合計 {} を実働時間 {} に一致させてください",
            format_manhour_minutes(total_minutes),
            format_manhour_minutes(work_minutes)
        ));
    }
    let master = load_master(&app);
    let daily_log = load_daily_log(&app, date);
    let preview = build_manhour_preview(&master, &daily_log, date, Some(&attendance));
    if preview.has_unfinished_logs {
        return Err("未終了ログを解消してから工数を送信してください".to_string());
    }
    if !preview.unmapped_operations.is_empty() {
        return Err(format!(
            "工数対応が未設定です: {}",
            preview.unmapped_operations.join("、")
        ));
    }
    let expected_operations: BTreeSet<String> = preview
        .entries
        .iter()
        .map(|entry| entry.operation_name.clone())
        .collect();
    let submitted_operations: BTreeSet<String> = entries
        .iter()
        .map(|entry| entry.operation_name.clone())
        .collect();
    if expected_operations != submitted_operations {
        return Err("送信対象のオペレーションが計測ログと一致しません".to_string());
    }
    let comments_by_operation: HashMap<String, String> = preview
        .entries
        .into_iter()
        .map(|entry| (entry.operation_name, entry.comment))
        .collect();
    for entry in &mut entries {
        entry.comment = comments_by_operation
            .get(&entry.operation_name)
            .cloned()
            .unwrap_or_default();
    }

    let config = load_attendance_config(&app)?;
    let logger = ConnectionLogger::new(&app)?;
    logger.info(format!(
        "=== 工数送信開始: date={date}, entries={} ===",
        entries.len()
    ));
    let prepared = prepare_manhour_submission(&config, date, &entries, Some(&logger)).await?;
    let response = prepared
        .client
        .post(prepared.submit_url)
        .form(&prepared.fields)
        .send()
        .await
        .map_err(|error| request_error(Some(&logger), "工数を送信できません", error))?
        .error_for_status()
        .map_err(|error| request_error(Some(&logger), "工数の登録に失敗しました", error))?;
    let final_path = response.url().path().to_string();
    let response_html = response
        .text()
        .await
        .map_err(|error| request_error(Some(&logger), "工数登録結果を読み取れません", error))?;
    if final_path.ends_with("/manhour_reports")
        && response_html.contains("new_form")
        && (response_html.contains("error") || response_html.contains("alert"))
    {
        logger.error("工数登録画面に入力エラーまたは警告が返されました");
        return Err(
            "勤怠サイトが工数入力を受け付けませんでした。内容を確認してください".to_string(),
        );
    }
    logger.info("=== 工数送信完了 ===");

    Ok(ManhourSubmissionResult {
        date: date.format("%Y-%m-%d").to_string(),
        submitted_count: entries.len(),
        total_minutes,
    })
}

/// 日別の作業合計と、取得済みの勤怠実績を CSV ファイルに書き出してパスを返す
#[tauri::command]
fn export_summary_csv(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let logs_dir = app_data_dir.join("logs");

    // 日付 -> 合計秒数 を BTreeMap で集計（日付昇順が保たれる）
    let mut daily_totals: BTreeMap<String, i64> = BTreeMap::new();

    if logs_dir.exists() {
        for entry in fs::read_dir(&logs_dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap_or_default();
            if let Ok(daily_log) = serde_json::from_str::<DailyLog>(&content) {
                for log in daily_log.logs {
                    if let Some(end_time) = log.end_time {
                        let date_str = log.start_time.format("%Y-%m-%d").to_string();
                        *daily_totals.entry(date_str).or_insert(0) +=
                            (end_time - log.start_time).num_seconds().max(0);
                    }
                }
            }
        }
    }

    let export_dir = app_data_dir.join("exports");
    fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    let filename = format!("summary_{}.csv", Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let file_path = export_dir.join(&filename);

    let attendance_days = load_attendance_days(&app);
    let all_dates: BTreeSet<String> = daily_totals
        .keys()
        .chain(attendance_days.keys())
        .cloned()
        .collect();

    let mut wtr = Writer::from_path(&file_path).map_err(|e| e.to_string())?;
    wtr.write_record([
        "日付",
        "計測時間(分)",
        "始業時間",
        "終業時間",
        "休憩時間(分)",
        "業務時間(分)",
        "計測との差(分)",
        "勤務状況",
    ])
    .map_err(|e| e.to_string())?;
    for date in all_dates {
        let tracked_minutes = daily_totals
            .get(&date)
            .map(|seconds| *seconds as f64 / 60.0);
        let attendance = attendance_days.get(&date);
        let difference = attendance
            .and_then(|day| day.work_minutes)
            .zip(tracked_minutes)
            .map(|(work, tracked)| work as f64 - tracked);
        wtr.write_record([
            date,
            tracked_minutes
                .map(|minutes| format!("{minutes:.1}"))
                .unwrap_or_default(),
            attendance
                .and_then(|day| day.start_time.clone())
                .unwrap_or_default(),
            attendance
                .and_then(|day| day.end_time.clone())
                .unwrap_or_default(),
            attendance
                .and_then(|day| day.break_minutes)
                .map(|minutes| minutes.to_string())
                .unwrap_or_default(),
            attendance
                .and_then(|day| day.work_minutes)
                .map(|minutes| minutes.to_string())
                .unwrap_or_default(),
            difference
                .map(|minutes| format!("{minutes:.1}"))
                .unwrap_or_default(),
            attendance
                .and_then(|day| day.status.clone())
                .unwrap_or_default(),
        ])
        .map_err(|e| e.to_string())?;
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
        // 必ず最初に登録し、二重起動によるショートカット競合を防ぐ。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
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
        .on_window_event(|window, event| {
            // メインウィンドウが閉じられたときは、進行中ログを必ず確定する。
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                let _ = stop_task_inner(window.app_handle());
            }
        })
        .setup(|app| {
            let data = load_master(app.handle());
            // 同日中のクラッシュ・強制終了で残った最新ログだけを復元する。
            // 過去日の未終了ログは履歴画面でユーザーに判断してもらう。
            let today_log = load_daily_log(app.handle(), Local::now().date_naive());
            let recovered = today_log
                .logs
                .iter()
                .rev()
                .find(|log| log.end_time.is_none())
                .cloned();
            app.manage(AppState {
                data: Mutex::new(data),
                active_task_id: Mutex::new(recovered.as_ref().map(|log| log.task_id.clone())),
                active_task_start: Mutex::new(recovered.map(|log| log.start_time)),
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
            get_history,
            resolve_unfinished_log,
            start_task,
            stop_active_task,
            show_launcher_self,
            close_launcher,
            add_operation,
            add_task,
            update_operation,
            update_task,
            export_csv,
            get_attendance_settings,
            save_attendance_settings,
            get_connection_diagnostics,
            clear_connection_diagnostics,
            test_attendance_connection,
            fetch_attendance_day,
            get_manhour_preview,
            submit_manhours,
            export_summary_csv,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn local_time(value: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Local)
    }

    #[test]
    fn completed_log_uses_its_end_time() {
        let start = local_time("2026-06-28T09:00:00+09:00");
        let log = TimeLog {
            task_id: "task-a".to_string(),
            start_time: start,
            end_time: Some(start + Duration::minutes(5)),
        };

        assert_eq!(
            measured_duration_seconds(&log, None, None, start + Duration::hours(1)),
            Some(300)
        );
    }

    #[test]
    fn active_log_uses_current_time() {
        let start = local_time("2026-06-28T09:00:00+09:00");
        let log = TimeLog {
            task_id: "task-a".to_string(),
            start_time: start,
            end_time: None,
        };

        assert_eq!(
            measured_duration_seconds(
                &log,
                Some("task-a"),
                Some(&start),
                start + Duration::minutes(10)
            ),
            Some(600)
        );
    }

    #[test]
    fn orphan_log_is_not_counted() {
        let start = local_time("2026-06-28T09:00:00+09:00");
        let log = TimeLog {
            task_id: "task-a".to_string(),
            start_time: start,
            end_time: None,
        };

        assert_eq!(
            measured_duration_seconds(
                &log,
                Some("task-b"),
                Some(&start),
                start + Duration::days(10)
            ),
            None
        );
    }

    #[test]
    fn legacy_task_without_hidden_flag_remains_visible() {
        let task: Task = serde_json::from_str(r#"{"id":"task-a","name":"Task","tag":""}"#).unwrap();
        assert!(!task.hidden);
    }

    #[test]
    fn parses_login_txt_without_exposing_values() {
        let config = parse_attendance_config(
            "url=https://example.test/ja/login\n企業ID=company\n従業員番号=employee\nパスワード=secret\n出勤簿=https://example.test/ja/sp/attendance\n工数=https://example.test/ja/sp/manhours\n",
        )
        .unwrap();
        assert_eq!(config.company_id, "company");
        assert_eq!(
            config.attendance_url,
            "https://example.test/ja/sp/attendance"
        );
        assert_eq!(config.manhour_url, "https://example.test/ja/sp/manhours");
    }

    #[test]
    fn maps_operation_code_to_parent_project() {
        assert_eq!(
            parse_manhour_operation("系26-019"),
            Some(("系26-0".to_string(), "系26-019".to_string()))
        );
        assert_eq!(
            parse_manhour_operation("系26-219"),
            Some(("系26-2".to_string(), "系26-219".to_string()))
        );
        assert_eq!(parse_manhour_operation("管理"), None);

        let operation = Operation {
            id: "op-management".to_string(),
            name: "管理26".to_string(),
            description: String::new(),
            tasks: vec![],
            hidden: false,
            manhour_project_code: "管理26".to_string(),
            manhour_task_code: "管理26-001".to_string(),
        };
        assert_eq!(
            operation_manhour_mapping(&operation),
            Some(("管理26".to_string(), "管理26-001".to_string()))
        );
    }

    #[test]
    fn aggregates_logs_by_operation_for_manhour_preview() {
        let start = local_time("2026-06-30T09:00:00+09:00");
        let master = MasterData {
            operations: vec![Operation {
                id: "op-a".to_string(),
                name: "系26-019".to_string(),
                description: String::new(),
                tasks: vec![
                    Task {
                        id: "task-a".to_string(),
                        name: "実装".to_string(),
                        tag: String::new(),
                        hidden: false,
                    },
                    Task {
                        id: "task-b".to_string(),
                        name: "レビュー".to_string(),
                        tag: String::new(),
                        hidden: false,
                    },
                ],
                hidden: false,
                manhour_project_code: String::new(),
                manhour_task_code: String::new(),
            }],
        };
        let daily = DailyLog {
            logs: vec![
                TimeLog {
                    task_id: "task-a".to_string(),
                    start_time: start,
                    end_time: Some(start + Duration::minutes(60)),
                },
                TimeLog {
                    task_id: "task-a".to_string(),
                    start_time: start + Duration::minutes(60),
                    end_time: Some(start + Duration::minutes(95)),
                },
                TimeLog {
                    task_id: "task-b".to_string(),
                    start_time: start + Duration::minutes(95),
                    end_time: Some(start + Duration::minutes(105)),
                },
            ],
        };
        let attendance = AttendanceDay {
            date: "2026-06-30".to_string(),
            start_time: Some("09:00".to_string()),
            end_time: Some("17:00".to_string()),
            break_minutes: Some(60),
            work_minutes: Some(420),
            status: Some("勤務".to_string()),
        };
        let preview = build_manhour_preview(
            &master,
            &daily,
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            Some(&attendance),
        );
        assert_eq!(preview.entries.len(), 1);
        assert_eq!(preview.entries[0].project_code, "系26-0");
        assert_eq!(preview.entries[0].task_code, "系26-019");
        assert_eq!(preview.entries[0].minutes, 420);
        assert_eq!(preview.entries[0].comment, "実装\nレビュー");
        assert_eq!(preview.difference_minutes, Some(0));
    }

    #[test]
    fn proportional_allocation_preserves_attendance_total() {
        let seconds = BTreeMap::from([
            ("系26-019".to_string(), 2 * 60 * 60),
            ("管理26".to_string(), 60 * 60),
        ]);
        let allocated = allocate_minutes_by_seconds(&seconds, 421);
        assert_eq!(allocated.values().sum::<i64>(), 421);
        assert_eq!(allocated.get("系26-019"), Some(&281));
        assert_eq!(allocated.get("管理26"), Some(&140));
    }

    #[test]
    fn parses_aggregate_times_and_work_duration_from_attendance() {
        let html = r#"
            <table>
              <thead><tr><th>日付</th></tr></thead>
              <tbody>
                <tr><td>06/29(月)</td></tr>
                <tr><td>06/30(火)</td></tr>
              </tbody>
            </table>
            <table>
              <thead><tr><th>集計(出) 集計(退)</th><th>休憩時間</th></tr></thead>
              <tbody>
                <tr>
                  <td>11:00 15:00</td><td>09:00 18:00</td><td>09:00 18:00</td>
                  <td>09:00 18:00</td><td>勤務</td><td>8:00</td><td>0:00</td><td>1:00</td>
                </tr>
                <tr>
                  <td>11:00 15:00</td><td>08:30 17:45</td><td>08:30 17:45</td>
                  <td>08:30 17:45</td><td>テレワーク</td><td>8:15</td><td>0:00</td><td>1:00</td>
                </tr>
              </tbody>
            </table>
        "#;
        let day =
            parse_attendance_html(html, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()).unwrap();
        assert_eq!(day.start_time.as_deref(), Some("08:30"));
        assert_eq!(day.end_time.as_deref(), Some("17:45"));
        assert_eq!(day.break_minutes, Some(60));
        assert_eq!(day.work_minutes, Some(495));
        assert_eq!(day.status.as_deref(), Some("テレワーク"));
    }

    #[test]
    fn attendance_url_uses_requested_month() {
        let url = attendance_url_for_date(
            "https://example.test/ja/sp/attendance/202605",
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://example.test/ja/sp/attendance/202606");
    }

    #[test]
    fn diagnostic_endpoint_hides_credentials_and_query() {
        assert_eq!(
            endpoint_for_log(
                "https://user:secret@example.test:8443/ja/login?login_company_code=private#token"
            ),
            "https://example.test:8443/ja/login"
        );
    }

    #[test]
    fn decodes_replace_with_javascript_response() {
        let script = r##"$("#records").replaceWith("\u003ctable\u003e\u003ctr\u003e\u003ctd\u003e08:30\u003c/td\u003e\u003c/tr\u003e\u003c/table\u003e");"##;
        assert_eq!(
            decode_javascript_string(script).as_deref(),
            Some("<table><tr><td>08:30</td></tr></table>")
        );
    }

    #[test]
    #[ignore = "ローカルのlogin.txtと勤怠サイトへの接続が必要"]
    fn fetches_live_attendance_without_logging_credentials() {
        let login_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../login.txt");
        let config = parse_attendance_config(&fs::read_to_string(login_path).unwrap()).unwrap();
        let date = Local::now().date_naive();
        let day =
            tauri::async_runtime::block_on(request_attendance_day(&config, date, None)).unwrap();
        assert_eq!(day.date, date.format("%Y-%m-%d").to_string());
    }

    #[test]
    #[ignore = "ローカルのlogin.txtと勤怠サイトへの接続が必要"]
    fn prepares_live_manhour_without_submitting() {
        let login_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../login.txt");
        let config = parse_attendance_config(&fs::read_to_string(login_path).unwrap()).unwrap();
        let entries = vec![ManhourSubmissionEntry {
            operation_name: "系26-019".to_string(),
            project_code: "系26-0".to_string(),
            task_code: "系26-019".to_string(),
            minutes: 1,
            comment: "実装".to_string(),
        }];
        let prepared = tauri::async_runtime::block_on(prepare_manhour_submission(
            &config,
            Local::now().date_naive(),
            &entries,
            None,
        ))
        .unwrap();
        assert!(prepared
            .fields
            .iter()
            .any(|(name, value)| name.ends_with("[hour_text]") && value == "0:01"));
    }
}
