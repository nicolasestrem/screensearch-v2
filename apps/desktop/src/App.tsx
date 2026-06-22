import {
  type FormEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Camera,
  CaretDown,
  CheckCircle,
  ClockCounterClockwise,
  Gear,
  ImageSquare,
  Info,
  MagnifyingGlass,
  Pause,
  Play,
  ShieldCheck,
  Sparkle,
  X,
} from "@phosphor-icons/react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  DEFAULT_HOTKEY,
  isTauri,
  type ArchiveSettings,
  type AutomationAction,
  type AutomationPlan,
  type AutomationTarget,
  type GenerationModel,
  type SearchEvent,
  type ShellSettings,
} from "./api";

type Citation = Extract<SearchEvent, { kind: "citation" }>;
type DetailTab = "text" | "metadata" | "source";
type ModalName = "privacy" | "settings" | "automation" | null;
type SettingsDraft = {
  retentionDays: number | null;
  diskBudgetBytes: number | null;
  excludedApplications: string;
  excludedTitles: string;
};

export function App() {
  const queryClient = useQueryClient();
  const searchInput = useRef<HTMLInputElement>(null);
  const initialSearch = useRef(false);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const lastFocus = useRef<HTMLElement | null>(null);
  const [referenceTime] = useState(Date.now);
  const [query, setQuery] = useState("What was visible on screen?");
  const [events, setEvents] = useState<SearchEvent[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [dateFilter, setDateFilter] = useState("any");
  const [applicationFilter, setApplicationFilter] = useState("all");
  const [detailTab, setDetailTab] = useState<DetailTab>("text");
  const [modal, setModal] = useState<ModalName>(null);

  const health = useQuery({
    queryKey: ["health"],
    queryFn: api.health,
    refetchInterval: 2_500,
  });
  const capture = useMutation({ mutationFn: api.capture });
  const pause = useMutation({
    mutationFn: api.setCapturePaused,
    onSuccess: (paused) => {
      queryClient.setQueryData(["health"], (current: typeof health.data) => current
        ? { ...current, capturePaused: paused }
        : current);
    },
  });
  const search = useMutation({
    mutationFn: async ({ value, generateAnswer }: { value: string; generateAnswer: boolean }) => {
      setEvents([]);
      setSelectedId(undefined);
      await api.search(value, generateAnswer, (event) => {
        setEvents((current) => [...current, event]);
        if (event.kind === "citation") {
          setSelectedId((current) => current ?? event.chunkId);
        }
      });
    },
  });

  const citations = useMemo(
    () => events.filter((event): event is Citation => event.kind === "citation"),
    [events],
  );
  const answer = useMemo(
    () => events.filter((event) => event.kind === "token").map((event) => event.text).join(""),
    [events],
  );
  const answerCompletion = useMemo(
    () => [...events].reverse().find((event): event is Extract<SearchEvent, { kind: "completed" }> => event.kind === "completed"),
    [events],
  );
  const applications = useMemo(
    () => [...new Set(citations.map((citation) => citation.application))].sort(),
    [citations],
  );
  const filteredCitations = useMemo(
    () => citations.filter((citation) => {
      if (applicationFilter !== "all" && citation.application !== applicationFilter) return false;
      if (dateFilter === "any") return true;
      const captured = safeDate(citation.capturedAt);
      if (!captured) return true;
      const age = referenceTime - captured.getTime();
      const day = 86_400_000;
      if (dateFilter === "today") return age < day;
      if (dateFilter === "week") return age < day * 7;
      return age < day * 30;
    }),
    [applicationFilter, citations, dateFilter, referenceTime],
  );
  const groups = useMemo(() => groupCitations(filteredCitations), [filteredCitations]);
  const selected = filteredCitations.find((citation) => citation.chunkId === selectedId)
    ?? filteredCitations[0];

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchInput.current?.focus();
        searchInput.current?.select();
        return;
      }
      if (event.key === "Escape") {
        if (modal) {
          event.preventDefault();
          setModal(null);
          lastFocus.current?.focus();
        } else if (document.activeElement === searchInput.current) {
          searchInput.current?.blur();
        }
        return;
      }
      if (modal || isTypingTarget(event.target)) return;
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp"
        && event.key !== "Home" && event.key !== "End") return;
      if (!filteredCitations.length) return;
      event.preventDefault();
      const index = filteredCitations.findIndex((citation) => citation.chunkId === selectedId);
      const last = filteredCitations.length - 1;
      let next: number;
      if (event.key === "Home") next = 0;
      else if (event.key === "End") next = last;
      else if (index < 0) next = 0;
      else if (event.key === "ArrowDown") next = Math.min(last, index + 1);
      else next = Math.max(0, index - 1);
      const target = filteredCitations[next];
      setSelectedId(target.chunkId);
      setDetailTab("text");
      itemRefs.current.get(target.chunkId)?.focus();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [filteredCitations, selectedId, modal]);

  useEffect(() => {
    if (!isTauri) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen("summon-search", () => {
      searchInput.current?.focus();
      searchInput.current?.select();
    }).then((dispose) => {
      // If the effect was already cleaned up before listen() resolved, dispose immediately.
      if (active) unlisten = dispose;
      else dispose();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (health.data?.status === "ready" && !initialSearch.current) {
      initialSearch.current = true;
      search.mutate({ value: query, generateAnswer: false });
    }
  }, [health.data?.status, query, search]);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (query.trim()) search.mutate({ value: query.trim(), generateAnswer: false });
  }

  function showRecentEvidence() {
    setDateFilter("any");
    setApplicationFilter("all");
    const newest = citations[0];
    if (!newest) {
      searchInput.current?.focus();
      searchInput.current?.select();
      return;
    }
    setSelectedId(newest.chunkId);
    setDetailTab("metadata");
    requestAnimationFrame(() => {
      document.querySelector(".timeline-pane")?.scrollTo({ top: 0, behavior: "smooth" });
      itemRefs.current.get(newest.chunkId)?.focus();
    });
  }

  function showVisualEvidence() {
    if (!selected) {
      searchInput.current?.focus();
      searchInput.current?.select();
      return;
    }
    setSelectedId(selected.chunkId);
    setDetailTab("source");
    requestAnimationFrame(() => {
      document.querySelector(".detail-pane")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  }

  function openModal(name: Exclude<ModalName, null>) {
    lastFocus.current = document.activeElement as HTMLElement | null;
    setModal(name);
  }

  function closeModal() {
    setModal(null);
    lastFocus.current?.focus();
  }

  const paused = health.data?.capturePaused ?? false;
  const captureState = health.data?.captureState ?? "paused";
  const backpressured = captureState === "backpressured";
  const error = health.error || capture.error || pause.error || search.error;

  return (
    <main className="app-frame">
      <header className="topbar">
        <div className="brand" aria-label="ScreenSearch V2">
          <span className="brand-mark"><Archive weight="duotone" /></span>
          <span>ScreenSearch <strong>V2</strong></span>
        </div>
        <form className="command-search" onSubmit={submit}>
          <MagnifyingGlass aria-hidden="true" />
          <input
            ref={searchInput}
            aria-label="Search your screen memory"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search your screen memory…"
          />
          <div className="search-actions">
            <kbd>Ctrl K</kbd>
            <button type="submit" className="search-submit" disabled={!query.trim() || search.isPending}>
              {search.isPending ? "Searching…" : "Search"}
            </button>
          </div>
        </form>
        <div className="top-actions">
          <button
            className="pause-button"
            type="button"
            onClick={() => pause.mutate(!paused)}
            disabled={!health.data || pause.isPending}
          >
            {paused ? <Play weight="fill" /> : <Pause weight="fill" />}
            {paused ? "Resume capture" : "Pause capture"}
          </button>
          <span className={`capture-state ${paused || backpressured ? "is-paused" : ""}`}>
            <i /> {paused ? "Paused" : backpressured ? "Catching up" : "Capturing"}
          </span>
          <button className="automation-button" type="button" onClick={() => openModal("automation")}>
            <ShieldCheck /> Automation
          </button>
          <IconButton label="Settings" onClick={() => openModal("settings")}><Gear /></IconButton>
        </div>
      </header>

      <div className="work-area">
        <nav className="rail" aria-label="Primary navigation">
          <IconButton label="Search" active onClick={() => searchInput.current?.focus()}><MagnifyingGlass /></IconButton>
          <IconButton label="Recent evidence" onClick={showRecentEvidence}><ClockCounterClockwise /></IconButton>
          <IconButton label="Visual evidence" onClick={showVisualEvidence}><ImageSquare /></IconButton>
          <span className="rail-spacer" />
          <IconButton label="Privacy and exclusions" onClick={() => openModal("privacy")}><ShieldCheck /></IconButton>
        </nav>

        <section className="product-surface">
          <div className="filterbar">
            <SelectControl label="Date" value={dateFilter} onChange={setDateFilter}>
              <option value="any">Any time</option>
              <option value="today">Today</option>
              <option value="week">Last 7 days</option>
              <option value="month">Last 30 days</option>
            </SelectControl>
            <SelectControl label="Application" value={applicationFilter} onChange={setApplicationFilter}>
              <option value="all">All applications</option>
              {applications.map((application) => <option key={application}>{application}</option>)}
            </SelectControl>
            <button className="filter-button" type="button" onClick={() => openModal("privacy")}>
              <ShieldCheck /> Privacy & exclusions <CaretDown />
            </button>
            <span className="result-count">
              {search.isPending ? "Searching local index…" : `${filteredCitations.length} evidence matches`}
            </span>
          </div>

          <div className="evidence-workspace">
            <aside className="timeline-pane" aria-label="Evidence timeline">
              {groups.map((group) => (
                <section className="timeline-group" key={group.label}>
                  <h2>{group.label} <span>{group.items.length}</span></h2>
                  {group.items.map((citation) => (
                    <TimelineItem
                      key={citation.chunkId}
                      citation={citation}
                      selected={citation.chunkId === selected?.chunkId}
                      itemRef={(node) => {
                        if (node) itemRefs.current.set(citation.chunkId, node);
                        else itemRefs.current.delete(citation.chunkId);
                      }}
                      onSelect={() => {
                        setSelectedId(citation.chunkId);
                        setDetailTab("text");
                      }}
                    />
                  ))}
                </section>
              ))}
              {!search.isPending && groups.length === 0 && (
                <div className="empty-state">
                  <MagnifyingGlass />
                  <strong>No local evidence found</strong>
                  <span>Try broader words or a longer date range.</span>
                </div>
              )}
            </aside>

            <section className="detail-pane" aria-label="Selected evidence">
              {selected ? (
                <EvidenceDetail
                  citation={selected}
                  tab={detailTab}
                  answer={answer}
                  answerStatus={answerCompletion?.answerStatus ?? "evidence_only"}
                  answerMessage={answerCompletion?.answerMessage ?? ""}
                  searching={search.isPending}
                  onTabChange={setDetailTab}
                  onGenerate={() => search.mutate({ value: query.trim(), generateAnswer: true })}
                />
              ) : (
                <div className="detail-empty">
                  <Archive />
                  <h1>Your private screen memory</h1>
                  <p>Search above to inspect screenshots, positioned OCR text, and local semantic matches.</p>
                </div>
              )}
            </section>
          </div>
        </section>
      </div>

      <footer className="statusbar">
        <span><ShieldCheck /> Offline mode · all processing stays local</span>
        <span className={health.data ? "healthy" : "unhealthy"}>
          {health.data ? <CheckCircle weight="fill" /> : <Info weight="fill" />}
          {health.data
            ? `Index ready · ${health.data.queueDepth} queued · daemon ${health.data.version}`
            : "Daemon offline"}
        </span>
      </footer>

      {capture.data && (
        <div className="toast" role="status">
          <CheckCircle weight="fill" />
          {capture.data.skippedReason
            ? captureSkipMessage(capture.data.skippedReason)
            : capture.data.duplicate
              ? "This frame was already indexed"
              : "Current frame queued for indexing"}
        </div>
      )}
      {error && <div className="error-toast" role="alert">{String(error)}</div>}
      {modal === "automation" && <AutomationModal onClose={closeModal} />}
      {modal && modal !== "automation" && <SettingsModal name={modal} paused={paused} onClose={closeModal} onCapture={() => capture.mutate()} />}
    </main>
  );
}

function EvidenceDetail({
  citation,
  tab,
  answer,
  answerStatus,
  answerMessage,
  searching,
  onTabChange,
  onGenerate,
}: {
  citation: Citation;
  tab: DetailTab;
  answer: string;
  answerStatus: string;
  answerMessage: string;
  searching: boolean;
  onTabChange: (tab: DetailTab) => void;
  onGenerate: () => void;
}) {
  const tabOrder = ["text", "metadata", "source"] as const;
  const tabRefs = useRef<Partial<Record<DetailTab, HTMLButtonElement | null>>>({});
  return (
    <>
      <div className="detail-heading">
        <div>
          <span>{dayLabel(citation.capturedAt)}</span>
          <strong>{formatDateTime(citation.capturedAt)}</strong>
        </div>
        <span>{citation.application} · {citation.matchKind} match</span>
      </div>
      <CaptureImage citation={citation} large />
      <div className="detail-grid">
        <div className="tabbed-panel">
          <div
            className="tabs"
            role="tablist"
            aria-label="Evidence details"
            onKeyDown={(event) => {
              const index = tabOrder.indexOf(tab);
              let next = index;
              if (event.key === "ArrowRight") next = (index + 1) % tabOrder.length;
              else if (event.key === "ArrowLeft") next = (index - 1 + tabOrder.length) % tabOrder.length;
              else if (event.key === "Home") next = 0;
              else if (event.key === "End") next = tabOrder.length - 1;
              else return;
              event.preventDefault();
              onTabChange(tabOrder[next]);
              tabRefs.current[tabOrder[next]]?.focus();
            }}
          >
            {tabOrder.map((value) => (
              <button
                key={value}
                ref={(node) => { tabRefs.current[value] = node; }}
                className={tab === value ? "active" : ""}
                type="button"
                role="tab"
                aria-selected={tab === value}
                tabIndex={tab === value ? 0 : -1}
                onClick={() => onTabChange(value)}
              >
                {value === "text" ? "Extracted text" : value === "metadata" ? "Metadata" : "Source"}
              </button>
            ))}
          </div>
          <div className="tab-content">
            {tab === "text" && <p>{citation.excerpt}</p>}
            {tab === "metadata" && (
              <DefinitionList rows={[
                ["Application", citation.application],
                ["Window title", citation.windowTitle || "Untitled window"],
                ["Captured", formatDateTime(citation.capturedAt)],
                ["Resolution", `${citation.width} × ${citation.height}`],
                ["Match", `${citation.matchKind} · ${Math.round(citation.score * 100)}%`],
              ]} />
            )}
            {tab === "source" && (
              <DefinitionList rows={[
                ["Capture ID", citation.captureId],
                ["OCR engine", citation.ocrModelId],
                ["Embedding", citation.embeddingModelId],
              ]} />
            )}
          </div>
        </div>
        <aside className="metadata-card">
          <DefinitionList rows={[
            ["Application", citation.application],
            ["Window title", citation.windowTitle || "Untitled window"],
            ["Captured", formatDateTime(citation.capturedAt)],
            ["Resolution", `${citation.width} × ${citation.height}`],
            ["OCR engine", citation.ocrModelId],
          ]} />
        </aside>
      </div>
      <section className="answer-panel">
        <div>
          <span className="answer-label"><Sparkle weight="fill" /> Answer (optional)</span>
          <small>{answerSubtitle(answer, answerStatus, answerMessage)}</small>
        </div>
        {answer ? <p>{answer}</p> : (
          <button type="button" onClick={onGenerate} disabled={searching}>
            <Sparkle /> {searching ? "Generating locally…" : "Generate from evidence"}
          </button>
        )}
      </section>
    </>
  );
}

function TimelineItem({ citation, selected, itemRef, onSelect }: { citation: Citation; selected: boolean; itemRef: (node: HTMLButtonElement | null) => void; onSelect: () => void }) {
  return (
    <button
      ref={itemRef}
      className={`timeline-item ${selected ? "selected" : ""}`}
      type="button"
      tabIndex={selected ? 0 : -1}
      aria-current={selected || undefined}
      onClick={onSelect}
    >
      <CaptureImage citation={citation} />
      <span className="timeline-copy">
        <time>{formatTime(citation.capturedAt)}</time>
        <strong>{citation.application}</strong>
        <span>{citation.windowTitle || citation.excerpt}</span>
        <small>{citation.matchKind} · {Math.round(citation.score * 100)}%</small>
      </span>
    </button>
  );
}

function CaptureImage({ citation, large = false }: { citation: Citation; large?: boolean }) {
  const image = useQuery({
    queryKey: ["capture-image", citation.captureId],
    queryFn: async () => {
      const asset = await api.captureAsset(citation.captureId);
      return readBlobAsDataUrl(new Blob(
        [new Uint8Array(asset.content)],
        { type: asset.mediaType },
      ));
    },
    staleTime: Number.POSITIVE_INFINITY,
  });
  const imageUrl = image.data;

  return (
    <span className={`capture-image ${large ? "large" : "thumbnail"}`}>
      {imageUrl
        ? <img src={imageUrl} alt={`Screen captured from ${citation.application}`} />
        : <span className="image-loading">{image.error ? "Preview unavailable" : "Loading evidence…"}</span>}
      {imageUrl && citation.bounds.map((bounds, index) => (
        <i
          className="ocr-highlight"
          key={`${citation.chunkId}-${index}`}
          style={{
            left: `${bounds.x * 100}%`,
            top: `${bounds.y * 100}%`,
            width: `${bounds.width * 100}%`,
            height: `${bounds.height * 100}%`,
          }}
        />
      ))}
    </span>
  );
}

function DefinitionList({ rows }: { rows: [string, string][] }) {
  return (
    <dl>
      {rows.map(([term, value]) => <div key={term}><dt>{term}</dt><dd title={value}>{value}</dd></div>)}
    </dl>
  );
}

function SelectControl({ label, value, onChange, children }: { label: string; value: string; onChange: (value: string) => void; children: ReactNode }) {
  return (
    <label className="select-control">
      <span>{label}:</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>{children}</select>
      <CaretDown aria-hidden="true" />
    </label>
  );
}

function IconButton({ label, active = false, onClick, children }: { label: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  return <button className={`icon-button ${active ? "active" : ""}`} type="button" aria-label={label} title={label} onClick={onClick}>{children}</button>;
}

function AutomationModal({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const dialogRef = useRef<HTMLElement>(null);
  const [warningAccepted, setWarningAccepted] = useState(false);
  const [target, setTarget] = useState<AutomationTarget | null>(null);
  const [actions, setActions] = useState<AutomationAction[]>([{ kind: "type_text", text: "preview" }]);
  const [approvalId, setApprovalId] = useState<string>();
  const [result, setResult] = useState<string>();
  const status = useQuery({ queryKey: ["automation-status"], queryFn: api.automationStatus, refetchInterval: 2_500 });
  const enable = useMutation({
    mutationFn: (enabled: boolean) => api.setAutomationEnabled(enabled),
    onSuccess: (value) => queryClient.setQueryData(["automation-status"], value),
  });
  const captureTarget = useMutation({
    mutationFn: api.automationForegroundTarget,
    onSuccess: (value) => {
      setTarget(value);
      setApprovalId(undefined);
      setResult(undefined);
    },
  });
  const approve = useMutation({
    mutationFn: (plan: AutomationPlan) => api.approveAutomation(plan),
    onSuccess: (approval) => {
      setApprovalId(approval.approvalId);
      setResult(`Approved ${approval.actionCount} action(s) until ${formatTime(approval.expiresAt)}.`);
    },
  });
  const execute = useMutation({
    mutationFn: ({ id, plan }: { id: string; plan: AutomationPlan }) => api.executeAutomation(id, plan),
    onSuccess: (value) => {
      setApprovalId(undefined);
      setResult(value || "Execution succeeded.");
      void queryClient.invalidateQueries({ queryKey: ["automation-status"] });
    },
  });
  const abort = useMutation({
    mutationFn: api.abortAutomation,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["automation-status"] }),
  });
  const resetAbort = useMutation({
    mutationFn: api.resetAutomationAbort,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["automation-status"] }),
  });

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("button, input, select, textarea")?.focus();
  }, []);

  const plan = target ? { target, actions } : null;
  const planReady = Boolean(plan && actions.length > 0 && actions.length <= 10);
  const busy = enable.isPending || captureTarget.isPending || approve.isPending || execute.isPending || abort.isPending || resetAbort.isPending;
  const modalError = status.error || enable.error || captureTarget.error || approve.error || execute.error || abort.error || resetAbort.error;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section ref={dialogRef} className="modal automation-modal" role="dialog" aria-modal="true" aria-labelledby="automation-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-heading">
          <div>
            <span><ShieldCheck /></span>
            <div><h2 id="automation-title">Guarded automation</h2><p>Manual, default-off actions with foreground and abort checks.</p></div>
          </div>
          <IconButton label="Close" onClick={onClose}><X /></IconButton>
        </div>
        <div className="modal-content">
          <div className="notice automation-warning">
            <Info weight="fill" />
            <p><strong>Automation emits real input only after approval.</strong><span>Use it only on the captured foreground app. Emergency abort is <kbd>Ctrl Alt Shift Esc</kbd>.</span></p>
          </div>
          <div className="setting-row">
            <span><strong>Enable guarded automation</strong><small>Requires a live abort shortcut heartbeat and remains off by default.</small></span>
            <span className={`state-pill ${status.data?.enabled ? "" : "paused"}`}>{status.data?.enabled ? "Enabled" : "Disabled"}</span>
          </div>
          <label className="automation-confirm">
            <input type="checkbox" checked={warningAccepted} onChange={(event) => setWarningAccepted(event.target.checked)} />
            <span>I understand this can send keyboard/UI Automation input to the foreground application.</span>
          </label>
          <div className="automation-actions">
            <button type="button" onClick={() => enable.mutate(true)} disabled={!warningAccepted || busy || status.data?.enabled}>{enable.isPending ? "Enabling…" : "Enable"}</button>
            <button type="button" className="secondary" onClick={() => enable.mutate(false)} disabled={busy || !status.data?.enabled}>Disable</button>
            <button type="button" className="danger" onClick={() => abort.mutate()} disabled={busy}>Abort now</button>
            <button type="button" className="secondary" onClick={() => resetAbort.mutate()} disabled={busy || !status.data?.abortActive}>Reset abort</button>
          </div>
          <div className="automation-safety-grid">
            <span><strong>{status.data?.abortAvailable ? "Live" : "Unavailable"}</strong><small>Abort shortcut</small></span>
            <span><strong>{status.data?.abortActive ? "Latched" : "Clear"}</strong><small>Abort state</small></span>
            <span><strong>{status.data?.running ? "Running" : "Idle"}</strong><small>Execution gate</small></span>
          </div>
          <div className="setting-row">
            <span><strong>Target application</strong><small>ScreenSearch hides briefly so the previous app can regain foreground.</small></span>
            <button type="button" onClick={() => captureTarget.mutate()} disabled={busy || !status.data?.enabled}>{captureTarget.isPending ? "Capturing…" : "Capture target"}</button>
          </div>
          {target && <div className="automation-target"><strong>{target.displayTitle}</strong><small>{target.executableName} · PID {target.processId} · HWND {target.windowHandle}</small></div>}
          <AutomationActionList actions={actions} onChange={(next) => { setActions(next); setApprovalId(undefined); }} />
          <section className="automation-review" aria-label="Automation plan review">
            <strong>Exact review</strong>
            <pre>{plan ? JSON.stringify(plan, null, 2) : "Capture a target before approving."}</pre>
          </section>
          <div className="modal-actions">
            <span>{result ?? "Approve and execute are separate steps. Approval expires after 60 seconds."}</span>
            <div className="automation-actions compact">
              <button type="button" onClick={() => plan && approve.mutate(plan)} disabled={!planReady || busy || !status.data?.enabled}>{approve.isPending ? "Approving…" : "Approve"}</button>
              <button type="button" onClick={() => plan && approvalId && execute.mutate({ id: approvalId, plan })} disabled={!planReady || !approvalId || busy}>{execute.isPending ? "Executing…" : "Execute approved"}</button>
            </div>
          </div>
        </div>
        {modalError && <div className="modal-error" role="alert">{errorText(modalError)}</div>}
      </section>
    </div>
  );
}

