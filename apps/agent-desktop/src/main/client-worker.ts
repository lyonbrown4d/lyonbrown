import { DesktopBackend } from "@lyonbrown/agent-desktop/internal/main/backend";
import { productionLauncher } from "@lyonbrown/agent-desktop/internal/main/production";
import { ClientWorkerService } from "@lyonbrown/agent-desktop/internal/main/client-worker-service";

const service = new ClientWorkerService(
  (publish) => new DesktopBackend(productionLauncher(), publish),
  {
    connected: () => process.connected,
    send: (value, done) => {
      process.send?.(value, undefined, undefined, done);
    },
    disconnect: () => {
      process.disconnect();
    },
  },
);
process.on("disconnect", () => {
  void service.close();
});
process.on("message", (value: unknown) => {
  void service.receive(value).catch(() => service.close());
});
