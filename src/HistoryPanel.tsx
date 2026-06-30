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

export default function HistoryPanel(props: HistoryPanelProps) {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([]);
  const [days, setDays] = createSignal<7 | 30 | 0>(7);
  const [loading, setLoading] = createSignal(true);
  const [confirmingId, setConfirmingId] = createSignal<string | null>(null);
  const [clock, setClock] = createSignal(Date.now());
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

  onMount(async () => {
    const timer = setInterval(() => setClock(Date.now()), 1000);
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