function AutomationActionList({ actions, onChange }: { actions: AutomationAction[]; onChange: (actions: AutomationAction[]) => void }) {
  function update(index: number, action: AutomationAction) {
    onChange(actions.map((item, itemIndex) => itemIndex === index ? action : item));
  }
  function move(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= actions.length) return;
    const next = [...actions];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  }
  return (
    <section className="automation-builder" aria-label="Automation action builder">
      <div className="model-heading">
        <span><strong>Actions</strong><small>1–10 ordered typed actions. Keyboard fallback is explicit.</small></span>
        <button type="button" onClick={() => onChange([...actions, { kind: "type_text", text: "" }])} disabled={actions.length >= 10}>Add action</button>
      </div>
      {actions.map((action, index) => (
        <div className="automation-action-row" key={index}>
          <select aria-label={`Action ${index + 1} type`} value={action.kind} onChange={(event) => update(index, defaultAutomationAction(event.target.value))}>
            <option value="type_text">Type text</option>
            <option value="key_chord">Key chord</option>
            <option value="uia_invoke">UIA invoke</option>
            <option value="uia_set_value">UIA set value</option>
          </select>
          <AutomationActionFields action={action} onChange={(next) => update(index, next)} />
          <div>
            <button type="button" className="secondary" onClick={() => move(index, -1)} disabled={index === 0}>↑</button>
            <button type="button" className="secondary" onClick={() => move(index, 1)} disabled={index === actions.length - 1}>↓</button>
            <button type="button" className="danger" onClick={() => onChange(actions.filter((_, itemIndex) => itemIndex !== index))} disabled={actions.length <= 1}>Remove</button>
          </div>
        </div>
      ))}
    </section>
  );
}

