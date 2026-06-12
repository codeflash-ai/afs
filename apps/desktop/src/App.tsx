import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  AlertTriangle,
  Check,
  ChevronRight,
  Clipboard,
  Clock3,
  Copy,
  EyeOff,
  FolderOpen,
  History,
  Home,
  ListChecks,
  Loader2,
  Power,
  RotateCcw,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

type AppView = "home" | "pending" | "review" | "activity" | "settings";
type LocateState = "idle" | "preparing" | "ready" | "error";

type DesktopSnapshot = {
  health: {
    state: string;
    attentionCount: number;
  };
  connection: {
    connector: string;
    workspaceName: string;
    accountLabel: string;
    status: string;
  };
  mount: {
    connector: string;
    workspaceName: string;
    localPath: string;
    projection: string;
    readOnly: boolean;
    status: string;
  };
  pendingChanges: PendingChange[];
  activity: ActivityItem[];
  suggestions: ConnectorSuggestion[];
};

type PendingChange = {
  title: string;
  localPath: string;
  summary: string;
  state: "safe" | "needs_review" | "conflict" | "blocked";
};

type ActivityItem = {
  title: string;
  detail: string;
  when: string;
  kind: string;
  undoAvailable: boolean;
};

type ConnectorSuggestion = {
  connector: string;
  description: string;
  state: string;
};

type LocatedItem = {
  title: string;
  kind: string;
  localPath: string;
  state: "ready" | "preparing" | "no_access" | "not_found";
};

type PushPlan = {
  title: string;
  summary: string;
  pagesUpdated: number;
  databaseRowsUpdated: number;
  pagesDeleted: number;
  canPush: boolean;
  guardrailState: string;
  files: PendingChange[];
};

const sampleSnapshot: DesktopSnapshot = {
  health: {
    state: "ready",
    attentionCount: 3,
  },
  connection: {
    connector: "notion",
    workspaceName: "CodeFlash",
    accountLabel: "saurabh@codeflash.ai",
    status: "ready",
  },
  mount: {
    connector: "notion",
    workspaceName: "CodeFlash",
    localPath: "~/Documents/AFS/Notion",
    projection: "macOS File Provider",
    readOnly: false,
    status: "ready",
  },
  pendingChanges: [
    {
      title: "Roadmap 2026",
      localPath: "Engineering/Roadmap 2026 ~a3f2.md",
      summary: "2 text edits",
      state: "safe",
    },
    {
      title: "Launch Plan",
      localPath: "Marketing/Launch Plan ~8841.md",
      summary: "needs review: large deletion",
      state: "needs_review",
    },
    {
      title: "Customer Notes",
      localPath: "Sales/Customer Notes ~6b91.md",
      summary: "1 property edit",
      state: "safe",
    },
  ],
  activity: [
    {
      title: "Pushed Roadmap 2026 to Notion",
      detail: "2 block edits",
      when: "Today",
      kind: "push",
      undoAvailable: true,
    },
    {
      title: "Located Launch Plan",
      detail: "Prepared local path for an agent",
      when: "Today",
      kind: "locate",
      undoAvailable: false,
    },
    {
      title: "Connected Notion workspace CodeFlash",
      detail: "Credentials stored in the OS credential store",
      when: "Earlier",
      kind: "connect",
      undoAvailable: false,
    },
  ],
  suggestions: [
    {
      connector: "Linear",
      description: "Mount issues and projects as local files.",
      state: "planned",
    },
  ],
};

const samplePushPlan: PushPlan = {
  title: "Review Push",
  summary: "3 files will update Notion.",
  pagesUpdated: 2,
  databaseRowsUpdated: 1,
  pagesDeleted: 0,
  canPush: true,
  guardrailState: "safe",
  files: sampleSnapshot.pendingChanges,
};

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function callCommand<T>(command: string, args?: Record<string, unknown>, fallback?: T) {
  if (!isTauriRuntime()) {
    if (fallback === undefined) {
      throw new Error(`Tauri command unavailable: ${command}`);
    }
    return fallback;
  }

  return invoke<T>(command, args);
}

