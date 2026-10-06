import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { BridgeClient } from "./bridge/client";
import { launch } from "./launch";
import { runAclProbe } from "./qualification/aclProbe";
import { installQualificationCommands } from "./qualification/commands";
import { runIpcSelfTest } from "./qualification/ipcSelfTest";
import { App } from "./ui/App";
import "./styles.css";

const root = document.getElementById("root");

if (launch.probe === "acl") {
  // The unauthorized probe view never connects; it only proves refusal.
  if (root) root.textContent = "ACL probe";
  void runAclProbe();
} else if (root) {
  const client = new BridgeClient();
  client.onHydrated((hydrated) => void hydrated.refreshDiagnostics());
  if (launch.qualifyIpc) {
    let ran = false;
    client.onHydrated((hydrated) => {
      if (ran) return;
      ran = true;
      void runIpcSelfTest(hydrated);
    });
  }
  if (launch.qualificationBuild) installQualificationCommands(client);
  client.start();
  createRoot(root).render(
    <StrictMode>
      <App client={client} />
    </StrictMode>,
  );
}
