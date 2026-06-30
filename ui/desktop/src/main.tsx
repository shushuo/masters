import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { initTheme } from "./lib/theme";
import "./index.css";

// Apply the saved theme preference before first paint to avoid a flash of the wrong palette.
initTheme();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
