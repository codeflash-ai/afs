import { describe, expect, it } from "vitest";

import {
  devSidecarPreparationCommands,
  npmDevServerCommand,
} from "./dev-script-helpers.mjs";

describe("desktop dev script helpers", () => {
  it("starts npm dev through cmd.exe on Windows", () => {
    expect(
      npmDevServerCommand({
        platform: "win32",
        env: { ComSpec: "C:\\Windows\\System32\\cmd.exe" },
      }),
    ).toEqual({
      program: "C:\\Windows\\System32\\cmd.exe",
      args: ["/d", "/s", "/c", "npm", "run", "dev"],
    });
  });

  it("starts npm dev directly outside Windows", () => {
    expect(npmDevServerCommand({ platform: "linux", env: {} })).toEqual({
      program: "npm",
      args: ["run", "dev"],
    });
  });

  it("stops the daemon before rebuilding Windows sidecars", () => {
    const commands = devSidecarPreparationCommands({
      cargo: "cargo",
      platform: "win32",
      processExecPath: "node",
      scriptDir: "C:\\repo\\apps\\desktop\\scripts",
      workspaceRoot: "C:\\repo",
    });

    expect(commands.map((command) => command.name)).toEqual([
      "stop-daemon",
      "build-sidecars",
    ]);
    expect(commands[0]).toMatchObject({
      program: "node",
      args: [
        "C:\\repo\\apps\\desktop\\scripts\\stop-daemon-for-build.mjs",
        "--loc",
        "C:\\repo\\target\\debug\\loc.exe",
      ],
      cwd: "C:\\repo",
    });
  });
});