function AutomationActionFields({ action, onChange }: { action: AutomationAction; onChange: (action: AutomationAction) => void }) {
  if (action.kind === "uia_invoke") {
    return <input aria-label="Automation ID" value={action.automationId} onChange={(event) => onChange({ ...action, automationId: event.target.value })} placeholder="Exact Automation ID" />;
  }
  if (action.kind === "uia_set_value") {
    return (
      <div className="automation-field-pair">
        <input aria-label="Automation ID" value={action.automationId} onChange={(event) => onChange({ ...action, automationId: event.target.value })} placeholder="Exact Automation ID" />
        <input aria-label="Value" value={action.value} onChange={(event) => onChange({ ...action, value: event.target.value })} placeholder="Value" />
      </div>
    );
  }
  if (action.kind === "key_chord") {
    return (
      <div className="automation-field-pair">
        <div className="modifier-list">
          {(["control", "alt", "shift"] as const).map((modifier) => (
            <label key={modifier}>
              <input type="checkbox" checked={action.modifiers.includes(modifier)} onChange={(event) => {
                const modifiers = event.target.checked
                  ? [...action.modifiers, modifier]
                  : action.modifiers.filter((item) => item !== modifier);
                onChange({ ...action, modifiers });
              }} />
              {modifier}
            </label>
          ))}
        </div>
        <input aria-label="Key" value={action.key} onChange={(event) => onChange({ ...action, key: event.target.value })} placeholder="S, Enter, F5…" />
      </div>
    );
  }
  return <input aria-label="Text to type" value={action.text} onChange={(event) => onChange({ ...action, text: event.target.value })} placeholder="Text to type" maxLength={512} />;
}

