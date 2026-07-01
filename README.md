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
- 出勤簿から始業・終業・休憩・業務時間を取得
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

### 勤怠連携

履歴画面の「日次集計」→「接続設定」から、ログインURL、出勤簿URL、工数URL、
企業ID、従業員番号、パスワードを入力します。URLとIDはアプリ設定へ、
パスワードはWindows資格情報マネージャーへ保存されます。

Windows証明書ストアは自動的に利用されます。ストアに未登録の社内CAが必要な
場合は、接続設定の「追加証明書」に `.cer` ファイルの絶対パスを指定できます。

履歴画面の「日次集計」で対象日を選び、「勤怠を取得」を押すと、出勤簿の
「集計(出)」「集計(退)」「休憩時間」「実働時間」を取得します。取得結果は
ローカルへ保存され、サマリーCSVにも含まれます。

従来の `login.txt` がある場合は初回の接続設定に値を引き継げます。
`login.txt` はGitの管理対象外です。

### 工数入力

履歴画面の「工数入力」で対象日の計測ログを集計し、勤怠の実働時間を各
オペレーションの計測時間比率で按分します。分単位の端数は、合計が実働時間と
必ず一致するよう調整されます。コメントには、各オペレーション内で作業実績の
あったタスク名を重複なし・1行1件で設定します。送信前に登録内容の確認が必要です。

オペレーション名が `系26-XXX` 形式の場合、勤怠側では次のように対応付けます。

```text
オペレーション 系26-019
  → 親プロジェクト 系26-0
  → タスク         系26-019
```

`XXX` の先頭桁を `{n}` として親プロジェクト `系26-{n}` を導出します。
`系24`、`管理26` など自動導出できない名称は、管理画面のオペレーション編集で
勤怠側の「工数プロジェクト」と「工数タスク」を明示的に設定できます。
対応が未設定のオペレーションは送信対象外としてプレビューに表示されます。
未終了ログがある場合、実働時間が未取得の場合、または工数合計が実働時間を
超える場合は送信できません。

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
├─ attendance\
│  └─ YYYY-MM-DD.json
└─ exports\
   └─ export_YYYY-MM-DD_HH-MM-SS.csv
```

- `master.json`: オペレーションとタスクの定義
- `logs`: 日別の計測セッション
- `attendance`: 出勤簿から取得した日別勤務実績（認証情報は含みません）
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