export default function App() {
  const [snapshot, setSnapshot] = useState<DesktopSnapshot>(sampleSnapshot);
  const [view, setView] = useState<AppView>("home");
  const [showOnboarding, setShowOnboarding] = useState(() => window.location.hash !== "#app");

  useEffect(() => {
    void callCommand<DesktopSnapshot>("desktop_snapshot", undefined, sampleSnapshot)
      .then(setSnapshot)
      .catch(() => setSnapshot(sampleSnapshot));
  }, []);

  if (showOnboarding) {
    return (
      <Onboarding
        snapshot={snapshot}
        onComplete={() => {
          setShowOnboarding(false);
          setView("home");
        }}
      />
    );
  }

  return <MainShell snapshot={snapshot} view={view} onViewChange={setView} />;
}

function Onboarding({
  snapshot,
  onComplete,
}: {
  snapshot: DesktopSnapshot;
  onComplete: () => void;
}) {
  const [step, setStep] = useState(1);
  const [oauthReady, setOauthReady] = useState(false);
  const [mountPath, setMountPath] = useState(snapshot.mount.localPath);
  const [locateUrl, setLocateUrl] = useState("");
  const [locatedItem, setLocatedItem] = useState<LocatedItem | null>(null);
  const [locateState, setLocateState] = useState<LocateState>("idle");

  async function startConnect() {
    await callCommand("connect_notion", undefined, { ok: true });
    setStep(2);
    window.setTimeout(() => setOauthReady(true), 1100);
  }

  async function startMount() {
    await callCommand("create_workspace_mount", { path: mountPath }, { ok: true });
    setStep(4);
  }

  async function locatePage() {
    if (!locateUrl.trim()) {
      return;
    }

    setLocateState("preparing");
    try {
      const item = await callCommand<LocatedItem>(
        "locate_notion_page",
        { url: locateUrl },
        {
          title: "Roadmap 2026",
          kind: "Page",
          localPath: "~/Documents/AFS/Notion/Engineering/Roadmap 2026 ~a3f2.md",
          state: "ready",
        },
      );
      setLocatedItem(item);
      setLocateState("ready");
    } catch {
      setLocateState("error");
      setLocatedItem(null);
    }
  }

  return (
    <main className="setup-shell">
      <section className="setup-window">
        <WindowChrome title="AFS Setup" meta={step < 5 ? `${step} of 4` : ""} />
        {step === 1 && (
          <SetupContent mark={<BrandTile>AFS</BrandTile>}>
            <div>
              <h1>Let your agents edit Notion as local files.</h1>
              <p>
                Mount your Notion workspace in Documents. Agents edit local
                files, then AFS syncs reviewed changes back to Notion.
              </p>
            </div>
            <PrimaryButton onClick={startConnect}>Connect Notion</PrimaryButton>
            <p className="quiet-note">Local edits stay pending until you review and push.</p>
          </SetupContent>
        )}

        {step === 2 && (
          <SetupContent mark={<BrandTile variant="notion">N</BrandTile>}>
            <div>
              <h1>Finish connecting in Notion</h1>
              <p>
                A browser window is open. Choose your workspace, pick the pages
                AFS can use, then approve access.
              </p>
            </div>
            <ProgressList
              items={[
                { label: "Browser opened", state: "done" },
                { label: "Select workspace and pages", state: oauthReady ? "done" : "active" },
                { label: "Approve access", state: oauthReady ? "done" : "idle" },
              ]}
            />
            <PrimaryButton disabled={!oauthReady} onClick={() => setStep(3)}>
              {oauthReady ? "Continue" : "Waiting for Notion"}
            </PrimaryButton>
            <TextButton onClick={() => void callCommand("connect_notion", undefined, { ok: true })}>
              Open browser again
            </TextButton>
            <p className="quiet-note">Credentials are stored securely in the OS credential store.</p>
          </SetupContent>
        )}

        {step === 3 && (
          <SetupContent mark={<BrandTile variant="folder" />}>
            <div>
              <h1>Where should your Notion files appear?</h1>
              <p>AFS keeps the folder visible in Documents and organized under its own directory.</p>
            </div>
            <div className="path-field">
              <input value={mountPath} onChange={(event) => setMountPath(event.target.value)} />
              <SecondaryButton compact>Choose</SecondaryButton>
            </div>
            <PrimaryButton disabled={!mountPath.trim()} onClick={startMount}>
              Continue
            </PrimaryButton>
            <p className="quiet-note">
              This folder will include AGENTS.md and CLAUDE.md to help your agents edit files
              natively.
            </p>
          </SetupContent>
        )}

        {step === 4 && (
          <SetupContent mark={<BrandTile variant="progress" />}>
            <div>
              <h1>Preparing your Notion workspace</h1>
              <p>You can continue as soon as the folder and agent instructions are ready.</p>
            </div>
            <ProgressList
              items={[
                { label: "Connected Notion", state: "done" },
                { label: "Created local folder", state: "done" },
                { label: "Found top-level workspace pages", state: "done" },
                { label: "Added agent instructions", state: "done" },
                { label: "Preparing workspace in the background", state: "active" },
              ]}
            />
            <div className="button-row">
              <PrimaryButton onClick={() => setStep(5)}>Continue</PrimaryButton>
              <TextButton onClick={() => void callCommand("open_path", { path: mountPath }, { ok: true })}>
                Open Notion Folder
              </TextButton>
            </div>
          </SetupContent>
        )}

        {step === 5 && (
          <SetupContent mark={<BrandTile variant="ready" />}>
            <div>
              <h1>Your Notion folder is ready</h1>
              <p className="path-line">{mountPath}</p>
            </div>
            <PrimaryButton onClick={onComplete}>Open Notion Folder</PrimaryButton>
            <LocateBox
              label="Open a Notion page"
              value={locateUrl}
              onChange={setLocateUrl}
              onSubmit={locatePage}
              state={locateState}
            />
            {locatedItem && <LocatedPath item={locatedItem} />}
            <AgentPrompt />
            <TextButton onClick={() => copyText(mountPath)}>Copy folder path</TextButton>
          </SetupContent>
        )}
      </section>
    </main>
  );
}

