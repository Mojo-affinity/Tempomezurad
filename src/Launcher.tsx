import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// ============================================================
// 型定義
// ============================================================

interface RecentTaskInfo {
  task_id: string;
  task_name: string;
  operation_name: string;
  tag: string;
}

// ============================================================
// ランチャーコンポーネント
// ============================================================

function Launcher() {
  const win = getCurrentWebviewWindow();

  const [tasks, setTasks] = createSignal<RecentTaskInfo[]>([]);
  const [focusedIndex, setFocusedIndex] = createSignal(0);
  const [loading, setLoading] = createSignal(false);

  let unlistenShow: (() => void) | undefined;

  // ============================================================
  // ウィンドウ操作
  // ============================================================

  const closeSelf = () => {
    // hide() で常駐維持（close() しない）
    invoke("close_launcher").catch(console.error);
  };

  const selectTask = async (taskId: string) => {
    try {
      await invoke("start_task", { taskId });
    } catch (e) {
      console.error("start_task エラー:", e);
    }
    closeSelf();
  };

  // ============================================================
  // キーボードナビゲーション
  // ============================================================

  const handleKeyDown = (e: KeyboardEvent) => {
    const ts = tasks();
    if (ts.length === 0) return;

    switch (e.key) {
      case "Tab":
        e.preventDefault();
        setFocusedIndex((i) =>
          e.shiftKey ? (i - 1 + ts.length) % ts.length : (i + 1) % ts.length
        );
        scrollFocused();
        break;
      case "ArrowDown":
        e.preventDefault();
        setFocusedIndex((i) => (i + 1) % ts.length);
        scrollFocused();
        break;
      case "ArrowUp":
        e.preventDefault();
        setFocusedIndex((i) => (i - 1 + ts.length) % ts.length);
        scrollFocused();
        break;
      case "Enter": {
        e.preventDefault();
        const t = ts[focusedIndex()];
        if (t) selectTask(t.task_id);
        break;
      }
      case "Escape":
        e.preventDefault();
        closeSelf();
        break;
    }
  };

  const scrollFocused = () => {
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-launcher-index="${focusedIndex()}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  };

  // ============================================================
  // 初期化:
  //   "show-launcher" イベントを待ち受ける。
  //   イベント受信 → タスク取得 → DOM 反映 → win.show() の順で実行する。
  //   これにより、ウィンドウが表示される時点で必ずコンテンツが揃っており
  //   白フラッシュが発生しない。
  // ============================================================

  onMount(async () => {
    document.addEventListener("keydown", handleKeyDown);

    unlistenShow = await listen("show-launcher", async () => {
      // 1. フォーカスをリセット
      setFocusedIndex(0);

      // 2. タスクリストを最新化（ウィンドウはまだ非表示）
      setLoading(true);
      const fresh = await invoke<RecentTaskInfo[]>("get_recent_tasks");
      setTasks(fresh);
      setLoading(false);

      // 3. SolidJS の DOM 更新が完了したフレームでウィンドウを表示
      requestAnimationFrame(() => {
        win.show().catch(console.error);
      });
    });
  });

  onCleanup(() => {
    document.removeEventListener("keydown", handleKeyDown);
    unlistenShow?.();
  });

  // ============================================================
  // UI
  // ============================================================

  return (
    // 全画面半透明オーバーレイ。背景クリックで閉じる
    <div
      class="w-screen h-screen flex items-center justify-center"
      style="background-color: rgba(0,0,0,0.6); backdrop-filter: blur(4px);"
      onClick={closeSelf}
    >
      {/* モーダルパネル。クリック伝播を止める */}
      <div
        class="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl flex flex-col overflow-hidden"
        style="width: 420px; max-height: 60vh;"
        onClick={(e) => e.stopPropagation()}
      >
        {/* ヘッダー */}
        <div class="px-4 py-2.5 border-b border-gray-700 bg-gray-800 flex items-center gap-2 shrink-0">
          <span class="text-blue-400 text-xs font-mono font-bold tracking-wider">
            TASK LAUNCHER
          </span>
          <span class="ml-auto text-gray-500 text-xs">
            ↑↓ / Tab &nbsp;·&nbsp; Enter で開始 &nbsp;·&nbsp; Esc で閉じる
          </span>
        </div>

        {/* タスクリスト */}
        <div class="overflow-y-auto flex-1">
          <Show when={loading()}>
            <p class="text-gray-500 text-xs text-center py-8">読み込み中…</p>
          </Show>

          <Show when={!loading() && tasks().length === 0}>
            <p class="text-gray-500 text-xs text-center py-8">
              タスクがありません。メインウィンドウで追加してください。
            </p>
          </Show>

          <For each={tasks()}>
            {(task, index) => {
              const focused = () => focusedIndex() === index();
              return (
                <div
                  data-launcher-index={index()}
                  class={`px-4 py-3 border-b border-gray-800 cursor-pointer transition-colors select-none ${
                    focused()
                      ? "bg-blue-700 text-white"
                      : "text-gray-300 hover:bg-gray-800"
                  }`}
                  onClick={() => selectTask(task.task_id)}
                  onMouseEnter={() => setFocusedIndex(index())}
                >
                  <div class="flex items-center gap-2">
                    <span
                      class={`text-sm font-medium truncate ${
                        focused() ? "text-white" : "text-gray-200"
                      }`}
                    >
                      {task.task_name}
                    </span>
                    <Show when={task.tag}>
                      <span
                        class={`text-xs px-1.5 py-0.5 rounded shrink-0 ${
                          focused()
                            ? "bg-blue-600 text-blue-100"
                            : "bg-gray-700 text-gray-400"
                        }`}
                      >
                        {task.tag}
                      </span>
                    </Show>
                    <Show when={focused()}>
                      <span class="ml-auto text-blue-200 text-xs font-bold shrink-0">
                        ↵
                      </span>
                    </Show>
                  </div>
                  <div
                    class={`text-xs mt-0.5 ${
                      focused() ? "text-blue-200" : "text-gray-500"
                    }`}
                  >
                    {task.operation_name}
                  </div>
                </div>
              );
            }}
          </For>
        </div>
      </div>
    </div>
  );
}

export default Launcher;
