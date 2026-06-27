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

interface RecentTaskInfo {
  task_id: string;
  task_name: string;
  operation_name: string;
  tag: string;
}

function Launcher() {
  const [tasks, setTasks] = createSignal<RecentTaskInfo[]>([]);
  const [query, setQuery] = createSignal("");
  const [focusedIndex, setFocusedIndex] = createSignal(0);
  const [loading, setLoading] = createSignal(false);
  let searchInput: HTMLInputElement | undefined;
  let unlistenShow: (() => void) | undefined;

  const filteredTasks = createMemo(() => {
    const normalized = query().trim().toLocaleLowerCase();
    if (!normalized) return tasks();
    return tasks().filter((task) =>
      [task.task_name, task.operation_name, task.tag].some((value) =>
        value.toLocaleLowerCase().includes(normalized),
      ),
    );
  });

  const closeSelf = () => {
    invoke("close_launcher").catch(console.error);
  };

  const selectTask = async (taskId: string) => {
    try {
      await invoke("start_task", { task_id: taskId });
    } catch (error) {
      console.error("タスクを開始できませんでした", error);
    }
    closeSelf();
  };

  const scrollFocused = (index: number) => {
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-launcher-index="${index}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  };

  const moveFocus = (direction: 1 | -1) => {
    const count = filteredTasks().length;
    if (!count) return;
    const next = (focusedIndex() + direction + count) % count;
    setFocusedIndex(next);
    scrollFocused(next);
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    const currentTasks = filteredTasks();
    switch (event.key) {
      case "Tab":
      case "ArrowDown":
        event.preventDefault();
        moveFocus(event.shiftKey ? -1 : 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        moveFocus(-1);
        break;
      case "Enter": {
        if (event.isComposing) return;
        event.preventDefault();
        const task = currentTasks[focusedIndex()];
        if (task) void selectTask(task.task_id);
        break;
      }
      case "Escape":
        event.preventDefault();
        closeSelf();
        break;
    }
  };

  const refreshTasks = async () => {
    setFocusedIndex(0);
    setQuery("");
    setLoading(true);
    try {
      setTasks(await invoke<RecentTaskInfo[]>("get_recent_tasks"));
    } finally {
      setLoading(false);
      requestAnimationFrame(() => searchInput?.focus());
    }
  };

  onMount(async () => {
    document.addEventListener("keydown", handleKeyDown);
    unlistenShow = await listen("show-launcher", refreshTasks);
  });

  onCleanup(() => {
    document.removeEventListener("keydown", handleKeyDown);
    unlistenShow?.();
  });

  return (
    <div class="launcher-overlay" onClick={closeSelf}>
      <section
        class="launcher-panel"
        role="dialog"
        aria-label="タスクランチャー"
        onClick={(event) => event.stopPropagation()}
      >
        <header class="launcher-header">
          <div class="flex min-w-0 items-center gap-3">
            <div class="launcher-brand">
              <Icon name="spark" size={17} />
            </div>
            <div class="min-w-0">
              <p class="launcher-title">Quick switch</p>
              <p class="launcher-subtitle">タスクを検索して計測を開始</p>
            </div>
          </div>
          <button class="launcher-close" onClick={closeSelf} title="閉じる">
            <Icon name="x" size={16} />
          </button>
        </header>

        <div class="launcher-search-wrap">
          <label class="launcher-search">
            <Icon name="search" size={18} />
            <input
              ref={searchInput}
              value={query()}
              onInput={(event) => {
                setQuery(event.currentTarget.value);
                setFocusedIndex(0);
              }}
              placeholder="タスク、オペレーション、タグを検索"
              autocomplete="off"
              spellcheck={false}
            />
            <Show when={query()}>
              <button
                type="button"
                onClick={() => {
                  setQuery("");
                  setFocusedIndex(0);
                  searchInput?.focus();
                }}
              >
                <Icon name="x" size={14} />
              </button>
            </Show>
          </label>
        </div>

        <div class="launcher-meta">
          <span>{query() ? "検索結果" : "最近使用したタスク"}</span>
          <span>{filteredTasks().length} tasks</span>
        </div>

        <div class="launcher-list">
          <Show when={loading()}>
            <div class="launcher-empty">
              <span class="launcher-spinner" />
              <p>タスクを読み込んでいます</p>
            </div>
          </Show>

          <Show when={!loading() && filteredTasks().length === 0}>
            <div class="launcher-empty">
              <Icon name={query() ? "search" : "timer"} size={24} />
              <p>
                {query()
                  ? "一致するタスクがありません"
                  : "タスクがまだありません"}
              </p>
              <Show when={query()}>
                <button
                  onClick={() => {
                    setQuery("");
                    searchInput?.focus();
                  }}
                >
                  検索をクリア
                </button>
              </Show>
            </div>
          </Show>

          <For each={filteredTasks()}>
            {(task, index) => {
              const focused = () => focusedIndex() === index();
              return (
                <button
                  data-launcher-index={index()}
                  class={`launcher-task ${focused() ? "is-focused" : ""}`}
                  onClick={() => selectTask(task.task_id)}
                  onMouseEnter={() => setFocusedIndex(index())}
                >
                  <span class="launcher-task-icon">
                    <Icon name="timer" size={17} />
                  </span>
                  <span class="min-w-0 flex-1 text-left">
                    <span class="flex items-center gap-2">
                      <span class="truncate text-sm font-semibold text-slate-100">
                        {task.task_name}
                      </span>
                      <Show when={task.tag}>
                        <span class="launcher-tag">{task.tag}</span>
                      </Show>
                    </span>
                    <span class="mt-1 block truncate text-[11px] text-slate-500">
                      {task.operation_name}
                    </span>
                  </span>
                  <span class="launcher-enter">
                    <Show when={focused()}>
                      <span>開始</span>
                      <kbd>↵</kbd>
                    </Show>
                  </span>
                </button>
              );
            }}
          </For>
        </div>

        <footer class="launcher-footer">
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd>
            <span>選択</span>
          </span>
          <span>
            <kbd>Enter</kbd>
            <span>開始</span>
          </span>
          <span>
            <kbd>Esc</kbd>
            <span>閉じる</span>
          </span>
        </footer>
      </section>
    </div>
  );
}

export default Launcher;
