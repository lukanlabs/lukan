import React from "react";
import ReactDOM from "react-dom/client";
import { initTransport } from "./lib/transport";
import App from "./App";
import "./styles/index.css";

initTransport().then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