function MainShell({
  snapshot,
  view,
  onViewChange,
}: {
  snapshot: DesktopSnapshot;
  view: AppView;
  onViewChange: (view: AppView) => void;
}) {
  return (
    <main className="app-frame">
      <WindowChrome title="AFS" meta={snapshot.health.attentionCount > 0 ? "Pending Changes" : "Ready"} />
      <div className="app-shell">
        <aside className="sidebar">
          <div className="sidebar-brand">
            <ApertureIcon />
            <strong>AFS</strong>
          </div>
          <nav>
            <SidebarButton active={view === "home"} icon={<Home />} onClick={() => onViewChange("home")}>
              Home
            </SidebarButton>
            <SidebarButton
              active={view === "pending" || view === "review"}
              icon={<ListChecks />}
              onClick={() => onViewChange("pending")}
            >
              Pending
            </SidebarButton>
            <SidebarButton
              active={view === "activity"}
              icon={<History />}
              onClick={() => onViewChange("activity")}
            >
              Activity
            </SidebarButton>
            <SidebarButton
              active={view === "settings"}
              icon={<Settings />}
              onClick={() => onViewChange("settings")}
            >
              Settings
            </SidebarButton>
          </nav>
          <div className="sidebar-status">
            <StatusPill tone={snapshot.health.attentionCount > 0 ? "warn" : "ready"}>
              {snapshot.health.attentionCount > 0 ? "Pending Changes" : "Notion Ready"}
            </StatusPill>
          </div>
        </aside>

        <section className="content">
          {view === "home" && <HomeView snapshot={snapshot} onReview={() => onViewChange("pending")} />}
          {view === "pending" && <PendingView snapshot={snapshot} onReview={() => onViewChange("review")} />}
          {view === "review" && <ReviewView onDone={() => onViewChange("activity")} />}
          {view === "activity" && <ActivityView snapshot={snapshot} />}
          {view === "settings" && <SettingsView snapshot={snapshot} />}
        </section>
      </div>
    </main>
  );
}

