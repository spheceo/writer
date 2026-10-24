import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inTauri } from "./api";
import { startScrollbars } from "./scrollbars";
import "./styles.css";

// In the Mac window our header sits under the window buttons, so it leaves
// room for them.
if (inTauri() && navigator.userAgent.includes("Mac")) document.documentElement.classList.add("mac-window");

// A click never leaves a focus ring (the kit's rule): Chrome shows
// :focus-visible on a clicked control once any key is pressed. A pointer
// marks the page until Tab, which brings the rings back.
const html = document.documentElement;
document.addEventListener("pointerdown", () => html.setAttribute("data-pointer", ""), { capture: true });
document.addEventListener("keydown", (event) => event.key === "Tab" && html.removeAttribute("data-pointer"), { capture: true });

startScrollbars();

const root = document.getElementById("root");
if (!root) throw new Error("missing root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
