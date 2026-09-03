import { createRoot } from "react-dom/client";

import App from "@/app/App";
import "./styles.css";

const container = document.getElementById("root");
if (!container) throw new Error("root element missing from index.html");

// Deliberately no StrictMode: its double-invoked effects would open and
// subscribe to each pty session twice in development.
createRoot(container).render(<App />);
