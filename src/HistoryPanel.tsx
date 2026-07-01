import {
  For,
  Show,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "./Icons";

interface HistoryEntry {
  id: string;
  task_id: string;
  task_name: string;
  operation_name: string;
  tag: string;
  start_time: string;
  end_time: string | null;
  duration_seconds: number | null;
  is_active: boolean;
}

interface AttendanceDay {
  date: string;
  start_time: string | null;
  end_time: string | null;
  break_minutes: number | null;
  work_minutes: number | null;
  status: string | null;
}

interface AttendanceSettings {
  login_url: string;
  company_id: string;
  employee_id: string;
  attendance_url: string;
  manhour_url: string;
  password_saved: boolean;
  source: "app" | "login.txt" | "none";
}

interface ManhourPreviewEntry {
  operation_name: string;
  project_code: string;
  task_code: string;
  minutes: number;
  time_text: string;
}

interface ManhourPreview {
  date: string;
  entries: ManhourPreviewEntry[];
  total_minutes: number;
  attendance_work_minutes: number | null;
  difference_minutes: number | null;
  unmapped_operations: string[];
  has_unfinished_logs: boolean;
}

interface HistoryPanelProps {
  onExport: () => void;
  onExportSummary: () => void;
  onNotify: (message: string) => void;
}

function formatDuration(total: number): string {
  const safe = Math.max(0, total);
  if (safe < 60) return `${safe}s`;
  const minutes = Math.floor(safe / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat("ja-JP", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function dateKey(value: string): string {
  const date = new Date(value);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function dateLabel(key: string): string {
  const today = dateKey(new Date().toISOString());
  const yesterdayDate = new Date();
  yesterdayDate.setDate(yesterdayDate.getDate() - 1);
  const yesterday = dateKey(yesterdayDate.toISOString());
  if (key === today) return "今日";
  if (key === yesterday) return "昨日";
  return new Intl.DateTimeFormat("ja-JP", {
    month: "long",
    day: "numeric",
    weekday: "short",
  }).format(new Date(`${key}T12:00:00`));
}

function formatMinutes(value: number | null): string {
  if (value === null) return "—";
  return `${Math.floor(value / 60)}h ${value % 60}m`;
}

export default function HistoryPanel(props: HistoryPanelProps) {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([]);
  const [days, setDays] = createSignal<7 | 30 | 0>(7);
  const [loading, setLoading] = createSignal(true);
  const [confirmingId, setConfirmingId] = createSignal<string | null>(null);
  const [clock, setClock] = createSignal(Date.now());
  const [attendanceDate, setAttendanceDate] = createSignal(
    dateKey(new Date().toISOString()),
  );
  const [attendance, setAttendance] = createSignal<AttendanceDay | null>(null);
  const [attendanceLoading, setAttendanceLoading] = createSignal(false);
  const [attendanceSettings, setAttendanceSettings] =
    createSignal<AttendanceSettings>({
      login_url: "",
      company_id: "",
      employee_id: "",
      attendance_url: "",
      manhour_url: "",
      password_saved: false,
      source: "none",
    });
  const [attendancePassword, setAttendancePassword] = createSignal("");
  const [showAttendanceSettings, setShowAttendanceSettings] =
    createSignal(false);
  const [settingsSaving, setSettingsSaving] = createSignal(false);
  const [manhourPreview, setManhourPreview] =
    createSignal<ManhourPreview | null>(null);
  const [manhourLoading, setManhourLoading] = createSignal(false);
  const [manhourSubmitting, setManhourSubmitting] = createSignal(false);
  const [confirmManhourSubmit, setConfirmManhourSubmit] = createSignal(false);
  let unlistenHistory: (() => void) | undefined;
  let unlistenState: (() => void) | undefined;

  const loadHistory = async () => {
    setLoading(true);
    try {
      const history = await invoke<HistoryEntry[]>("get_history", {
        days: days() || null,
      });
      setEntries(history);
    } catch (error) {
      console.error("履歴を読み込めませんでした", error);
      props.onNotify("履歴を読み込めませんでした");
    } finally {
      setLoading(false);
    }
  };

  const changeRange = (value: 7 | 30 | 0) => {
    setDays(value);
    queueMicrotask(loadHistory);
  };

  const unresolved = createMemo(() =>
    entries().filter((entry) => !entry.end_time && !entry.is_active),
  );

  const entryDuration = (entry: HistoryEntry) =>
    entry.is_active
      ? Math.max(
          0,
          Math.floor((clock() - new Date(entry.start_time).getTime()) / 1000),
        )
      : (entry.duration_seconds ?? 0);

  const totalSeconds = createMemo(() =>
    entries().reduce((total, entry) => total + entryDuration(entry), 0),
  );

  const todaySeconds = createMemo(() => {
    const today = dateKey(new Date().toISOString());
    return entries()
      .filter((entry) => dateKey(entry.start_time) === today)
      .reduce((total, entry) => total + entryDuration(entry), 0);
  });

  const groupedEntries = createMemo(() => {
    const groups = new Map<string, HistoryEntry[]>();
    for (const entry of entries()) {
      const key = dateKey(entry.start_time);
      const group = groups.get(key) ?? [];
      group.push(entry);
      groups.set(key, group);
    }
    return [...groups.entries()].map(([key, items]) => ({
      key,
      label: dateLabel(key),
      total: items.reduce((sum, entry) => sum + entryDuration(entry), 0),
      items,
    }));
  });

  const resolveEntry = async (
    entry: HistoryEntry,
    action: "discard" | "close_now",
  ) => {
    try {
      await invoke("resolve_unfinished_log", {
        taskId: entry.task_id,
        startTime: entry.start_time,
        action,
      });
      setConfirmingId(null);
      await loadHistory();
      props.onNotify(
        action === "discard"
          ? "未終了ログを削除しました"
          : "現在時刻でログを終了しました",
      );
    } catch (error) {
      console.error("未終了ログを解消できませんでした", error);
      props.onNotify("未終了ログを解消できませんでした");
    }
  };

  const loadAttendance = async () => {
    setAttendanceLoading(true);
    try {
      const day = await invoke<AttendanceDay>("fetch_attendance_day", {
        date: attendanceDate(),
      });
      setAttendance(day);
      props.onNotify(`${attendanceDate()} の勤怠を取得しました`);
    } catch (error) {
      console.error("勤怠を取得できませんでした", error);
      props.onNotify(`勤怠を取得できませんでした: ${String(error)}`);
    } finally {
      setAttendanceLoading(false);
    }
  };

  const loadAttendanceSettings = async () => {
    try {
      const settings = await invoke<AttendanceSettings>(
        "get_attendance_settings",
      );
      setAttendanceSettings(settings);
      setShowAttendanceSettings(settings.source === "none");
    } catch (error) {
      console.error("勤怠設定を読み込めませんでした", error);
    }
  };

  const saveAttendanceSettings = async (event: SubmitEvent) => {
    event.preventDefault();
    setSettingsSaving(true);
    try {
      const current = attendanceSettings();
      const saved = await invoke<AttendanceSettings>(
        "save_attendance_settings",
        {
          loginUrl: current.login_url,
          companyId: current.company_id,
          employeeId: current.employee_id,
          attendanceUrl: current.attendance_url,
          manhourUrl: current.manhour_url,
          password: attendancePassword(),
        },
      );
      setAttendanceSettings(saved);
      setAttendancePassword("");
      setShowAttendanceSettings(false);
      props.onNotify("勤怠の接続設定を保存しました");
    } catch (error) {
      console.error("勤怠設定を保存できませんでした", error);
      props.onNotify(`勤怠設定を保存できませんでした: ${String(error)}`);
    } finally {
      setSettingsSaving(false);
    }
  };

  const loadManhourPreview = async () => {
    setManhourLoading(true);
    setConfirmManhourSubmit(false);
    try {
      const preview = await invoke<ManhourPreview>("get_manhour_preview", {
        date: attendanceDate(),
      });
      setManhourPreview(preview);
    } catch (error) {
      console.error("工数集計を作成できませんでした", error);
      props.onNotify(`工数集計を作成できませんでした: ${String(error)}`);
    } finally {
      setManhourLoading(false);
    }
  };

  const manhourTotal = createMemo(
    () =>
      manhourPreview()?.entries.reduce(
        (total, entry) => total + entry.minutes,
        0,
      ) ?? 0,
  );

  const updateManhourMinutes = (index: number, minutes: number) => {
    setManhourPreview((preview) =>
      preview
        ? {
            ...preview,
            entries: preview.entries.map((entry, entryIndex) =>
              entryIndex === index
                ? { ...entry, minutes: Math.max(0, minutes) }
                : entry,
            ),
          }
        : preview,
    );
    setConfirmManhourSubmit(false);
  };

  const submitManhours = async () => {
    const preview = manhourPreview();
    if (!preview) return;
    setManhourSubmitting(true);
    try {
      const result = await invoke<{
        submitted_count: number;
        total_minutes: number;
      }>("submit_manhours", {
        date: preview.date,
        entries: preview.entries.map((entry) => ({
          operation_name: entry.operation_name,
          project_code: entry.project_code,
          task_code: entry.task_code,
          minutes: entry.minutes,
        })),
      });
      setConfirmManhourSubmit(false);
      props.onNotify(
        `${result.submitted_count}件・${formatMinutes(result.total_minutes)}の工数を登録しました`,
      );
    } catch (error) {
      console.error("工数を登録できませんでした", error);
      props.onNotify(`工数を登録できませんでした: ${String(error)}`);
    } finally {
      setManhourSubmitting(false);
    }
  };

  onMount(async () => {
    const timer = setInterval(() => setClock(Date.now()), 1000);
    await loadAttendanceSettings();
    await loadHistory();
    unlistenHistory = await listen("history-changed", loadHistory);
    unlistenState = await listen("state-changed", loadHistory);
    onCleanup(() => clearInterval(timer));
  });

  onCleanup(() => {
    unlistenHistory?.();
    unlistenState?.();
  });

  return (
    <div class="history-view">
      <section class="history-intro">
        <div>
          <p class="eyebrow">Insights</p>
          <h1>作業履歴</h1>
          <p>記録を振り返り、未終了のセッションを整理できます。</p>
        </div>
        <div class="flex items-center gap-1.5">
          <button class="history-export" onClick={props.onExportSummary} title="日別サマリーをCSVで書き出す">
            <Icon name="chart" size={14} />
            サマリー
          </button>
          <button class="history-export" onClick={props.onExport} title="詳細ログをCSVで書き出す">
            <Icon name="download" size={14} />
            CSV
          </button>
        </div>
      </section>

      <div class="history-range" aria-label="表示期間">
        <For each={[7, 30, 0] as const}>
          {(value) => (
            <button
              class={days() === value ? "is-active" : ""}
              onClick={() => changeRange(value)}
            >
              {value === 0 ? "すべて" : `${value}日`}
            </button>
          )}
        </For>
      </div>

      <section class="history-summary">
        <div>
          <span>Today</span>
          <strong>{formatDuration(todaySeconds())}</strong>
        </div>
        <div>
          <span>{days() ? `${days()} days` : "All time"}</span>
          <strong>{formatDuration(totalSeconds())}</strong>
        </div>
        <div>
          <span>Sessions</span>
          <strong>{entries().length}</strong>
        </div>
      </section>

      <section class="attendance-summary">
        <header>
          <div>
            <p class="eyebrow">Attendance</p>
            <div class="attendance-title-row">
              <h2>日次集計</h2>
              <button
                class="attendance-config-toggle"
                onClick={() =>
                  setShowAttendanceSettings((visible) => !visible)
                }
              >
                <Icon name="edit" size={10} />
                接続設定
              </button>
            </div>
          </div>
          <div class="attendance-actions">
            <input
              type="date"
              value={attendanceDate()}
              onInput={(event) => {
                setAttendanceDate(event.currentTarget.value);
                setAttendance(null);
                setManhourPreview(null);
                setConfirmManhourSubmit(false);
              }}
              aria-label="勤怠の対象日"
            />
            <button
              onClick={loadAttendance}
              disabled={attendanceLoading() || !attendanceDate()}
            >
              <Show
                when={!attendanceLoading()}
                fallback={<span class="launcher-spinner" />}
              >
                <Icon name="download" size={13} />
              </Show>
              {attendanceLoading() ? "取得中" : "勤怠を取得"}
            </button>
          </div>
        </header>

        <Show when={showAttendanceSettings()}>
          <form
            class="attendance-settings-form"
            onSubmit={saveAttendanceSettings}
          >
            <label>
              <span>ログインURL</span>
              <input
                type="url"
                required
                value={attendanceSettings().login_url}
                onInput={(event) =>
                  setAttendanceSettings((settings) => ({
                    ...settings,
                    login_url: event.currentTarget.value,
                  }))
                }
                placeholder="https://…/ja/login"
                autocomplete="url"
              />
            </label>
            <label>
              <span>出勤簿URL</span>
              <input
                type="url"
                required
                value={attendanceSettings().attendance_url}
                onInput={(event) =>
                  setAttendanceSettings((settings) => ({
                    ...settings,
                    attendance_url: event.currentTarget.value,
                  }))
                }
                placeholder="https://…/ja/sp/attendance"
                autocomplete="url"
              />
            </label>
            <label>
              <span>工数URL</span>
              <input
                type="url"
                required
                value={attendanceSettings().manhour_url}
                onInput={(event) =>
                  setAttendanceSettings((settings) => ({
                    ...settings,
                    manhour_url: event.currentTarget.value,
                  }))
                }
                placeholder="https://…/ja/sp/manhours"
                autocomplete="url"
              />
            </label>
            <div class="attendance-settings-grid">
              <label>
                <span>企業ID</span>
                <input
                  required
                  value={attendanceSettings().company_id}
                  onInput={(event) =>
                    setAttendanceSettings((settings) => ({
                      ...settings,
                      company_id: event.currentTarget.value,
                    }))
                  }
                  autocomplete="organization"
                />
              </label>
              <label>
                <span>従業員番号</span>
                <input
                  required
                  value={attendanceSettings().employee_id}
                  onInput={(event) =>
                    setAttendanceSettings((settings) => ({
                      ...settings,
                      employee_id: event.currentTarget.value,
                    }))
                  }
                  autocomplete="username"
                />
              </label>
            </div>
            <label>
              <span>
                パスワード
                <Show when={attendanceSettings().password_saved}>
                  <small>保存済み・変更時のみ入力</small>
                </Show>
              </span>
              <input
                type="password"
                required={!attendanceSettings().password_saved}
                value={attendancePassword()}
                onInput={(event) =>
                  setAttendancePassword(event.currentTarget.value)
                }
                placeholder={
                  attendanceSettings().password_saved
                    ? "••••••••"
                    : "パスワードを入力"
                }
                autocomplete="current-password"
              />
            </label>
            <div class="attendance-settings-footer">
              <span>
                パスワードはWindows資格情報マネージャーへ保存されます。
              </span>
              <button type="submit" disabled={settingsSaving()}>
                {settingsSaving() ? "保存中" : "設定を保存"}
              </button>
            </div>
          </form>
        </Show>

        <Show
          when={attendance()}
          fallback={
            <p class="attendance-empty">
              login.txt の接続情報を使って、出勤簿から勤務実績を取得します。
            </p>
          }
        >
          {(day) => (
            <div class="attendance-metrics">
              <div>
                <span>始業</span>
                <strong>{day().start_time ?? "—"}</strong>
              </div>
              <div>
                <span>終業</span>
                <strong>{day().end_time ?? "—"}</strong>
              </div>
              <div>
                <span>休憩</span>
                <strong>{formatMinutes(day().break_minutes)}</strong>
              </div>
              <div class="is-primary">
                <span>業務時間</span>
                <strong>{formatMinutes(day().work_minutes)}</strong>
              </div>
              <Show when={day().status}>
                <span class="attendance-status">{day().status}</span>
              </Show>
            </div>
          )}
        </Show>
      </section>

      <section class="manhour-summary">
        <header>
          <div>
            <p class="eyebrow">Man-hours</p>
            <h2>工数入力</h2>
          </div>
          <button
            class="manhour-preview-button"
            onClick={loadManhourPreview}
            disabled={manhourLoading() || !attendanceDate()}
          >
            <Show
              when={!manhourLoading()}
              fallback={<span class="launcher-spinner" />}
            >
              <Icon name="chart" size={13} />
            </Show>
            {manhourLoading() ? "集計中" : "工数を集計"}
          </button>
        </header>

        <Show
          when={manhourPreview()}
          fallback={
            <p class="attendance-empty">
              オペレーションを勤怠プロジェクト・タスクへ対応付けて集計します。
            </p>
          }
        >
          {(preview) => (
            <>
              <div class="manhour-overview">
                <div>
                  <span>計測工数</span>
                  <strong>{formatMinutes(manhourTotal())}</strong>
                </div>
                <div>
                  <span>実働時間</span>
                  <strong>
                    {formatMinutes(preview().attendance_work_minutes)}
                  </strong>
                </div>
                <div>
                  <span>未配分</span>
                  <strong>
                    {preview().attendance_work_minutes === null
                      ? "—"
                      : formatMinutes(
                          Math.max(
                            0,
                            preview().attendance_work_minutes! -
                              manhourTotal(),
                          ),
                        )}
                  </strong>
                </div>
              </div>

              <Show when={preview().unmapped_operations.length > 0}>
                <p class="manhour-warning">
                  工数対応が未設定です:{" "}
                  {preview().unmapped_operations.join("、")}
                  。管理画面のオペレーション編集から設定してください。
                </p>
              </Show>
              <Show when={preview().has_unfinished_logs}>
                <p class="manhour-warning">
                  未終了ログがあります。停止または解消してから送信してください。
                </p>
              </Show>
              <Show
                when={
                  preview().attendance_work_minutes !== null &&
                  manhourTotal() > preview().attendance_work_minutes!
                }
              >
                <p class="manhour-warning">
                  工数合計が実働時間を超えています。
                </p>
              </Show>

              <div class="manhour-entry-list">
                <For each={preview().entries}>
                  {(entry, index) => (
                    <div class="manhour-entry">
                      <div class="min-w-0">
                        <strong>{entry.operation_name}</strong>
                        <span>
                          {entry.project_code} / {entry.task_code}
                        </span>
                      </div>
                      <label>
                        <input
                          type="number"
                          min="1"
                          max="1440"
                          value={entry.minutes}
                          onInput={(event) =>
                            updateManhourMinutes(
                              index(),
                              Number(event.currentTarget.value),
                            )
                          }
                          aria-label={`${entry.operation_name}の工数（分）`}
                        />
                        <span>分</span>
                      </label>
                    </div>
                  )}
                </For>
              </div>

              <Show when={preview().entries.length === 0}>
                <p class="manhour-warning">
                  対象となる「系26-XXX」オペレーションの完了ログがありません。
                </p>
              </Show>

              <div class="manhour-submit-area">
                <Show
                  when={confirmManhourSubmit()}
                  fallback={
                    <button
                      onClick={() => setConfirmManhourSubmit(true)}
                      disabled={
                        preview().entries.length === 0 ||
                        preview().entries.some((entry) => entry.minutes <= 0) ||
                        preview().has_unfinished_logs ||
                        preview().attendance_work_minutes === null ||
                        manhourTotal() >
                          (preview().attendance_work_minutes ?? 0)
                      }
                    >
                      勤怠サイトへ送信
                    </button>
                  }
                >
                  <div class="manhour-confirm">
                    <span>
                      {preview().date} に {preview().entries.length}件・
                      {formatMinutes(manhourTotal())}を登録します。
                    </span>
                    <div>
                      <button
                        class="secondary"
                        onClick={() => setConfirmManhourSubmit(false)}
                      >
                        戻る
                      </button>
                      <button
                        onClick={submitManhours}
                        disabled={manhourSubmitting()}
                      >
                        {manhourSubmitting() ? "送信中" : "送信を確定"}
                      </button>
                    </div>
                  </div>
                </Show>
              </div>
            </>
          )}
        </Show>
      </section>

      <Show when={unresolved().length > 0}>
        <section class="history-warning">
          <span class="history-warning-icon">
            <Icon name="clock" size={17} />
          </span>
          <span class="min-w-0 flex-1">
            <strong>未終了の記録が {unresolved().length} 件あります</strong>
            <span>
              内容を確認して、削除または現在時刻で終了してください。
            </span>
          </span>
        </section>
      </Show>

      <Show when={loading()}>
        <div class="history-loading">
          <span class="launcher-spinner" />
          履歴を読み込んでいます
        </div>
      </Show>

      <Show when={!loading() && groupedEntries().length === 0}>
        <div class="history-empty">
          <Icon name="chart" size={25} />
          <p>この期間の作業履歴はありません</p>
          <span>タスクを開始すると、ここに記録されます。</span>
        </div>
      </Show>

      <div class="history-groups">
        <For each={groupedEntries()}>
          {(group) => (
            <section class="history-group">
              <header>
                <div>
                  <h2>{group.label}</h2>
                  <span>{group.key}</span>
                </div>
                <strong>{formatDuration(group.total)}</strong>
              </header>

              <div class="history-list">
                <For each={group.items}>
                  {(entry) => (
                    <article
                      class={`history-entry ${entry.is_active ? "is-active" : ""} ${!entry.end_time && !entry.is_active ? "is-unfinished" : ""}`}
                    >
                      <span class="history-entry-line" />
                      <div class="min-w-0 flex-1">
                        <div class="flex items-start justify-between gap-2">
                          <div class="min-w-0">
                            <p class="truncate">{entry.task_name}</p>
                            <span class="block truncate">
                              {entry.operation_name}
                            </span>
                          </div>
                          <strong>
                            {entry.duration_seconds === null && !entry.is_active
                              ? "未終了"
                              : formatDuration(entryDuration(entry))}
                          </strong>
                        </div>

                        <div class="history-entry-meta">
                          <span>
                            {formatTime(entry.start_time)}
                            {" — "}
                            {entry.end_time
                              ? formatTime(entry.end_time)
                              : entry.is_active
                                ? "計測中"
                                : "終了時刻なし"}
                          </span>
                          <Show when={entry.tag}>
                            <span class="tag">{entry.tag}</span>
                          </Show>
                        </div>

                        <Show when={!entry.end_time && !entry.is_active}>
                          <Show
                            when={confirmingId() === entry.id}
                            fallback={
                              <button
                                class="history-resolve"
                                onClick={() => setConfirmingId(entry.id)}
                              >
                                この記録を解消
                              </button>
                            }
                          >
                            <div class="history-resolve-actions">
                              <button
                                class="discard"
                                onClick={() => resolveEntry(entry, "discard")}
                              >
                                <Icon name="trash" size={12} />
                                削除
                              </button>
                              <button
                                onClick={() => resolveEntry(entry, "close_now")}
                              >
                                <Icon name="clock" size={12} />
                                今を終了時刻にする
                              </button>
                              <button onClick={() => setConfirmingId(null)}>
                                戻る
                              </button>
                            </div>
                          </Show>
                        </Show>
                      </div>
                    </article>
                  )}
                </For>
              </div>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
