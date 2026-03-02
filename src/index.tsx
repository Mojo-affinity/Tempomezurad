/* @refresh reload */
import { render } from "solid-js/web";
import "./index.css";
import App from "./App";
import Launcher from "./Launcher";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// ウィンドウラベルで描画するコンポーネントを切り替える
// "launcher" ウィンドウ → Launcher、それ以外 ("main" など) → App
const label = getCurrentWebviewWindow().label;
const Root = label === "launcher" ? Launcher : App;

render(() => <Root />, document.getElementById("root") as HTMLElement);