function HomeView({ snapshot, onReview }: { snapshot: DesktopSnapshot; onReview: () => void }) {
  const [url, setUrl] = useState("");
  const [locateState, setLocateState] = useState<LocateState>("idle");
  const [locatedItem, setLocatedItem] = useState<LocatedItem | null>(null);

  async function locatePage() {
    if (!url.trim()) {
      return;
    }
    setLocateState("preparing");
    try {
      const item = await callCommand<LocatedItem>(
        "locate_notion_page",
        { url },
        {
          title: "Roadmap 2026",
          kind: "Page",
          localPath: "~/Documents/AFS/Notion/Engineering/Roadmap 2026 ~a3f2.md",
          state: "ready",
        },
      );
      setLocatedItem(item);
      setLocateState("ready");
    } catch {
      setLocateState("error");
      setLocatedItem(null);
    }
  }

  return (
    <div className="view-stack">
      <ViewHeader eyebrow="Home" title="Notion workspace">
        <StatusPill tone="ready">Ready</StatusPill>
      </ViewHeader>

      <section className="workspace-card">
        <div>
          <p className="label">Connected workspace</p>
          <h2>{snapshot.mount.workspaceName}</h2>
          <p className="path-line">{snapshot.mount.localPath}</p>
        </div>
        <SecondaryButton icon={<FolderOpen />} onClick={() => void callCommand("open_path", { path: snapshot.mount.localPath }, { ok: true })}>
          Open Folder
        </SecondaryButton>
      </section>

      <section className="panel locate-panel">
        <LocateBox
          label="Open a Notion page"
          value={url}
          onChange={setUrl}
          onSubmit={locatePage}
          state={locateState}
        />
        {locatedItem && <LocatedPath item={locatedItem} />}
      </section>

      {snapshot.pendingChanges.length > 0 ? (
        <section className="attention-panel">
          <div>
            <p className="label">Pending Changes</p>
            <h2>{snapshot.pendingChanges.length} files have pending changes.</h2>
          </div>
          <PrimaryButton icon={<ListChecks />} onClick={onReview}>
            Review Pending Changes
          </PrimaryButton>
        </section>
      ) : (
        <section className="panel muted-panel">
          <Check />
          <div>
            <h2>No pending changes</h2>
            <p>Local edits will appear here before they update Notion.</p>
          </div>
        </section>
      )}

      <section className="suggestion-card">
        <Sparkles />
        <div>
          <p className="label">Suggestion</p>
          <h3>Connect {snapshot.suggestions[0]?.connector ?? "Linear"}</h3>
          <p>{snapshot.suggestions[0]?.description ?? "Mount more workspaces as local files."}</p>
        </div>
        <SecondaryButton compact>Coming Soon</SecondaryButton>
      </section>
    </div>
  );
}

function PendingView({ snapshot, onReview }: { snapshot: DesktopSnapshot; onReview: () => void }) {
  return (
    <div className="view-stack">
      <ViewHeader eyebrow="Pending" title="Pending Changes">
        <PrimaryButton icon={<ListChecks />} onClick={onReview}>
          Review Push
        </PrimaryButton>
      </ViewHeader>
      <p className="view-copy">{snapshot.pendingChanges.length} files have pending changes.</p>
      <FileChangeList changes={snapshot.pendingChanges} />
    </div>
  );
}

