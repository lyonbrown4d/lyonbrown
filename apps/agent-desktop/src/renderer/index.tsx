import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DesktopApp } from "@lyonbrown/agent-desktop/internal/renderer/app";
const container = document.getElementById("root");
if (!container) throw new Error("MISSING_ROOT");
const client = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});
createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={client}>
      <DesktopApp bridge={window.lyonbrown} />
    </QueryClientProvider>
  </StrictMode>,
);
