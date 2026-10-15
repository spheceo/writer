import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inTauri } from "./api";
import "./styles.css";

// In the Mac window our header sits under the window buttons, so it leaves
// room for them.
if (inTauri() && navigator.userAgent.includes("Mac")) document.documentElement.classList.add("mac-window");

const root = document.getElementById("root");
if (!root) throw new Error("missing root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
