# Tempomezurado プロジェクト開発方針

## 1. プロジェクト概要
業務ごとに細分化されたタスクの作業時間を計測・集計し、工数入力支援・業務効率改善に役立てるウィジェット型タイマーアプリ。
エスペラント語で「時間計測」を意味する「Tempomezurado」をプロジェクト名とする。

## 2. 技術スタック
- **コア/バックエンド:** Rust, Tauri (v2推奨)
- **フロントエンド:** SolidJS, TypeScript, Vite
- **スタイリング:** Tailwind CSS (ウィジェットの軽量なUI構築のため)
- **主要ライブラリ (Rust):**
  - `serde`, `serde_json`: データのシリアライズ/デシリアライズ、永続化
  - `chrono`: 時間管理、集計
  - `csv`: 集計データのCSV出力
  - `tauri-plugin-global-shortcut`: タスク切り替えのショートカット処理

## 3. アーキテクチャと責務の分離


- **バックエンド (Rust) の責務:**
  - `Operation`, `Task`, `TimeLog` のデータ構造の定義とメモリ上(`std::sync::Mutex`等)での状態管理。
  - ローカルファイル (JSON) へのデータの永続化と読み込み。
  - 現在時刻に基づく正確な時間計測、ログの記録、CSV集計処理。
  - グローバルショートカットの検知と処理。
  - **※絶対ルール:** 正確な「時間」の信頼できる情報源(Single Source of Truth)は常にRust側とする。

- **フロントエンド (SolidJS) の責務:**
  - Tauri IPC (`@tauri-apps/plugin-core` の `invoke` や `listen`) を介したバックエンドとの通信。
  - `createSignal` を用いた、極力無駄のないDOM更新（タイマー表示など）。
  - ウィンドウのドラッグ領域 (`data-tauri-drag-region`) の提供。
  - **※絶対ルール:** フロントエンドのタイマーはあくまで「見た目」であり、再起動や再読み込み時は常にRustから最新の状態を取得して同期する。

## 4. データ構造 (Rust側のモデル)
```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Operation {
    pub name: String,
    pub description: String,
    pub tasks: Vec<Task>, 
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub time_logs: Vec<TimeLog>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeLog {
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>, // 進行中の場合はNone
}