function defaultAutomationAction(kind: string): AutomationAction {
  if (kind === "uia_invoke") return { kind: "uia_invoke", automationId: "" };
  if (kind === "uia_set_value") return { kind: "uia_set_value", automationId: "", value: "" };
  if (kind === "key_chord") return { kind: "key_chord", modifiers: ["control"], key: "enter" };
  return { kind: "type_text", text: "" };
}

function errorText(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

function SettingsModal({ name, paused, onClose, onCapture }: { name: Exclude<ModalName, null | "automation">; paused: boolean; onClose: () => void; onCapture: () => void }) {
  const queryClient = useQueryClient();
  const dialogRef = useRef<HTMLElement>(null);
  const settings = useQuery({ queryKey: ["archive-settings"], queryFn: api.archiveSettings });
  const shell = useQuery({ queryKey: ["shell-settings"], queryFn: api.getShellSettings });
  const models = useQuery({ queryKey: ["generation-models"], queryFn: api.generationModels });
  const [draft, setDraft] = useState<SettingsDraft>();
  const [modelDraft, setModelDraft] = useState({
    sourcePath: "models\\NVIDIA-Nemotron3-Nano-4B-Q4_K_M.gguf",
    repository: "unsloth/Qwen3.5-4B-GGUF",
    filename: "Qwen3.5-4B-Q4_K_M.gguf",
    displayName: "Qwen3.5 4B Q4_K_M",
  });
  const [confirmDelete, setConfirmDelete] = useState(false);
  const current = draft ?? settingsDraft(settings.data);

  useEffect(() => {
    const root = dialogRef.current;
    if (!root) return;
    const selector = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';
    const focusable = () => [...root.querySelectorAll<HTMLElement>(selector)]
      .filter((element) => !element.hasAttribute("disabled"));
    focusable()[0]?.focus();
    function trap(event: KeyboardEvent) {
      if (event.key !== "Tab") return;
      const items = focusable();
      if (!items.length) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    root.addEventListener("keydown", trap);
    return () => root.removeEventListener("keydown", trap);
  }, []);

  const save = useMutation({
    mutationFn: () => api.updateArchiveSettings({
      retentionDays: current.retentionDays,
      diskBudgetBytes: current.diskBudgetBytes,
      excludedApplications: splitPatterns(current.excludedApplications),
      excludedTitles: splitPatterns(current.excludedTitles),
    }),
    onSuccess: (result) => {
      queryClient.setQueryData<ArchiveSettings>(["archive-settings"], result.settings);
      setDraft(settingsDraft(result.settings));
      void queryClient.invalidateQueries({ queryKey: ["health"] });
    },
  });
  const deleteAll = useMutation({
    mutationFn: () => api.deleteAllCaptures(true),
    onSuccess: () => {
      setConfirmDelete(false);
      void queryClient.invalidateQueries({ queryKey: ["archive-settings"] });
      void queryClient.invalidateQueries({ queryKey: ["health"] });
    },
  });
  const saveHotkey = useMutation({
    mutationFn: (hotkey: string) => api.setShellSettings(hotkey),
    onSuccess: (result) => {
      queryClient.setQueryData<ShellSettings>(["shell-settings"], result);
    },
  });
  const importModel = useMutation({
    mutationFn: () => api.importLocalGenerationModel(modelDraft.sourcePath, modelDraft.displayName, true),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["generation-models"] }),
  });
  const downloadModel = useMutation({
    mutationFn: () => api.downloadGenerationModel(modelDraft.repository, modelDraft.filename, modelDraft.displayName, true),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["generation-models"] }),
  });
  const selectModel = useMutation({
    mutationFn: (modelId: string) => api.selectGenerationModel(modelId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["generation-models"] }),
  });
  const deleteModel = useMutation({
    mutationFn: (modelId: string) => api.deleteGenerationModel(modelId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["generation-models"] }),
  });
  const unloadModel = useMutation({
    mutationFn: api.unloadGenerationModel,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["generation-models"] }),
  });
  const modalError = settings.error || save.error || deleteAll.error || saveHotkey.error
    || models.error || importModel.error || downloadModel.error || selectModel.error || deleteModel.error || unloadModel.error;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section ref={dialogRef} className="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-heading">
          <div>
            <span>{name === "privacy" ? <ShieldCheck /> : <Gear />}</span>
            <div><h2 id="modal-title">{name === "privacy" ? "Privacy & exclusions" : "ScreenSearch settings"}</h2><p>Local controls for this device.</p></div>
          </div>
          <IconButton label="Close" onClick={onClose}><X /></IconButton>
        </div>
        {name === "privacy" ? (
          <div className="modal-content">
            <div className="notice"><ShieldCheck weight="fill" /><p><strong>Offline by design</strong><span>Captures, OCR text, embeddings, and search stay in the local application data directory.</span></p></div>
            <label className="settings-field">
              <span><strong>Application exclusions</strong><small>One case-insensitive application name per line.</small></span>
              <textarea value={current.excludedApplications} onChange={(event) => setDraft({ ...current, excludedApplications: event.target.value })} placeholder={"1password.exe\nprivate-app.exe"} />
            </label>
            <label className="settings-field">
              <span><strong>Window title exclusions</strong><small>Skip windows whose titles contain one of these phrases.</small></span>
              <textarea value={current.excludedTitles} onChange={(event) => setDraft({ ...current, excludedTitles: event.target.value })} placeholder={"Private browsing\nConfidential"} />
            </label>
            <div className="modal-actions">
              <span>{save.isSuccess ? "Privacy exclusions saved" : "Rules apply before screenshots are stored."}</span>
              <button type="button" onClick={() => save.mutate()} disabled={settings.isPending || save.isPending}>{save.isPending ? "Saving…" : "Save exclusions"}</button>
            </div>
          </div>
        ) : (
          <div className="modal-content">
            <div className="setting-row"><span><strong>Automatic capture</strong><small>{paused ? "Capture is paused." : "The focused monitor is captured every two seconds."}</small></span><span className={`state-pill ${paused ? "paused" : ""}`}>{paused ? "Paused" : "Active"}</span></div>
            <div className="storage-summary">
              <span><strong>{settings.data?.captureCount.toLocaleString() ?? "—"}</strong><small>captures</small></span>
              <span><strong>{settings.data ? formatBytes(settings.data.assetBytes) : "—"}</strong><small>screen assets</small></span>
            </div>
            <label className="settings-field compact">
              <span><strong>Age retention</strong><small>Only completed or waiting captures are eligible; active analysis is protected.</small></span>
              <select value={current.retentionDays ?? ""} onChange={(event) => setDraft({ ...current, retentionDays: event.target.value ? Number(event.target.value) : null })}>
                <option value="">Keep all until I choose</option>
                <option value="7">7 days</option>
                <option value="30">30 days</option>
                <option value="90">90 days</option>
                <option value="365">1 year</option>
              </select>
            </label>
            <label className="settings-field compact">
              <span><strong>Screen asset budget</strong><small>Oldest eligible captures are removed first when this limit is exceeded.</small></span>
              <select value={current.diskBudgetBytes ?? ""} onChange={(event) => setDraft({ ...current, diskBudgetBytes: event.target.value ? Number(event.target.value) : null })}>
                <option value="">No asset limit until I choose</option>
                <option value={1 * 1024 ** 3}>1 GB</option>
                <option value={5 * 1024 ** 3}>5 GB</option>
                <option value={10 * 1024 ** 3}>10 GB</option>
                <option value={25 * 1024 ** 3}>25 GB</option>
              </select>
            </label>
            <div className="modal-actions">
              <span>{save.isSuccess ? "Retention policy saved" : "Changes are applied immediately and checked every minute."}</span>
              <button type="button" onClick={() => save.mutate()} disabled={settings.isPending || save.isPending}>{save.isPending ? "Applying…" : "Save storage policy"}</button>
            </div>
            <div className="setting-row"><span><strong>Capture current frame</strong><small>Queue an immediate frame without changing the automatic cadence.</small></span><button type="button" onClick={onCapture}><Camera /> Capture now</button></div>
            <div className="setting-row"><span><strong>Search shortcut</strong><small>Focus search from anywhere in this window.</small></span><kbd>Ctrl K</kbd></div>
            <div className="setting-row"><span><strong>Summon shortcut</strong><small>Bring ScreenSearch to the front from any application.</small></span><HotkeyCapture value={shell.data?.hotkey ?? DEFAULT_HOTKEY} busy={saveHotkey.isPending} onChange={(hotkey) => saveHotkey.mutate(hotkey)} /></div>
            <ModelSettings
              models={models.data ?? []}
              draft={modelDraft}
              busy={importModel.isPending || downloadModel.isPending || selectModel.isPending || deleteModel.isPending || unloadModel.isPending}
              importPending={importModel.isPending}
              downloadPending={downloadModel.isPending}
              unloadPending={unloadModel.isPending}
              onDraft={setModelDraft}
              onImport={() => importModel.mutate()}
              onDownload={() => downloadModel.mutate()}
              onSelect={(modelId) => selectModel.mutate(modelId)}
              onDelete={(modelId) => deleteModel.mutate(modelId)}
              onUnload={() => unloadModel.mutate()}
            />
            <div className="danger-zone">
              <span><strong>Delete all captured history</strong><small>This pauses capture and permanently removes screenshots, OCR, and search indexes. Models are kept.</small></span>
              {confirmDelete ? (
                <div><button type="button" className="secondary" onClick={() => setConfirmDelete(false)}>Cancel</button><button type="button" className="danger" onClick={() => deleteAll.mutate()} disabled={deleteAll.isPending}>{deleteAll.isPending ? "Deleting…" : "Confirm delete all"}</button></div>
              ) : <button type="button" className="danger" onClick={() => setConfirmDelete(true)}>Delete all…</button>}
            </div>
          </div>
        )}
        {modalError && <div className="modal-error" role="alert">{String(modalError)}</div>}
      </section>
    </div>
  );
}

