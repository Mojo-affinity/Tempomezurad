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

interface AppStateView {
  operations: Operation[];
}

interface ManagePanelProps {
  onNotify: (message: string) => void;
}

export default function ManagePanel(props: ManagePanelProps) {
  const [operations, setOperations] = createSignal<Operation[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [showArchived, setShowArchived] = createSignal(false);

  const [editingOperationId, setEditingOperationId] = createSignal<
    string | null
  >(null);
  const [editingTaskId, setEditingTaskId] = createSignal<string | null>(null);
  const [editName, setEditName] = createSignal("");
  const [editSecondary, setEditSecondary] = createSignal("");

  const [showAddOperation, setShowAddOperation] = createSignal(false);
  const [addTaskTo, setAddTaskTo] = createSignal<string | null>(null);
  const [newName, setNewName] = createSignal("");
  const [newSecondary, setNewSecondary] = createSignal("");
  let unlistenState: (() => void) | undefined;

  const loadState = async () => {
    setLoading(true);
    try {
      const state = await invoke<AppStateView>("get_state");
      setOperations(state.operations);
    } catch (error) {
      console.error("管理データを読み込めませんでした", error);
      props.onNotify("管理データを読み込めませんでした");
    } finally {
      setLoading(false);
    }
  };

  const visibleOperations = createMemo(() =>
    operations().filter((operation) => showArchived() || !operation.hidden),
  );

  const totalTasks = createMemo(() =>
    operations().reduce(
      (total, operation) => total + operation.tasks.length,
      0,
    ),
  );

  const archivedCount = createMemo(
    () =>
      operations().filter((operation) => operation.hidden).length +
      operations().reduce(
        (total, operation) =>
          total + operation.tasks.filter((task) => task.hidden).length,
        0,
      ),
  );

  const beginOperationEdit = (operation: Operation) => {
    setEditingTaskId(null);
    setEditingOperationId(operation.id);
    setEditName(operation.name);
    setEditSecondary(operation.description);
  };

  const beginTaskEdit = (task: Task) => {
    setEditingOperationId(null);
    setEditingTaskId(task.id);
    setEditName(task.name);
    setEditSecondary(task.tag);
  };

  const cancelEdit = () => {
    setEditingOperationId(null);
    setEditingTaskId(null);
  };

  const saveOperation = async (operationId: string) => {
    if (!editName().trim()) return;
    try {
      await invoke("update_operation", {
        opId: operationId,
        name: editName(),
        description: editSecondary(),
      });
      cancelEdit();
      await loadState();
      props.onNotify("オペレーションを更新しました");
    } catch (error) {
      console.error("オペレーションを更新できませんでした", error);
      props.onNotify("更新できませんでした");
    }
  };

  const saveTask = async (taskId: string) => {
    if (!editName().trim()) return;
    try {
      await invoke("update_task", {
        taskId,
        name: editName(),
        tag: editSecondary(),
      });
      cancelEdit();
      await loadState();
      props.onNotify("タスクを更新しました");
    } catch (error) {
      console.error("タスクを更新できませんでした", error);
      props.onNotify("更新できませんでした");
    }
  };

  const addOperation = async () => {
    if (!newName().trim()) return;
    try {
      await invoke("add_operation", {
        name: newName().trim(),
        description: newSecondary().trim(),
      });
      setShowAddOperation(false);
      setNewName("");
      setNewSecondary("");
      await loadState();
      props.onNotify("オペレーションを追加しました");
    } catch (error) {
      console.error("オペレーションを追加できませんでした", error);
      props.onNotify("追加できませんでした");
    }
  };

  const addTask = async (operationId: string) => {
    if (!newName().trim()) return;
    try {
      await invoke("add_task", {
        operationId,
        name: newName().trim(),
        tag: newSecondary().trim(),
      });
      setAddTaskTo(null);
      setNewName("");
      setNewSecondary("");
      await loadState();
      props.onNotify("タスクを追加しました");
    } catch (error) {
      console.error("タスクを追加できませんでした", error);
      props.onNotify("追加できませんでした");
    }
  };

  const toggleOperation = async (operationId: string) => {
    await invoke("toggle_operation_visibility", { opId: operationId });
    await loadState();
    props.onNotify("表示状態を更新しました");
  };

  const toggleTask = async (taskId: string) => {
    await invoke("toggle_task_visibility", { taskId });
    await loadState();
    props.onNotify("表示状態を更新しました");
  };

  const reorderOperation = async (
    operationId: string,
    direction: "up" | "down",
  ) => {
    await invoke("reorder_operation", { opId: operationId, direction });
    await loadState();
  };

  const reorderTask = async (
    operationId: string,
    taskId: string,
    direction: "up" | "down",
  ) => {
    await invoke("reorder_task", {
      opId: operationId,
      taskId,
      direction,
    });
    await loadState();
  };

  const openAddTask = (operationId: string) => {
    setShowAddOperation(false);
    setAddTaskTo(operationId);
    setNewName("");
    setNewSecondary("");
  };

  onMount(async () => {
    await loadState();
    unlistenState = await listen("state-changed", loadState);
  });

  onCleanup(() => unlistenState?.());

  const controlButton =
    "grid h-7 w-7 place-items-center rounded-lg text-slate-600 transition hover:bg-white/6 hover:text-slate-300 disabled:cursor-not-allowed disabled:opacity-25";

  return (
    <div class="manage-view">
      <section class="manage-intro">
        <div>
          <p class="eyebrow">Workspace</p>
          <h1>タスク管理</h1>
          <p>業務構造、名称、タグ、表示順を整理します。</p>
        </div>
        <button
          class={`manage-archive-toggle ${showArchived() ? "is-active" : ""}`}
          onClick={() => setShowArchived((value) => !value)}
        >
          <Icon name={showArchived() ? "eye" : "eye-off"} size={14} />
          アーカイブ
        </button>
      </section>

      <section class="manage-summary">
        <div>
          <span>Operations</span>
          <strong>{operations().length}</strong>
        </div>
        <div>
          <span>Tasks</span>
          <strong>{totalTasks()}</strong>
        </div>
        <div>
          <span>Archived</span>
          <strong>{archivedCount()}</strong>
        </div>
      </section>

      <Show when={loading()}>
        <div class="history-loading">
          <span class="launcher-spinner" />
          ワークスペースを読み込んでいます
        </div>
      </Show>

      <div class="manage-list">
        <For each={visibleOperations()}>
          {(operation, operationIndex) => (
            <section
              class={`manage-operation ${operation.hidden ? "is-archived" : ""}`}
            >
              <Show
                when={editingOperationId() === operation.id}
                fallback={
                  <header class="manage-operation-header">
                    <div class="min-w-0 flex-1">
                      <div class="flex items-center gap-2">
                        <h2 class="truncate">{operation.name}</h2>
                        <Show when={operation.hidden}>
                          <span class="manage-archived-label">Archived</span>
                        </Show>
                      </div>
                      <p class="truncate">
                        {operation.description || "説明はありません"}
                      </p>
                    </div>
                    <div class="flex items-center">
                      <button
                        class={controlButton}
                        onClick={() => beginOperationEdit(operation)}
                        title="編集"
                      >
                        <Icon name="edit" size={13} />
                      </button>
                      <button
                        class={controlButton}
                        disabled={operationIndex() === 0}
                        onClick={() =>
                          reorderOperation(operation.id, "up")
                        }
                        title="上へ"
                      >
                        <Icon name="arrow-up" size={13} />
                      </button>
                      <button
                        class={controlButton}
                        disabled={operationIndex() === operations().length - 1}
                        onClick={() =>
                          reorderOperation(operation.id, "down")
                        }
                        title="下へ"
                      >
                        <Icon name="arrow-down" size={13} />
                      </button>
                      <button
                        class={controlButton}
                        onClick={() => toggleOperation(operation.id)}
                        title={operation.hidden ? "再表示" : "アーカイブ"}
                      >
                        <Icon
                          name={operation.hidden ? "eye-off" : "eye"}
                          size={13}
                        />
                      </button>
                    </div>
                  </header>
                }
              >
                <div class="manage-edit-form">
                  <label>
                    <span>オペレーション名</span>
                    <input
                      value={editName()}
                      onInput={(event) => setEditName(event.currentTarget.value)}
                      onKeyDown={(event) =>
                        event.key === "Enter" && saveOperation(operation.id)
                      }
                      autofocus
                    />
                  </label>
                  <label>
                    <span>説明</span>
                    <input
                      value={editSecondary()}
                      onInput={(event) =>
                        setEditSecondary(event.currentTarget.value)
                      }
                      onKeyDown={(event) =>
                        event.key === "Enter" && saveOperation(operation.id)
                      }
                    />
                  </label>
                  <div class="flex gap-1.5">
                    <button class="secondary-button" onClick={cancelEdit}>
                      キャンセル
                    </button>
                    <button
                      class="primary-button"
                      onClick={() => saveOperation(operation.id)}
                    >
                      保存
                    </button>
                  </div>
                </div>
              </Show>

              <div class="manage-tasks">
                <For
                  each={operation.tasks.filter(
                    (task) => showArchived() || !task.hidden,
                  )}
                >
                  {(task, taskIndex) => (
                    <Show
                      when={editingTaskId() === task.id}
                      fallback={
                        <div
                          class={`manage-task ${task.hidden ? "is-archived" : ""}`}
                        >
                          <span class="manage-task-dot" />
                          <div class="min-w-0 flex-1">
                            <div class="flex items-center gap-1.5">
                              <p class="truncate">{task.name}</p>
                              <Show when={task.hidden}>
                                <span class="manage-archived-label">
                                  Archived
                                </span>
                              </Show>
                            </div>
                            <Show
                              when={task.tag}
                              fallback={
                                <span class="text-[8px] text-slate-700">
                                  タグなし
                                </span>
                              }
                            >
                              <span class="tag">{task.tag}</span>
                            </Show>
                          </div>
                          <div class="flex items-center">
                            <button
                              class={controlButton}
                              onClick={() => beginTaskEdit(task)}
                              title="編集"
                            >
                              <Icon name="edit" size={12} />
                            </button>
                            <button
                              class={controlButton}
                              disabled={taskIndex() === 0}
                              onClick={() =>
                                reorderTask(operation.id, task.id, "up")
                              }
                              title="上へ"
                            >
                              <Icon name="arrow-up" size={12} />
                            </button>
                            <button
                              class={controlButton}
                              disabled={
                                taskIndex() === operation.tasks.length - 1
                              }
                              onClick={() =>
                                reorderTask(operation.id, task.id, "down")
                              }
                              title="下へ"
                            >
                              <Icon name="arrow-down" size={12} />
                            </button>
                            <button
                              class={controlButton}
                              onClick={() => toggleTask(task.id)}
                              title={task.hidden ? "再表示" : "アーカイブ"}
                            >
                              <Icon
                                name={task.hidden ? "eye-off" : "eye"}
                                size={12}
                              />
                            </button>
                          </div>
                        </div>
                      }
                    >
                      <div class="manage-edit-form is-task">
                        <label>
                          <span>タスク名</span>
                          <input
                            value={editName()}
                            onInput={(event) =>
                              setEditName(event.currentTarget.value)
                            }
                            onKeyDown={(event) =>
                              event.key === "Enter" && saveTask(task.id)
                            }
                            autofocus
                          />
                        </label>
                        <label>
                          <span>タグ</span>
                          <input
                            value={editSecondary()}
                            onInput={(event) =>
                              setEditSecondary(event.currentTarget.value)
                            }
                            onKeyDown={(event) =>
                              event.key === "Enter" && saveTask(task.id)
                            }
                          />
                        </label>
                        <div class="flex gap-1.5">
                          <button class="secondary-button" onClick={cancelEdit}>
                            キャンセル
                          </button>
                          <button
                            class="primary-button"
                            onClick={() => saveTask(task.id)}
                          >
                            保存
                          </button>
                        </div>
                      </div>
                    </Show>
                  )}
                </For>

                <Show when={addTaskTo() === operation.id}>
                  <div class="manage-edit-form is-task is-new">
                    <label>
                      <span>新しいタスク名</span>
                      <input
                        value={newName()}
                        onInput={(event) => setNewName(event.currentTarget.value)}
                        onKeyDown={(event) =>
                          event.key === "Enter" && addTask(operation.id)
                        }
                        autofocus
                      />
                    </label>
                    <label>
                      <span>タグ</span>
                      <input
                        value={newSecondary()}
                        onInput={(event) =>
                          setNewSecondary(event.currentTarget.value)
                        }
                        onKeyDown={(event) =>
                          event.key === "Enter" && addTask(operation.id)
                        }
                      />
                    </label>
                    <div class="flex gap-1.5">
                      <button
                        class="secondary-button"
                        onClick={() => setAddTaskTo(null)}
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

                <Show when={!operation.hidden && addTaskTo() !== operation.id}>
                  <button
                    class="manage-add-task"
                    onClick={() => openAddTask(operation.id)}
                  >
                    <Icon name="add" size={12} />
                    タスクを追加
                  </button>
                </Show>
              </div>
            </section>
          )}
        </For>
      </div>

      <div class="mt-3">
        <Show
          when={showAddOperation()}
          fallback={
            <button
              class="add-operation-button"
              onClick={() => {
                setAddTaskTo(null);
                setShowAddOperation(true);
                setNewName("");
                setNewSecondary("");
              }}
            >
              <Icon name="add" size={14} />
              オペレーションを追加
            </button>
          }
        >
          <div class="manage-edit-form is-new-operation">
            <label>
              <span>新しいオペレーション名</span>
              <input
                value={newName()}
                onInput={(event) => setNewName(event.currentTarget.value)}
                onKeyDown={(event) =>
                  event.key === "Enter" && addOperation()
                }
                autofocus
              />
            </label>
            <label>
              <span>説明</span>
              <input
                value={newSecondary()}
                onInput={(event) => setNewSecondary(event.currentTarget.value)}
                onKeyDown={(event) =>
                  event.key === "Enter" && addOperation()
                }
              />
            </label>
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
    </div>
  );
}