function ReviewView({ onDone }: { onDone: () => void }) {
  const [plan, setPlan] = useState<PushPlan>(samplePushPlan);
  const [complete, setComplete] = useState(false);

  useEffect(() => {
    void callCommand<PushPlan>("review_push_plan", undefined, samplePushPlan)
      .then(setPlan)
      .catch(() => setPlan(samplePushPlan));
  }, []);

  async function push() {
    await callCommand("push_to_notion", undefined, { ok: true });
    setComplete(true);
  }

  if (complete) {
    return (
      <div className="center-result">
        <BrandTile variant="ready" />
        <h1>Pushed to Notion</h1>
        <p>3 files updated successfully.</p>
        <PrimaryButton onClick={onDone}>Done</PrimaryButton>
      </div>
    );
  }

  return (
    <div className="view-stack">
      <ViewHeader eyebrow="Review Push" title={plan.title}>
        <StatusPill tone="ready">Safe</StatusPill>
      </ViewHeader>
      <p className="view-copy">{plan.summary}</p>

      <section className="summary-grid">
        <Metric label="Pages updated" value={plan.pagesUpdated} />
        <Metric label="Database rows updated" value={plan.databaseRowsUpdated} />
        <Metric label="Pages deleted" value={plan.pagesDeleted} />
      </section>

      <FileChangeList changes={plan.files} />

      <div className="footer-actions">
        <PrimaryButton icon={<ShieldCheck />} onClick={push}>
          Push to Notion
        </PrimaryButton>
        <SecondaryButton>Cancel</SecondaryButton>
      </div>
    </div>
  );
}

function ActivityView({ snapshot }: { snapshot: DesktopSnapshot }) {
  const grouped = useMemo(() => {
    return snapshot.activity.reduce<Record<string, ActivityItem[]>>((acc, item) => {
      acc[item.when] = [...(acc[item.when] ?? []), item];
      return acc;
    }, {});
  }, [snapshot.activity]);

  return (
    <div className="view-stack">
      <ViewHeader eyebrow="Activity" title="Recent activity" />
      {Object.entries(grouped).map(([when, items]) => (
        <section className="activity-group" key={when}>
          <p className="label">{when}</p>
          {items.map((item) => (
            <article className="activity-item" key={`${when}-${item.title}`}>
              <Clock3 />
              <div>
                <h3>{item.title}</h3>
                <p>{item.detail}</p>
              </div>
              {item.undoAvailable && (
                <SecondaryButton compact icon={<RotateCcw />}>
                  Undo Push
                </SecondaryButton>
              )}
            </article>
          ))}
        </section>
      ))}
    </div>
  );
}

function SettingsView({ snapshot }: { snapshot: DesktopSnapshot }) {
  return (
    <div className="view-stack">
      <ViewHeader eyebrow="Controls" title="Mount detail and settings" />
      <section className="workspace-card">
        <div>
          <p className="label">Notion</p>
          <h2>{snapshot.mount.workspaceName}</h2>
          <p className="path-line">{snapshot.mount.localPath}</p>
        </div>
        <PrimaryButton icon={<FolderOpen />}>Open Folder</PrimaryButton>
      </section>

      <section className="settings-grid">
        <div className="panel">
          <PanelTitle title="Location" />
          <PathRow path={snapshot.mount.localPath} />
          <SettingRow title="Status" value="Ready" />
          <SettingRow title="Access" value={snapshot.mount.readOnly ? "Read Only" : "Edit enabled"} />
          <SettingRow title="Source scope" value={`${snapshot.mount.workspaceName} workspace`} />
          <SettingRow
            title="Mounted content"
            value="Uses Notion workspace and top-level page hierarchy inside the local folder."
          />
        </div>

        <div className="panel">
          <PanelTitle title="General" />
          <ToggleRow title="Launch AFS at login" enabled />
          <ToggleRow title="Show AFS in the menu bar" enabled />
          <SettingRow title="Default folder" value="~/Documents/AFS" />
          <PanelTitle title="Safety" />
          <SettingRow title="Push confirmation" value="Require for large changes" />
          <SettingRow title="Default new mount mode" value="Edit enabled" />
        </div>

        <div className="panel">
          <PanelTitle title="Diagnostics" />
          <SettingRow title="AFS process" value="Running" />
          <SettingRow title="State folder" value="~/.afs" />
          <SettingRow title="Projection" value={snapshot.mount.projection} />
          <div className="button-row">
            <SecondaryButton compact>Copy Summary</SecondaryButton>
            <SecondaryButton compact>Restart AFS</SecondaryButton>
          </div>
        </div>

        <div className="panel">
          <PanelTitle title="Quit Options" />
          <button className="option-row">
            <EyeOff />
            <span>Don't Show in Menubar</span>
            <ChevronRight />
          </button>
          <button className="option-row danger">
            <Power />
            <span>Quit Completely</span>
            <ChevronRight />
          </button>
        </div>
      </section>
    </div>
  );
}

