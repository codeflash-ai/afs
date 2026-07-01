#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { devSidecarPreparationCommands } from "./dev-script-helpers.mjs";

if (process.env.LOCALITY_DESKTOP_SKIP_DEV_SIDECARS === "1") {
  console.log("prepare-dev-sidecars: skipped by LOCALITY_DESKTOP_SKIP_DEV_SIDECARS=1");
  process.exit(0);
}

const scriptDir = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(scriptDir, "../../..");
const cargo = process.env.CARGO || "cargo";

for (const command of devSidecarPreparationCommands({
  cargo,
  platform: process.platform,
  processExecPath: process.execPath,
  scriptDir,
  workspaceRoot,
})) {
  const result = spawnSync(command.program, command.args, {
    cwd: command.cwd,
    env: process.env,
    stdio: "inherit",
  });

  if (result.error) {
    console.error(
      `prepare-dev-sidecars: ${command.name} failed: ${result.error.message}`,
    );
    process.exit(1);
  }

  if ((result.status ?? 1) !== 0) {
    process.exit(result.status ?? 1);
  }
}

process.exit(0);
