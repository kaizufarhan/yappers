import React from "react";
import ReactDOM from "react-dom/client";
import { Overlay } from "./Overlay";
import "../styles.css";
// The bubble keeps its own look (charcoal, coral, Figtree) apart from the
// main window's brutalist theme.
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Overlay />
  </React.StrictMode>,
);