function FileChangeList({ changes }: { changes: PendingChange[] }) {
  return (
    <section className="file-list">
      {changes.map((change) => (
        <article className={`file-row ${change.state}`} key={change.localPath}>
          <div className="file-state">
            {change.state === "needs_review" ? <AlertTriangle /> : <Check />}
          </div>
          <div>
            <h3>{change.title}</h3>
            <p>{change.localPath}</p>
            <span>{change.summary}</span>
          </div>
          <SecondaryButton compact>Open</SecondaryButton>
        </article>
      ))}
    </section>
  );
}

function LocateBox({
  label,
  value,
  onChange,
  onSubmit,
  state,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  state: LocateState;
}) {
  return (
    <div className="locate-box">
      <label>{label}</label>
      <div className="locate-row">
        <Search />
        <input
          value={value}
          placeholder="Paste a Notion URL to get the local file path"
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              onSubmit();
            }
          }}
        />
        <PrimaryButton compact disabled={!value.trim() || state === "preparing"} onClick={onSubmit}>
          {state === "preparing" ? "Preparing" : "Open Page"}
        </PrimaryButton>
      </div>
      {state === "error" && <p className="field-error">Paste a Notion page or database URL.</p>}
    </div>
  );
}

function LocatedPath({ item }: { item: LocatedItem }) {
  return (
    <div className="located-path">
      <div>
        <p className="label">{item.kind}</p>
        <h3>{item.title}</h3>
        <code>{item.localPath}</code>
      </div>
      <div className="button-row">
        <SecondaryButton compact icon={<Copy />} onClick={() => copyText(item.localPath)}>
          Copy Path
        </SecondaryButton>
        <SecondaryButton compact icon={<FolderOpen />}>
          Reveal in Finder
        </SecondaryButton>
      </div>
    </div>
  );
}

function AgentPrompt() {
  return (
    <div className="agent-prompt">
      <Clipboard />
      <div>
        <span>Try this with an agent</span>
        <p>"Edit this Notion file and make the launch plan clearer."</p>
      </div>
    </div>
  );
}

function ViewHeader({
  eyebrow,
  title,
  children,
}: {
  eyebrow: string;
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <header className="view-header">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
      </div>
      {children}
    </header>
  );
}

function WindowChrome({ title, meta }: { title: string; meta?: string }) {
  return (
    <div className="window-chrome" data-tauri-drag-region>
      <div className="traffic">
        <button aria-label="Hide window" className="traffic-dot close" onClick={() => void windowAction("hide")} />
        <button aria-label="Minimize window" className="traffic-dot minimize" onClick={() => void windowAction("minimize")} />
        <button aria-label="Toggle fullscreen" className="traffic-dot zoom" onClick={() => void windowAction("toggleMaximize")} />
      </div>
      <div data-tauri-drag-region>{title}</div>
      <div data-tauri-drag-region>{meta}</div>
    </div>
  );
}

