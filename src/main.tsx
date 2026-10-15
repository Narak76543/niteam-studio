import React from "react";
import ReactDOM from "react-dom/client";

import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/base.css";
import App from "./App";

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