function HotkeyCapture({ value, busy, onChange }: { value: string; busy: boolean; onChange: (hotkey: string) => void }) {
  const [recording, setRecording] = useState(false);
  return (
    <button
      type="button"
      className={`hotkey-capture ${recording ? "recording" : ""}`}
      aria-label="Change summon shortcut"
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={(event) => {
        if (!recording) return;
        if (event.key === "Tab") return;
        event.preventDefault();
        if (event.key === "Escape") {
          setRecording(false);
          return;
        }
        if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return;
        const parts: string[] = [];
        if (event.ctrlKey || event.metaKey) parts.push("CmdOrCtrl");
        if (event.altKey) parts.push("Alt");
        if (event.shiftKey) parts.push("Shift");
        parts.push(normalizeHotkeyKey(event.key));
        setRecording(false);
        onChange(parts.join("+"));
      }}
    >
      {recording ? "Press keys…" : busy ? "Saving…" : <kbd>{prettyHotkey(value)}</kbd>}
    </button>
  );
}

function ModelSettings({
  models,
  draft,
  busy,
  importPending,
  downloadPending,
  unloadPending,
  onDraft,
  onImport,
  onDownload,
  onSelect,
  onDelete,
  onUnload,
}: {
  models: GenerationModel[];
  draft: { sourcePath: string; repository: string; filename: string; displayName: string };
  busy: boolean;
  importPending: boolean;
  downloadPending: boolean;
  unloadPending: boolean;
  onDraft: (draft: { sourcePath: string; repository: string; filename: string; displayName: string }) => void;
  onImport: () => void;
  onDownload: () => void;
  onSelect: (modelId: string) => void;
  onDelete: (modelId: string) => void;
  onUnload: () => void;
}) {
  const active = models.find((model) => model.active);
  return (
    <section className="model-settings" aria-label="Generation models">
      <div className="model-heading">
        <span>
          <strong>Answer model</strong>
          <small>{active ? `${active.displayName} · ${active.quantization || "GGUF"}` : "No generation model selected."}</small>
        </span>
        <button type="button" onClick={onUnload} disabled={busy || !active}>{unloadPending ? "Unloading..." : "Unload"}</button>
      </div>
      <div className="model-source-grid">
        <div className="model-source">
          <span className="model-source-heading">
            <strong>Local GGUF</strong>
            <small>Import ignored repo samples or any local GGUF file.</small>
          </span>
          <label>
            <span>Path</span>
            <input value={draft.sourcePath} onChange={(event) => onDraft({ ...draft, sourcePath: event.target.value })} />
          </label>
          <label>
            <span>Display name</span>
            <input value={draft.displayName} onChange={(event) => onDraft({ ...draft, displayName: event.target.value })} />
          </label>
          <button type="button" onClick={onImport} disabled={busy || !draft.sourcePath.trim()}><Archive /> {importPending ? "Importing..." : "Import local"}</button>
        </div>
        <div className="model-source">
          <span className="model-source-heading">
            <strong>Hugging Face</strong>
            <small>Download one explicit file into local app data.</small>
          </span>
          <label>
            <span>Repository</span>
            <input value={draft.repository} onChange={(event) => onDraft({ ...draft, repository: event.target.value })} />
          </label>
          <label>
            <span>Filename</span>
            <input value={draft.filename} onChange={(event) => onDraft({ ...draft, filename: event.target.value })} />
          </label>
          <button type="button" onClick={onDownload} disabled={busy || !draft.repository.trim() || !draft.filename.trim()}><Sparkle /> {downloadPending ? "Downloading..." : "Download HF"}</button>
        </div>
      </div>
      {models.length > 0 && (
        <div className="model-list">
          {models.map((model) => (
            <div key={model.id} className="model-row">
              <span>
                <strong>{model.displayName}</strong>
                <small>{model.source} · {formatBytes(model.byteLength)} · {model.architecture || "GGUF"} · {model.quantization || "unknown quant"}</small>
              </span>
              <div>
                <button type="button" onClick={() => onSelect(model.id)} disabled={busy || model.active}>Use</button>
                <button type="button" className="secondary" onClick={() => onDelete(model.id)} disabled={busy || model.active}>Delete</button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function isTypingTarget(target: EventTarget | null) {
  const element = target as HTMLElement | null;
  if (!element) return false;
  const tag = element.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || element.isContentEditable;
}

function answerSubtitle(answer: string, status: string, message: string) {
  if (answer) return "Grounded in the evidence above";
  if (status === "model_missing") return message || "Select or download a local GGUF model in Settings.";
  if (status === "no_evidence") return "No matching evidence was found, so generation did not run.";
  if (status === "generation_failed") return message || "Local generation failed.";
  if (status === "cancelled") return "Generation was cancelled.";
  return "Requires an active local GGUF model";
}

function normalizeHotkeyKey(key: string) {
  if (key === " " || key === "Spacebar") return "Space";
  if (key.startsWith("Arrow")) return key.slice(5);
  if (key.length === 1) return key.toUpperCase();
  return key;
}

function prettyHotkey(value: string) {
  return value
    .split("+")
    .map((part) => (part === "CmdOrCtrl" ? "Ctrl" : part))
    .join(" ");
}

function splitPatterns(value: string) {
  return [...new Set(value.split(/\r?\n/).map((pattern) => pattern.trim()).filter(Boolean))];
}

function settingsDraft(settings?: ArchiveSettings): SettingsDraft {
  return {
    retentionDays: settings?.retentionDays ?? null,
    diskBudgetBytes: settings?.diskBudgetBytes ?? null,
    excludedApplications: settings?.excludedApplications.join("\n") ?? "",
    excludedTitles: settings?.excludedTitles.join("\n") ?? "",
  };
}

function formatBytes(bytes: number) {
  if (bytes < 1024 ** 2) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

function groupCitations(citations: Citation[]) {
  const groups = new Map<string, Citation[]>();
  citations.forEach((citation) => {
    const label = dayLabel(citation.capturedAt);
    groups.set(label, [...(groups.get(label) ?? []), citation]);
  });
  return [...groups.entries()].map(([label, items]) => ({ label, items }));
}

const warnedTimestamps = new Set<string>();

// Parse a capture timestamp defensively. A single malformed value must never crash the timeline,
// so unparseable input returns null (callers render a fallback) and is logged once for diagnosis.
function safeDate(value: string): Date | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    if (!warnedTimestamps.has(value)) {
      warnedTimestamps.add(value);
      console.warn("ScreenSearch: unparseable capture timestamp", JSON.stringify(value));
    }
    return null;
  }
  return date;
}

function dayLabel(value: string) {
  const date = safeDate(value);
  if (!date) return "Unknown date";
  const today = new Date();
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const captured = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const difference = Math.round((start - captured) / 86_400_000);
  if (difference === 0) return "Today";
  if (difference === 1) return "Yesterday";
  return "Earlier";
}

function formatTime(value: string) {
  const date = safeDate(value);
  if (!date) return "Unknown time";
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(date);
}

function formatDateTime(value: string) {
  const date = safeDate(value);
  if (!date) return "Unknown date";
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(date);
}

function readBlobAsDataUrl(blob: Blob) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result)), { once: true });
    reader.addEventListener("error", () => reject(reader.error ?? new Error("read capture preview")), { once: true });
    reader.readAsDataURL(blob);
  });
}

function captureSkipMessage(reason: string) {
  if (reason === "paused") return "Capture is paused";
  if (reason === "backpressured") return "Capture is waiting for indexing to catch up";
  if (reason === "near_duplicate") return "No meaningful screen change detected";
  if (reason.startsWith("excluded_")) return "This application is excluded from capture";
  return "Current frame was not captured";
}
