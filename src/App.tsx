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

interface Task {
  id: string;
  name: string;
  tag: string;
  hidden: boolean;
}

interface Operation {
  id: string;
  name: string;
  description: string;
  tasks: Task[];
  hidden: boolean;
}

interface ActiveTaskInfo {
  task_id: string | null;
  task_name: string | null;
  operation_name: string | null;
  elapsed_seconds: number | null;
}

interface AppStateView {
  operations: Operation[];
  active: ActiveTaskInfo;
  today_seconds: Record<string, number>;
}

interface RecentTaskInfo {
  task_id: string;
  task_name: string;
  operation_name: string;
  tag: string;
}

function formatClock(total: number): string {
  const safe = Math.max(0, total);
  const h = Math.floor(safe / 3600);
  const m = Math.floor((safe % 3600) / 60);
  const s = safe % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function formatDuration(total: number): string {
  const safe = Math.max(0, total);
  if (safe < 60) return `${safe}s`;
  const minutes = Math.floor(safe / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function App() {
  const [operations, setOperations] = createSignal<Operation[]>([]);
  const [active, setActive] = createSignal<ActiveTaskInfo>({
    task_id: null,
    task_name: null,
    operation_name: null,
    elapsed_seconds: null,
  });
  const [elapsedSeconds, setElapsedSeconds] = createSignal(0);
  const [todaySeconds, setTodaySeconds] = createSignal<Record<string, number>>({});
  const [recentTasks, setRecentTasks] = createSignal<RecentTaskInfo[]>([]);

  const [pinned, setPinned] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [editMode, setEditMode] = createSignal(false);
  const [showHidden, setShowHidden] = createSignal(false);
  const [collapsedOps, setCollapsedOps] = createSignal<Set<string>>(new Set());
  const [toast, setToast] = createSignal<string | null>(null);

  const [showAddOperation, setShowAddOperation] = createSignal(false);
  const [addTaskForOpId, setAddTaskForOpId] = createSignal<string | null>(null);
  const [opName, setOpName] = createSignal("");
  const [opDescription, setOpDescription] = createSignal("");
  const [taskName, setTaskName] = createSignal("");
  const [taskTag, setTaskTag] = createSignal("");

  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenStateChanged: (() => void) | undefined;

  const notify = (message: string) => {
    if (toastTimer) clearTimeout(toastTimer);
    setToast(message);
    toastTimer = setTimeout(() => setToast(null), 3200);
  };

  const syncState = async () => {
    try {
      const [state, recent] = await Promise.all([
        invoke<AppStateView>("get_state"),
        invoke<RecentTaskInfo[]>("get_recent_tasks"),
      ]);
      setOperations(state.operations);
      setActive(state.active);
      setElapsedSeconds(state.active.elapsed_seconds ?? 0);
      setTodaySeconds(state.today_seconds);
      setRecentTasks(recent);
    } catch (error) {
      console.error("状態の同期に失敗しました", error);
      notify("状態を読み込めませんでした");
    }
  };

  const liveDelta = () => {
    if (!active().task_id) return 0;
    return Math.max(0, elapsedSeconds() - (active().elapsed_seconds ?? 0));
  };

  const totalToday = createMemo(
    () =>
      Object.values(todaySeconds()).reduce((sum, seconds) => sum + seconds, 0) +
      liveDelta(),
  );

  const taskToday = (taskId: string) =>
    (todaySeconds()[taskId] ?? 0) +
    (active().task_id === taskId ? liveDelta() : 0);

  const visibleOperations = createMemo(() => {
    const normalized = query().trim().toLocaleLowerCase();
    return operations()
      .filter((operation) => showHidden() || !operation.hidden)
      .map((operation) => ({
        ...operation,
        tasks: operation.tasks.filter((task) => {
          if (!showHidden() && task.hidden) return false;
          if (!normalized) return true;
          return [task.name, task.tag, operation.name, operation.description].some(
            (value) => value.toLocaleLowerCase().includes(normalized),
          );
        }),
      }))
      .filter((operation) => {
        if (!normalized) return true;
        const operationMatches = [operation.name, operation.description].some(
          (value) => value.toLocaleLowerCase().includes(normalized),
        );
        return operationMatches || operation.tasks.length > 0;
      });
  });

  const activeTaskCount = createMemo(() =>
    operations().reduce(
      (count, operation) =>
        count + operation.tasks.filter((task) => !task.hidden).length,
      0,
    ),
  );

  const startTask = async (taskId: string) => {
    try {
      await invoke("start_task", { taskId });
      await syncState();
    } catch (error) {
      console.error("タスクを開始できませんでした", error);
      notify("タスクを開始できませんでした");
    }
  };

  const stopTask = async () => {
    try {
      await invoke("stop_active_task");
      await syncState();
      notify("計測を停止しました");
    } catch (error) {
      console.error("計測を停止できませんでした", error);
      notify("計測を停止できませんでした");
    }
  };

  const togglePin = async () => {
    const next = !pinned();
    try {
      await invoke("set_always_on_top", { value: next });
      setPinned(next);
      notify(next ? "最前面に固定しました" : "最前面固定を解除しました");
    } catch (error) {
      console.error("最前面設定を変更できませんでした", error);
      notify("最前面設定を変更できませんでした");
    }
  };

  const exportCsv = async () => {
    try {
      await invoke<string>("export_csv");
      notify("CSVを書き出しました");
    } catch (error) {
      console.error("CSVを書き出せませんでした", error);
      notify("CSVを書き出せませんでした");
    }
  };

  const addOperation = async () => {
    if (!opName().trim()) return;
    try {
      await invoke("add_operation", {
        name: opName().trim(),
        description: opDescription().trim(),
      });
      setOpName("");
      setOpDescription("");
      setShowAddOperation(false);
      await syncState();
      notify("オペレーションを追加しました");
    } catch (error) {
      console.error("オペレーションを追加できませんでした", error);
      notify("追加できませんでした");
    }
  };

  const addTask = async (operationId: string) => {
    if (!taskName().trim()) return;
    try {
      await invoke("add_task", {
        operationId,
        name: taskName().trim(),
        tag: taskTag().trim(),
      });
      setTaskName("");
      setTaskTag("");
      setAddTaskForOpId(null);
      await syncState();
      notify("タスクを追加しました");
    } catch (error) {
      console.error("タスクを追加できませんでした", error);
      notify("タスクを追加できませんでした");
    }
  };

  const reorderOperation = async (opId: string, direction: "up" | "down") => {
    await invoke("reorder_operation", { opId, direction });
    await syncState();
  };

  const reorderTask = async (
    opId: string,
    taskId: string,
    direction: "up" | "down",
  ) => {
    await invoke("reorder_task", { opId, taskId, direction });
    await syncState();
  };

  const toggleOperationVisibility = async (opId: string) => {
    await invoke("toggle_operation_visibility", { opId });
    await syncState();
  };

  const toggleTaskVisibility = async (taskId: string) => {
    await invoke("toggle_task_visibility", { taskId });
    await syncState();
  };

  const toggleCollapse = (operationId: string) => {
    setCollapsedOps((current) => {
      const next = new Set(current);
      next.has(operationId)
        ? next.delete(operationId)
        : next.add(operationId);
      return next;
    });
  };

  const timer = setInterval(() => {
    if (active().task_id) setElapsedSeconds((seconds) => seconds + 1);
  }, 1000);

  onMount(async () => {
    await syncState();
    unlistenStateChanged = await listen("state-changed", syncState);
  });

  onCleanup(() => {
    clearInterval(timer);
    if (toastTimer) clearTimeout(toastTimer);
    unlistenStateChanged?.();
  });

  const iconButton =
    "grid h-8 w-8 place-items-center rounded-lg text-slate-400 transition hover:bg-white/7 hover:text-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400/70";
  const editIconButton =
    "grid h-7 w-7 place-items-center rounded-md text-slate-500 transition hover:bg-white/7 hover:text-slate-200 disabled:cursor-not-allowed disabled:opacity-25";

  return (
    <main class="app-shell">
      <header
        class="app-header"
        onMouseDown={(event) => {
          if (
            event.buttons === 1 &&
            !(event.target instanceof HTMLButtonElement)
          ) {
            invoke("start_dragging").catch(() => undefined);
          }
        }}
      >
        <div class="flex min-w-0 items-center gap-2.5 pointer-events-none">
          <div class="brand-mark">
            <Icon name="timer" size={15} />
          </div>
          <div class="min-w-0">
            <p class="truncate text-[13px] font-semibold tracking-tight text-slate-100">
              Tempomezurado
            </p>
            <p class="text-[9px] font-medium uppercase tracking-[0.18em] text-slate-500">
              Focus timer
            </p>
          </div>
        </div>

        <div class="flex items-center gap-0.5">
          <button class={iconButton} onClick={exportCsv} title="CSVを書き出す">
            <Icon name="download" size={15} />
          </button>
          <button
            class={`${iconButton} ${pinned() ? "!bg-indigo-500/15 !text-indigo-300" : ""}`}
            onClick={togglePin}
            title={pinned() ? "最前面固定を解除" : "最前面に固定"}
          >
            <Icon name="pin" size={15} />
          </button>
        </div>
      </header>

      <div class="app-scroll">
        <section class={`timer-hero ${active().task_id ? "is-running" : ""}`}>
          <div class="mb-5 flex items-center justify-between">
            <div
              class={`status-pill ${active().task_id ? "is-running" : ""}`}
            >
              <span class="status-dot" />
              {active().task_id ? "計測中" : "待機中"}
            </div>
            <span class="text-[10px] font-medium text-slate-500">
              今日 {formatDuration(totalToday())}
            </span>
          </div>

          <p class="timer-digits">{formatClock(elapsedSeconds())}</p>

          <div class="mt-4 min-h-10">
            <Show
              when={active().task_id}
              fallback={
                <div>
                  <p class="text-sm font-medium text-slate-300">
                    次に取り組むタスクを選択
                  </p>
                  <p class="mt-1 text-[11px] text-slate-500">
                    最近のタスク、または一覧からすぐ開始できます
                  </p>
                </div>
              }
            >
              <p class="truncate text-[11px] font-medium text-indigo-300">
                {active().operation_name}
              </p>
              <p class="mt-0.5 truncate text-sm font-semibold text-white">
                {active().task_name}
              </p>
            </Show>
          </div>

          <Show when={active().task_id}>
            <button class="stop-button" onClick={stopTask}>
              <Icon name="stop" size={14} />
              計測を停止
            </button>
          </Show>
        </section>

        <section class="summary-grid">
          <div class="summary-card">
            <p class="summary-label">Today</p>
            <p class="summary-value">{formatDuration(totalToday())}</p>
          </div>
          <div class="summary-card">
            <p class="summary-label">Tasks</p>
            <p class="summary-value">{activeTaskCount()}</p>
          </div>
          <div class="summary-card">
            <p class="summary-label">Status</p>
            <p
              class={`summary-value text-xs ${active().task_id ? "!text-emerald-300" : ""}`}
            >
              {active().task_id ? "Focusing" : "Ready"}
            </p>
          </div>
        </section>

        <Show when={recentTasks().length > 0}>
          <section class="content-section">
            <div class="section-heading">
              <div>
                <p class="eyebrow">Quick start</p>
                <h2>最近のタスク</h2>
              </div>
              <Icon name="spark" size={16} class="text-amber-300/80" />
            </div>

            <div class="space-y-1.5">
              <For each={recentTasks().slice(0, 3)}>
                {(task) => (
                  <button
                    class={`quick-task ${active().task_id === task.task_id ? "is-active" : ""}`}
                    onClick={() => startTask(task.task_id)}
                  >
                    <span class="quick-task-icon">
                      <Icon name="timer" size={15} />
                    </span>
                    <span class="min-w-0 flex-1 text-left">
                      <span class="block truncate text-xs font-semibold text-slate-200">
                        {task.task_name}
                      </span>
                      <span class="mt-0.5 block truncate text-[10px] text-slate-500">
                        {task.operation_name}
                      </span>
                    </span>
                    <Show when={task.tag}>
                      <span class="tag">{task.tag}</span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </section>
        </Show>

        <section class="content-section pb-5">
          <div class="section-heading">
            <div>
              <p class="eyebrow">Workspace</p>
              <h2>タスク一覧</h2>
            </div>
            <button
              class={`edit-toggle ${editMode() ? "is-active" : ""}`}
              onClick={() => {
                setEditMode((value) => !value);
                setShowHidden(false);
              }}
            >
              <Icon name={editMode() ? "x" : "edit"} size={13} />
              {editMode() ? "完了" : "編集"}
            </button>
          </div>

          <div class="mb-3 flex items-center gap-2">
            <label class="search-field">
              <Icon name="search" size={14} />
              <input
                value={query()}
                onInput={(event) => setQuery(event.currentTarget.value)}
                placeholder="タスクやタグを検索"
              />
              <Show when={query()}>
                <button
                  type="button"
                  class="text-slate-500 hover:text-slate-200"
                  onClick={() => setQuery("")}
                >
                  <Icon name="x" size={13} />
                </button>
              </Show>
            </label>

            <Show when={editMode()}>
              <button
                class={`visibility-toggle ${showHidden() ? "is-active" : ""}`}
                onClick={() => setShowHidden((value) => !value)}
                title="非表示項目を確認"
              >
                <Icon name={showHidden() ? "eye" : "eye-off"} size={15} />
              </button>
            </Show>
          </div>

          <div class="space-y-2">
            <For each={visibleOperations()}>
              {(operation, operationIndex) => {
                const collapsed = () => collapsedOps().has(operation.id);
                return (
                  <article
                    class={`operation-card ${operation.hidden ? "is-hidden" : ""}`}
                  >
                    <div class="operation-header">
                      <button
                        class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
                        onClick={() => toggleCollapse(operation.id)}
                      >
                        <span class="chevron">
                          <Icon
                            name={
                              collapsed() ? "chevron-right" : "chevron-down"
                            }
                            size={14}
                          />
                        </span>
                        <span class="min-w-0">
                          <span class="block truncate text-xs font-semibold text-slate-200">
                            {operation.name}
                          </span>
                          <Show when={operation.description}>
                            <span class="mt-0.5 block truncate text-[10px] text-slate-500">
                              {operation.description}
                            </span>
                          </Show>
                        </span>
                      </button>

                      <Show
                        when={editMode()}
                        fallback={
                          <span class="rounded-md bg-white/4 px-1.5 py-1 text-[9px] font-medium text-slate-500">
                            {operation.tasks.filter((task) => !task.hidden).length}
                          </span>
                        }
                      >
                        <div class="flex items-center">
                          <button
                            class={editIconButton}
                            disabled={operationIndex() === 0}
                            onClick={() =>
                              reorderOperation(operation.id, "up")
                            }
                            title="上へ"
                          >
                            <Icon name="arrow-up" size={13} />
                          </button>
                          <button
                            class={editIconButton}
                            disabled={
                              operationIndex() === visibleOperations().length - 1
                            }
                            onClick={() =>
                              reorderOperation(operation.id, "down")
                            }
                            title="下へ"
                          >
                            <Icon name="arrow-down" size={13} />
                          </button>
                          <button
                            class={editIconButton}
                            onClick={() =>
                              toggleOperationVisibility(operation.id)
                            }
                            title={operation.hidden ? "再表示" : "非表示"}
                          >
                            <Icon
                              name={operation.hidden ? "eye-off" : "eye"}
                              size={13}
                            />
                          </button>
                          <button
                            class={editIconButton}
                            onClick={() => {
                              setAddTaskForOpId(
                                addTaskForOpId() === operation.id
                                  ? null
                                  : operation.id,
                              );
                              setTaskName("");
                              setTaskTag("");
                            }}
                            title="タスクを追加"
                          >
                            <Icon name="add" size={14} />
                          </button>
                        </div>
                      </Show>
                    </div>

                    <Show when={!collapsed()}>
                      <div class="task-list">
                        <For each={operation.tasks}>
                          {(task, taskIndex) => (
                            <div
                              class={`task-row ${active().task_id === task.id ? "is-active" : ""} ${task.hidden ? "is-hidden" : ""}`}
                            >
                              <button
                                class="min-w-0 flex-1 py-2 text-left"
                                disabled={task.hidden}
                                onClick={() => startTask(task.id)}
                              >
                                <span class="flex items-center gap-2">
                                  <span class="task-indicator">
                                    <span />
                                  </span>
                                  <span class="min-w-0">
                                    <span class="block truncate text-xs font-medium text-slate-300">
                                      {task.name}
                                    </span>
                                    <span class="mt-0.5 flex items-center gap-1.5">
                                      <Show when={task.tag}>
                                        <span class="tag">{task.tag}</span>
                                      </Show>
                                      <Show when={taskToday(task.id) > 0}>
                                        <span class="text-[9px] font-medium tabular-nums text-slate-500">
                                          {formatDuration(taskToday(task.id))}
                                        </span>
                                      </Show>
                                    </span>
                                  </span>
                                </span>
                              </button>

                              <Show when={editMode()}>
                                <div class="flex items-center">
                                  <button
                                    class={editIconButton}
                                    disabled={taskIndex() === 0}
                                    onClick={() =>
                                      reorderTask(operation.id, task.id, "up")
                                    }
                                  >
                                    <Icon name="arrow-up" size={12} />
                                  </button>
                                  <button
                                    class={editIconButton}
                                    disabled={
                                      taskIndex() === operation.tasks.length - 1
                                    }
                                    onClick={() =>
                                      reorderTask(operation.id, task.id, "down")
                                    }
                                  >
                                    <Icon name="arrow-down" size={12} />
                                  </button>
                                  <button
                                    class={editIconButton}
                                    onClick={() =>
                                      toggleTaskVisibility(task.id)
                                    }
                                  >
                                    <Icon
                                      name={task.hidden ? "eye-off" : "eye"}
                                      size={12}
                                    />
                                  </button>
                                </div>
                              </Show>
                            </div>
                          )}
                        </For>

                        <Show when={operation.tasks.length === 0}>
                          <p class="px-3 py-4 text-center text-[10px] text-slate-600">
                            タスクはまだありません
                          </p>
                        </Show>

                        <Show when={addTaskForOpId() === operation.id}>
                          <div class="inline-form">
                            <input
                              value={taskName()}
                              onInput={(event) =>
                                setTaskName(event.currentTarget.value)
                              }
                              onKeyDown={(event) =>
                                event.key === "Enter" && addTask(operation.id)
                              }
                              placeholder="タスク名"
                              autofocus
                            />
                            <input
                              value={taskTag()}
                              onInput={(event) =>
                                setTaskTag(event.currentTarget.value)
                              }
                              onKeyDown={(event) =>
                                event.key === "Enter" && addTask(operation.id)
                              }
                              placeholder="タグ（任意）"
                            />
                            <div class="flex gap-1.5">
                              <button
                                class="secondary-button"
                                onClick={() => setAddTaskForOpId(null)}
                              >
                                キャンセル
                              </button>
                              <button
                                class="primary-button"
                                onClick={() => addTask(operation.id)}
                              >
                                追加
                              </button>
                            </div>
                          </div>
                        </Show>
                      </div>
                    </Show>
                  </article>
                );
              }}
            </For>

            <Show when={visibleOperations().length === 0}>
              <div class="empty-state">
                <Icon name="search" size={20} />
                <p class="mt-2 text-xs font-medium text-slate-400">
                  一致するタスクがありません
                </p>
                <button class="mt-2 text-[10px] text-indigo-300" onClick={() => setQuery("")}>
                  検索をクリア
                </button>
              </div>
            </Show>
          </div>

          <Show when={editMode()}>
            <div class="mt-3">
              <Show
                when={showAddOperation()}
                fallback={
                  <button
                    class="add-operation-button"
                    onClick={() => setShowAddOperation(true)}
                  >
                    <Icon name="add" size={14} />
                    オペレーションを追加
                  </button>
                }
              >
                <div class="inline-form rounded-xl border border-white/7 bg-white/[0.025]">
                  <input
                    value={opName()}
                    onInput={(event) => setOpName(event.currentTarget.value)}
                    onKeyDown={(event) =>
                      event.key === "Enter" && addOperation()
                    }
                    placeholder="オペレーション名"
                    autofocus
                  />
                  <input
                    value={opDescription()}
                    onInput={(event) =>
                      setOpDescription(event.currentTarget.value)
                    }
                    onKeyDown={(event) =>
                      event.key === "Enter" && addOperation()
                    }
                    placeholder="説明（任意）"
                  />
                  <div class="flex gap-1.5">
                    <button
                      class="secondary-button"
                      onClick={() => setShowAddOperation(false)}
                    >
                      キャンセル
                    </button>
                    <button class="primary-button" onClick={addOperation}>
                      追加
                    </button>
                  </div>
                </div>
              </Show>
            </div>
          </Show>
        </section>
      </div>

      <footer class="app-footer">
        <span>
          <kbd>Ctrl</kbd> <span>+</span> <kbd>Shift</kbd> <span>+</span>{" "}
          <kbd>Space</kbd>
        </span>
        <span>クイックランチャー</span>
      </footer>

      <Show when={toast()}>
        <div class="toast" role="status">
          <span class="h-1.5 w-1.5 rounded-full bg-indigo-400" />
          {toast()}
        </div>
      </Show>
    </main>
  );
}

export default App;
