import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ============================================================
// 型定義 (Rust の AppStateView に対応)
// ============================================================

interface TimeLog {
  start_time: string;
  end_time: string | null;
}

interface Task {
  id: string;
  name: string;
  tag: string;
  time_logs: TimeLog[];
}

interface Operation {
  id: string;
  name: string;
  description: string;
  tasks: Task[];
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

  // --- Phase 4: ピン / エクスポート ---
  const [pinned, setPinned] = createSignal(false);
  const [exportMsg, setExportMsg] = createSignal<string | null>(null);

  // --- フォーム表示制御 ---
  const [showAddOperation, setShowAddOperation] = createSignal(false);
  const [addTaskForOpId, setAddTaskForOpId] = createSignal<string | null>(null);

  // --- フォーム入力値 ---
  const [opName, setOpName] = createSignal("");
  const [opDesc, setOpDesc] = createSignal("");
  const [taskName, setTaskName] = createSignal("");
  const [taskTag, setTaskTag] = createSignal("");

  // --- Tab キーナビゲーション: 現在フォーカス中のタスクのフラットインデックス (-1 = 非アクティブ) ---
  const [focusedTaskIndex, setFocusedTaskIndex] = createSignal(-1);

  // 全オペレーション配下のタスク ID をフラットに列挙 (Tab ナビ用)
  const flatTaskIds = (): string[] => {
    const ids: string[] = [];
    for (const op of operations()) {
      for (const task of op.tasks) {
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
      // DOM 更新後にスクロール
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

  // --- グローバルショートカット (Ctrl+Shift+Space) で Rust がウィンドウをフォーカス → イベント受信 ---
  let unlistenWindowActivated: (() => void) | undefined;

  onMount(async () => {
    await syncState();

    unlistenWindowActivated = await listen("window-activated", async () => {
      // Rust 側で既にウィンドウは最前面に引き上げ済み
      // 状態を最新化してから最初のタスクにフォーカスを移す
      await syncState();
      const ids = flatTaskIds();
      setFocusedTaskIndex(ids.length > 0 ? 0 : -1);
      requestAnimationFrame(() => {
        if (ids.length > 0) {
          document
            .querySelector(`[data-task-id="${ids[0]}"]`)
            ?.scrollIntoView({ block: "nearest" });
        }
      });
    });

    document.addEventListener("keydown", handleKeyDown);
  });

  onCleanup(() => {
    clearInterval(timer);
    unlistenWindowActivated?.();
    document.removeEventListener("keydown", handleKeyDown);
  });

  // ============================================================
  // UI
  // ============================================================

  return (
    <main class="w-screen h-screen bg-gray-900 text-white font-sans flex flex-col overflow-hidden text-sm">

      {/*
        カスタムヘッダー (ドラッグ移動)
        - data-tauri-drag-region は Linux 環境によって動作しない場合があるため廃止
        - mousedown (左ボタン) で Rust の start_dragging コマンドを呼ぶ方式に変更
        - ボタン等のインタラクティブ要素は onMouseDown で stopPropagation することで
          ドラッグ領域から除外される
      */}
      <div
        onMouseDown={(e) => {
          // 左ボタン押下かつドラッグ対象要素 (button でない) の場合のみ
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
      <div class="shrink-0 flex flex-col items-center gap-1 py-4 bg-gray-800 border-b border-gray-700">
        <div class="text-4xl font-mono tabular-nums tracking-widest">
          {formatSeconds(elapsedSeconds())}
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
        <For each={operations()}>
          {(op) => (
            <div class="rounded-md overflow-hidden border border-gray-700">

              {/* Operation ヘッダー */}
              <div class="flex items-center justify-between px-3 py-2 bg-gray-700">
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
                <button
                  onClick={() => {
                    setAddTaskForOpId(
                      addTaskForOpId() === op.id ? null : op.id
                    );
                    setTaskName("");
                    setTaskTag("");
                  }}
                  class="ml-2 shrink-0 text-gray-500 hover:text-blue-400 text-xs transition-colors"
                >
                  ＋
                </button>
              </div>

              {/* Task リスト */}
              <For each={op.tasks}>
                {(task) => {
                  const isActive = () => activeInfo().task_id === task.id;
                  // このタスクのグローバルインデックスを都度計算
                  const globalIdx = () => flatTaskIds().indexOf(task.id);
                  const isFocused = () =>
                    focusedTaskIndex() >= 0 &&
                    focusedTaskIndex() === globalIdx();

                  return (
                    <button
                      data-task-id={task.id}
                      onClick={() => {
                        setFocusedTaskIndex(-1);
                        handleStartTask(task.id);
                      }}
                      class={`w-full flex items-center justify-between px-3 py-2 text-left border-t border-gray-700 transition-colors ${
                        isActive()
                          ? "bg-blue-950 text-blue-300"
                          : isFocused()
                            ? "bg-gray-700 text-white ring-2 ring-inset ring-blue-400"
                            : "bg-gray-800 hover:bg-gray-700 text-gray-300"
                      }`}
                    >
                      <span class="truncate">{task.name}</span>
                      <div class="flex items-center gap-1.5 shrink-0 ml-2">
                        <Show when={task.tag}>
                          <span class="text-xs px-1.5 py-0.5 rounded bg-gray-700 text-gray-400">
                            {task.tag}
                          </span>
                        </Show>
                        {/* Tab ナビゲーション中のフォーカスインジケーター */}
                        <Show when={isFocused()}>
                          <span class="text-blue-400 text-xs font-bold">
                            ↵
                          </span>
                        </Show>
                      </div>
                    </button>
                  );
                }}
              </For>

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
