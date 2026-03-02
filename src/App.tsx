import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ============================================================
// 型定義 (Rust の AppStateView に対応)
// ============================================================

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

// ============================================================
// ユーティリティ
// ============================================================

function formatSeconds(total: number): string {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** 当日合計時間の短縮表示 (例: 3h 12m / 45m / 30s) */
function formatDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}h ${m % 60}m`;
  return `${m}m`;
}

// ============================================================
// App コンポーネント
// ============================================================

function App() {
  // --- コアな状態 ---
  const [operations, setOperations] = createSignal<Operation[]>([]);
  const [activeInfo, setActiveInfo] = createSignal<ActiveTaskInfo>({
    task_id: null,
    task_name: null,
    operation_name: null,
    elapsed_seconds: null,
  });
  const [elapsedSeconds, setElapsedSeconds] = createSignal(0);
  const [todaySeconds, setTodaySeconds] = createSignal<Record<string, number>>({});

  // --- ピン / エクスポート ---
  const [pinned, setPinned] = createSignal(false);
  const [exportMsg, setExportMsg] = createSignal<string | null>(null);

  // --- 非表示項目の表示切り替え ---
  const [showHidden, setShowHidden] = createSignal(false);

  // --- フォーム表示制御 ---
  const [showAddOperation, setShowAddOperation] = createSignal(false);
  const [addTaskForOpId, setAddTaskForOpId] = createSignal<string | null>(null);

  // --- フォーム入力値 ---
  const [opName, setOpName] = createSignal("");
  const [opDesc, setOpDesc] = createSignal("");
  const [taskName, setTaskName] = createSignal("");
  const [taskTag, setTaskTag] = createSignal("");

  // --- アコーディオン: 折り畳まれているオペレーション ID のセット ---
  const [collapsedOps, setCollapsedOps] = createSignal<Set<string>>(new Set());

  const toggleCollapse = (opId: string) => {
    setCollapsedOps((prev) => {
      const next = new Set(prev);
      if (next.has(opId)) {
        next.delete(opId);
      } else {
        next.add(opId);
      }
      return next;
    });
  };

  const isCollapsed = (opId: string) => collapsedOps().has(opId);

  // --- Tab キーナビゲーション: 現在フォーカス中のタスクのフラットインデックス (-1 = 非アクティブ) ---
  const [focusedTaskIndex, setFocusedTaskIndex] = createSignal(-1);

  // 表示中かつ折り畳まれていないタスク ID をフラットに列挙 (Tab ナビ用)
  const flatTaskIds = (): string[] => {
    const ids: string[] = [];
    for (const op of operations()) {
      if (op.hidden && !showHidden()) continue;
      if (isCollapsed(op.id)) continue;
      for (const task of op.tasks) {
        if (task.hidden && !showHidden()) continue;
        ids.push(task.id);
      }
    }
    return ids;
  };

  // --- Rust との同期 ---
  const syncState = async () => {
    const state = await invoke<AppStateView>("get_state");
    setOperations(state.operations);
    setActiveInfo(state.active);
    // タイマーの基点を Rust の elapsed_seconds で上書き (絶対ルール: Rust が SSOT)
    setElapsedSeconds(state.active.elapsed_seconds ?? 0);
    setTodaySeconds(state.today_seconds);
  };

  // 1 秒ごとにカウントアップ (表示のみ)
  const timer = setInterval(() => {
    if (activeInfo().task_id !== null) {
      setElapsedSeconds((s) => s + 1);
    }
  }, 1000);

  // --- タスク操作 ---
  const handleStartTask = async (taskId: string) => {
    try {
      await invoke("start_task", { taskId });
      await syncState();
    } catch (e) {
      console.error("start_task エラー:", e);
    }
  };

  const handleStopTask = async () => {
    try {
      await invoke("stop_active_task");
      await syncState();
    } catch (e) {
      console.error("stop_active_task エラー:", e);
    }
  };

  // --- ピン (常時最前面) トグル ---
  const handleTogglePin = async () => {
    const next = !pinned();
    try {
      await invoke("set_always_on_top", { value: next });
      setPinned(next);
    } catch (e) {
      console.error("set_always_on_top エラー:", e);
    }
  };

  // --- CSV エクスポート ---
  const handleExport = async () => {
    try {
      const path = await invoke<string>("export_csv");
      setExportMsg(path);
      setTimeout(() => setExportMsg(null), 5000);
    } catch (e) {
      console.error("export_csv エラー:", e);
      setExportMsg("エクスポート失敗");
      setTimeout(() => setExportMsg(null), 3000);
    }
  };

  // --- Operation 追加 ---
  const handleAddOperation = async () => {
    if (!opName().trim()) return;
    try {
      await invoke("add_operation", {
        name: opName().trim(),
        description: opDesc().trim(),
      });
      setOpName("");
      setOpDesc("");
      setShowAddOperation(false);
      await syncState();
    } catch (e) {
      console.error("add_operation エラー:", e);
    }
  };

  // --- Task 追加 ---
  const handleAddTask = async (operationId: string) => {
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
    } catch (e) {
      console.error("add_task エラー:", e);
    }
  };

  // --- 並べ替え ---
  const handleReorderOperation = async (opId: string, direction: "up" | "down") => {
    try {
      await invoke("reorder_operation", { opId, direction });
      await syncState();
    } catch (e) {
      console.error("reorder_operation エラー:", e);
    }
  };

  const handleReorderTask = async (
    opId: string,
    taskId: string,
    direction: "up" | "down"
  ) => {
    try {
      await invoke("reorder_task", { opId, taskId, direction });
      await syncState();
    } catch (e) {
      console.error("reorder_task エラー:", e);
    }
  };

  // --- 表示/非表示 ---
  const handleToggleOperationVisibility = async (opId: string) => {
    try {
      await invoke("toggle_operation_visibility", { opId });
      await syncState();
    } catch (e) {
      console.error("toggle_operation_visibility エラー:", e);
    }
  };

  const handleToggleTaskVisibility = async (taskId: string) => {
    try {
      await invoke("toggle_task_visibility", { taskId });
      await syncState();
    } catch (e) {
      console.error("toggle_task_visibility エラー:", e);
    }
  };

  // --- Tab / Enter / Escape によるキーボードナビゲーション ---
  // input/textarea にフォーカスがある場合はスキップして通常入力を妨げない
  const handleKeyDown = (e: KeyboardEvent) => {
    if (
      e.target instanceof HTMLInputElement ||
      e.target instanceof HTMLTextAreaElement
    ) {
      return;
    }

    const ids = flatTaskIds();
    if (ids.length === 0) return;

    if (e.key === "Tab") {
      e.preventDefault();
      const cur = focusedTaskIndex();
      const next = e.shiftKey
        ? cur <= 0
          ? ids.length - 1
          : cur - 1
        : cur < 0 || cur >= ids.length - 1
          ? 0
          : cur + 1;
      setFocusedTaskIndex(next);
      requestAnimationFrame(() => {
        document
          .querySelector(`[data-task-id="${ids[next]}"]`)
          ?.scrollIntoView({ block: "nearest" });
      });
    } else if (e.key === "Enter") {
      const idx = focusedTaskIndex();
      if (idx >= 0 && idx < ids.length) {
        setFocusedTaskIndex(-1);
        handleStartTask(ids[idx]);
      }
    } else if (e.key === "Escape") {
      setFocusedTaskIndex(-1);
    }
  };

  // ランチャーや他ウィンドウから start_task/stop_active_task が呼ばれた際の自動 sync
  let unlistenStateChanged: (() => void) | undefined;

  onMount(async () => {
    await syncState();

    unlistenStateChanged = await listen("state-changed", async () => {
      await syncState();
    });

    document.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    clearInterval(timer);
    unlistenStateChanged?.();
    document.removeEventListener("keydown", handleKeyDown);
  });

  // ============================================================
  // UI
  // ============================================================

  // 並べ替え/非表示ボタンの共通スタイル
  const ctrlBtn =
    "w-5 h-5 flex items-center justify-center rounded text-xs transition-colors";

  return (
    <main class="w-screen h-screen bg-gray-900 text-white font-sans flex flex-col overflow-hidden text-sm">

      {/*
        カスタムヘッダー (ドラッグ移動)
      */}
      <div
        onMouseDown={(e) => {
          if (e.buttons === 1 && !(e.target instanceof HTMLButtonElement)) {
            invoke("start_dragging").catch(() => {});
          }
        }}
        class="h-8 bg-gray-800 flex items-center justify-between px-2 shrink-0 cursor-move select-none border-b border-gray-700"
      >
        <span class="text-xs font-bold text-gray-400 pointer-events-none">
          Tempomezurado
        </span>

        {/* ヘッダーボタン群 */}
        <div class="flex items-center gap-1">
          {/* CSV エクスポートボタン */}
          <button
            onClick={handleExport}
            title="CSV エクスポート"
            class="w-6 h-6 flex items-center justify-center text-gray-500 hover:text-green-400 rounded transition-colors text-xs"
          >
            ↓
          </button>
          {/* 非表示項目の表示トグル */}
          <button
            onClick={() => setShowHidden((v) => !v)}
            title={showHidden() ? "非表示項目を隠す" : "非表示項目を表示する"}
            class={`w-6 h-6 flex items-center justify-center rounded transition-colors text-xs ${
              showHidden()
                ? "text-orange-400 hover:text-orange-300"
                : "text-gray-500 hover:text-gray-300"
            }`}
          >
            👁
          </button>
          {/* ピン (常時最前面) トグル */}
          <button
            onClick={handleTogglePin}
            title={pinned() ? "最前面固定を解除" : "最前面に固定"}
            class={`w-6 h-6 flex items-center justify-center rounded transition-colors text-xs ${
              pinned()
                ? "text-blue-400 hover:text-blue-300"
                : "text-gray-500 hover:text-gray-300"
            }`}
          >
            {pinned() ? "📌" : "📍"}
          </button>
        </div>
      </div>

      {/* タイマー表示エリア */}
      <div class="shrink-0 flex flex-col items-center gap-2 py-4 bg-gray-800 border-b border-gray-700">
        {/* 時間表示ブロック: 計測中に呼吸する枠線グロー */}
        <div
          class={`px-8 py-2 rounded-xl border transition-colors duration-500 ${
            activeInfo().task_id !== null
              ? "border-blue-500/50 animate-timer-breathe"
              : "border-gray-700/60"
          }`}
        >
          <div
            class={`text-4xl font-mono tabular-nums tracking-widest transition-colors duration-300 ${
              activeInfo().task_id !== null ? "text-blue-100" : "text-gray-300"
            }`}
          >
            {formatSeconds(elapsedSeconds())}
          </div>
        </div>

        <Show
          when={activeInfo().task_id !== null}
          fallback={<p class="text-gray-500 text-xs">タスクを選択してください</p>}
        >
          <p class="text-blue-400 text-xs max-w-56 truncate">
            {activeInfo().operation_name} › {activeInfo().task_name}
          </p>
          <button
            onClick={handleStopTask}
            class="mt-1 px-4 py-1 bg-red-800 hover:bg-red-700 rounded text-xs transition-colors"
          >
            ■ 停止
          </button>
        </Show>
      </div>

      {/* エクスポート結果メッセージ */}
      <Show when={exportMsg() !== null}>
        <div class="mx-2 mt-2 px-3 py-1.5 bg-gray-700 rounded text-xs text-gray-300 truncate">
          📄 {exportMsg()}
        </div>
      </Show>

      {/* Operation / Task リスト */}
      <div class="flex-1 overflow-y-auto p-2 space-y-2">
        <For
          each={
            showHidden()
              ? operations()
              : operations().filter((op) => !op.hidden)
          }
        >
          {(op) => (
            <div
              class={`rounded-md overflow-hidden border border-gray-700 ${
                op.hidden ? "opacity-60" : ""
              }`}
            >
              {/* Operation ヘッダー */}
              <div class="flex items-center justify-between px-3 py-2 bg-gray-700">
                {/* 折り畳みトグル + タイトル */}
                <div
                  class="min-w-0 flex-1 flex items-start gap-1.5 cursor-pointer select-none"
                  onClick={() => toggleCollapse(op.id)}
                >
                  <span class="text-gray-400 text-xs mt-0.5 shrink-0 w-3 text-center">
                    {isCollapsed(op.id) ? "▶" : "▼"}
                  </span>
                  <div class="min-w-0">
                    <span class="font-semibold text-gray-200 truncate block">
                      {op.name}
                    </span>
                    <Show when={op.description}>
                      <span class="text-gray-500 text-xs truncate block">
                        {op.description}
                      </span>
                    </Show>
                  </div>
                </div>

                {/* 操作ボタン群 */}
                <div class="flex items-center gap-0.5 ml-2 shrink-0">
                  <button
                    onClick={() => handleReorderOperation(op.id, "up")}
                    disabled={operations()[0]?.id === op.id}
                    title="上に移動"
                    class={`${ctrlBtn} text-gray-600 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed`}
                  >
                    ↑
                  </button>
                  <button
                    onClick={() => handleReorderOperation(op.id, "down")}
                    disabled={
                      operations()[operations().length - 1]?.id === op.id
                    }
                    title="下に移動"
                    class={`${ctrlBtn} text-gray-600 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed`}
                  >
                    ↓
                  </button>
                  <button
                    onClick={() => handleToggleOperationVisibility(op.id)}
                    title={op.hidden ? "再表示する" : "非表示にする"}
                    class={`${ctrlBtn} ${
                      op.hidden
                        ? "text-orange-400 hover:text-orange-300"
                        : "text-gray-600 hover:text-gray-300"
                    }`}
                  >
                    👁
                  </button>
                  <Show when={!op.hidden}>
                    <button
                      onClick={() => {
                        setAddTaskForOpId(
                          addTaskForOpId() === op.id ? null : op.id
                        );
                        setTaskName("");
                        setTaskTag("");
                      }}
                      class={`${ctrlBtn} text-gray-500 hover:text-blue-400`}
                    >
                      ＋
                    </button>
                  </Show>
                </div>
              </div>

              {/* Task リスト (折り畳み中は非表示) */}
              <Show when={!isCollapsed(op.id)}>
              <For
                each={
                  showHidden()
                    ? op.tasks
                    : op.tasks.filter((t) => !t.hidden)
                }
              >
                {(task) => {
                  const isActive = () => activeInfo().task_id === task.id;
                  // このタスクのグローバルインデックスを都度計算
                  const globalIdx = () => flatTaskIds().indexOf(task.id);
                  const isFocused = () =>
                    focusedTaskIndex() >= 0 &&
                    focusedTaskIndex() === globalIdx();

                  return (
                    <div
                      data-task-id={task.id}
                      class={`w-full flex items-center px-3 py-2 border-t border-gray-700 transition-colors gap-1 ${
                        task.hidden
                          ? "bg-gray-900 opacity-60"
                          : isActive()
                            ? "bg-blue-950 text-blue-300"
                            : isFocused()
                              ? "bg-gray-700 text-white ring-2 ring-inset ring-blue-400"
                              : "bg-gray-800 text-gray-300"
                      }`}
                    >
                      {/* タスク名エリア: クリックで計測開始 */}
                      <div
                        onClick={() => {
                          if (!task.hidden) {
                            setFocusedTaskIndex(-1);
                            handleStartTask(task.id);
                          }
                        }}
                        class={`flex-1 min-w-0 flex items-center gap-1.5 ${
                          !task.hidden
                            ? "cursor-pointer hover:text-white"
                            : "cursor-default"
                        }`}
                      >
                        <span class="truncate">{task.name}</span>
                        <Show when={task.tag}>
                          <span class="text-xs px-1.5 py-0.5 rounded bg-gray-700 text-gray-400">
                            {task.tag}
                          </span>
                        </Show>
                        <Show when={isFocused()}>
                          <span class="text-blue-400 text-xs font-bold">↵</span>
                        </Show>
                      </div>

                      {/* 当日合計時間 */}
                      <Show when={(todaySeconds()[task.id] ?? 0) > 0}>
                        <span
                          class={`text-xs tabular-nums shrink-0 ${
                            isActive() ? "text-blue-400" : "text-gray-500"
                          }`}
                        >
                          {formatDuration(todaySeconds()[task.id] ?? 0)}
                        </span>
                      </Show>

                      {/* 操作ボタン群 */}
                      <div class="flex items-center gap-0.5 shrink-0">
                        <button
                          onClick={() =>
                            handleReorderTask(op.id, task.id, "up")
                          }
                          disabled={op.tasks[0]?.id === task.id}
                          title="上に移動"
                          class={`${ctrlBtn} text-gray-600 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed`}
                        >
                          ↑
                        </button>
                        <button
                          onClick={() =>
                            handleReorderTask(op.id, task.id, "down")
                          }
                          disabled={
                            op.tasks[op.tasks.length - 1]?.id === task.id
                          }
                          title="下に移動"
                          class={`${ctrlBtn} text-gray-600 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed`}
                        >
                          ↓
                        </button>
                        <button
                          onClick={() => handleToggleTaskVisibility(task.id)}
                          title={task.hidden ? "再表示する" : "非表示にする"}
                          class={`${ctrlBtn} ${
                            task.hidden
                              ? "text-orange-400 hover:text-orange-300"
                              : "text-gray-600 hover:text-gray-300"
                          }`}
                        >
                          👁
                        </button>
                      </div>
                    </div>
                  );
                }}
              </For>
              </Show>

              {/* タスク追加フォーム */}
              <Show when={addTaskForOpId() === op.id}>
                <div class="px-3 py-2 bg-gray-800 border-t border-gray-700 space-y-1.5">
                  <input
                    type="text"
                    placeholder="タスク名"
                    value={taskName()}
                    onInput={(e) => setTaskName(e.currentTarget.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleAddTask(op.id)}
                    class="w-full bg-gray-700 rounded px-2 py-1 text-xs outline-none focus:ring-1 focus:ring-blue-500 placeholder-gray-500"
                    autofocus
                  />
                  <input
                    type="text"
                    placeholder="タグ (例: dev, review)"
                    value={taskTag()}
                    onInput={(e) => setTaskTag(e.currentTarget.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleAddTask(op.id)}
                    class="w-full bg-gray-700 rounded px-2 py-1 text-xs outline-none focus:ring-1 focus:ring-blue-500 placeholder-gray-500"
                  />
                  <div class="flex gap-1">
                    <button
                      onClick={() => handleAddTask(op.id)}
                      class="flex-1 py-1 bg-blue-700 hover:bg-blue-600 rounded text-xs transition-colors"
                    >
                      追加
                    </button>
                    <button
                      onClick={() => setAddTaskForOpId(null)}
                      class="flex-1 py-1 bg-gray-600 hover:bg-gray-500 rounded text-xs transition-colors"
                    >
                      キャンセル
                    </button>
                  </div>
                </div>
              </Show>

            </div>
          )}
        </For>

        {/* Operation 追加 */}
        <Show
          when={showAddOperation()}
          fallback={
            <button
              onClick={() => setShowAddOperation(true)}
              class="w-full py-2 border border-dashed border-gray-600 hover:border-gray-400 rounded-md text-gray-500 hover:text-gray-300 text-xs transition-colors"
            >
              ＋ オペレーション追加
            </button>
          }
        >
          <div class="rounded-md border border-gray-700 bg-gray-800 p-3 space-y-1.5">
            <input
              type="text"
              placeholder="オペレーション名"
              value={opName()}
              onInput={(e) => setOpName(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Enter" && handleAddOperation()}
              class="w-full bg-gray-700 rounded px-2 py-1 text-xs outline-none focus:ring-1 focus:ring-blue-500 placeholder-gray-500"
              autofocus
            />
            <input
              type="text"
              placeholder="説明 (任意)"
              value={opDesc()}
              onInput={(e) => setOpDesc(e.currentTarget.value)}
              class="w-full bg-gray-700 rounded px-2 py-1 text-xs outline-none focus:ring-1 focus:ring-blue-500 placeholder-gray-500"
            />
            <div class="flex gap-1">
              <button
                onClick={handleAddOperation}
                class="flex-1 py-1 bg-blue-700 hover:bg-blue-600 rounded text-xs transition-colors"
              >
                追加
              </button>
              <button
                onClick={() => {
                  setShowAddOperation(false);
                  setOpName("");
                  setOpDesc("");
                }}
                class="flex-1 py-1 bg-gray-600 hover:bg-gray-500 rounded text-xs transition-colors"
              >
                キャンセル
              </button>
            </div>
          </div>
        </Show>
      </div>

      {/* ショートカット情報 */}
      <div class="shrink-0 px-3 py-1.5 bg-gray-800 border-t border-gray-700">
        <p class="text-gray-600 text-xs text-center">
          Ctrl+Shift+Space → Tab でタスク選択 → Enter で開始
        </p>
      </div>
    </main>
  );
}

export default App;
