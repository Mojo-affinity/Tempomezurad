# Tempomezurado

業務・タスク単位の作業時間を記録し、日々の工数入力と振り返りを支援する
Windows向けデスクトップタイマーです。プロジェクト名はエスペラント語の
「時間計測」に由来します。

## 主な機能

- タスクの開始、停止、切り替え
- 当日合計とタスク別時間の表示
- `Ctrl+Shift+Space` で開くクイックランチャー
- タスク、オペレーション、タグの検索
- 7日・30日・全期間の履歴と集計
- 未終了ログの検出と解消
- オペレーションとタスクの編集、並べ替え、アーカイブ
- CSVエクスポート
- ウィンドウの最前面固定
- 多重起動防止

## 技術構成

- Tauri 2 / Rust
- SolidJS / TypeScript
- Vite
- Tailwind CSS 4

時間計測とローカルデータはRust側を信頼できる唯一の情報源として扱います。
フロントエンドの1秒更新は表示専用で、起動・操作時にはRust側の状態へ同期します。

## 開発環境

Windowsでは次の環境が必要です。

- Node.js 20以降
- Rust stable（MSVC toolchain）
- Visual Studio 2022 Build Tools
  - Desktop development with C++
  - Windows SDK
- Microsoft Edge WebView2 Runtime

```powershell
npm install
```

### 開発実行

```powershell
npm run tauri dev
```

Viteは `127.0.0.1:1420` を使用します。

### ビルド

```powershell
npm run build
npm run tauri -- build
```

### Rustテスト

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

## ローカルデータ

Windowsでは通常、次のディレクトリへ保存します。

```text
%APPDATA%\com.ubuntu.tempomezurado\
├─ master.json
├─ logs\
│  └─ YYYY-MM-DD.json
└─ exports\
   └─ export_YYYY-MM-DD_HH-MM-SS.csv
```

- `master.json`: オペレーションとタスクの定義
- `logs`: 日別の計測セッション
- `exports`: CSVエクスポート

計測中に異常終了した場合、同日中の最新セッションは次回起動時に復元されます。
過去日の未終了ログは履歴画面から確認・解消できます。

## ディレクトリ構成

```text
src/
├─ App.tsx            メイン画面と画面ナビゲーション
├─ Launcher.tsx       クイックランチャー
├─ HistoryPanel.tsx   履歴・集計
├─ ManagePanel.tsx    タスク管理
├─ Icons.tsx          SVGアイコン
└─ index.css          デザイントークンとUIスタイル

src-tauri/
└─ src/lib.rs         状態管理、永続化、計測、Tauriコマンド
```

## ライセンス

MIT