async function windowAction(action: "hide" | "minimize" | "toggleMaximize") {
  if (!isTauriRuntime()) {
    return;
  }

  const currentWindow = getCurrentWindow();
  if (action === "hide") {
    await currentWindow.hide();
  } else if (action === "minimize") {
    await currentWindow.minimize();
  } else {
    await currentWindow.toggleMaximize();
  }
}

function SetupContent({ mark, children }: { mark: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="setup-content">
      {mark}
      {children}
    </div>
  );
}

function BrandTile({
  children,
  variant,
}: {
  children?: React.ReactNode;
  variant?: "notion" | "folder" | "progress" | "ready";
}) {
  return (
    <div className={`brand-tile ${variant ?? ""}`}>
      {variant === "folder" && <FolderOpen />}
      {variant === "progress" && <Loader2 />}
      {variant === "ready" && <Check />}
      {!variant && children}
      {variant === "notion" && children}
    </div>
  );
}

function ProgressList({ items }: { items: { label: string; state: "done" | "active" | "idle" }[] }) {
  return (
    <ol className="progress-list">
      {items.map((item) => (
        <li className={item.state} key={item.label}>
          <span>{item.state === "done" ? <Check /> : null}</span>
          {item.label}
        </li>
      ))}
    </ol>
  );
}

function SidebarButton({
  active,
  icon,
  children,
  onClick,
}: {
  active: boolean;
  icon: React.ReactNode;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button className={`sidebar-link ${active ? "active" : ""}`} onClick={onClick}>
      {icon}
      <span>{children}</span>
    </button>
  );
}

function PrimaryButton({
  children,
  icon,
  compact,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  icon?: React.ReactNode;
  compact?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button className={`primary-button ${compact ? "compact" : ""}`} disabled={disabled} onClick={onClick}>
      {icon}
      <span>{children}</span>
    </button>
  );
}

function SecondaryButton({
  children,
  icon,
  compact,
  onClick,
}: {
  children: React.ReactNode;
  icon?: React.ReactNode;
  compact?: boolean;
  onClick?: () => void;
}) {
  return (
    <button className={`secondary-button ${compact ? "compact" : ""}`} onClick={onClick}>
      {icon}
      <span>{children}</span>
    </button>
  );
}

function TextButton({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) {
  return (
    <button className="text-button" onClick={onClick}>
      {children}
    </button>
  );
}

function StatusPill({ children, tone }: { children: React.ReactNode; tone: "ready" | "warn" | "danger" }) {
  return <span className={`status-pill ${tone}`}>{children}</span>;
}

function ApertureIcon({ state = "default" }: { state?: "default" | "review" | "reconnect" }) {
  return (
    <span className={`aperture-icon ${state}`}>
      <svg aria-hidden="true" viewBox="0 0 28 18">
        <path d="M7 14.4 4.5 9 7 3.6" />
        <path d="M21 3.6 23.5 9 21 14.4" />
        <path d="M9.5 5.7h9" />
        <path d="M9.5 12.3h9" />
        <path d="M12 9h4" />
      </svg>
      {state !== "default" && <i />}
    </span>
  );
}

function PanelTitle({ title }: { title: string }) {
  return <h3 className="panel-title">{title}</h3>;
}

function SettingRow({ title, value }: { title: string; value: string }) {
  return (
    <div className="setting-row">
      <span>{title}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ToggleRow({ title, enabled }: { title: string; enabled: boolean }) {
  return (
    <div className="setting-row">
      <span>{title}</span>
      <button className={`toggle ${enabled ? "enabled" : ""}`} aria-label={title}>
        <i />
      </button>
    </div>
  );
}

function PathRow({ path }: { path: string }) {
  return (
    <div className="path-row">
      <code>{path}</code>
      <SecondaryButton compact>Move</SecondaryButton>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <article className="metric">
      <strong>{value}</strong>
      <span>{label}</span>
    </article>
  );
}

function copyText(value: string) {
  void navigator.clipboard?.writeText(value);
}
