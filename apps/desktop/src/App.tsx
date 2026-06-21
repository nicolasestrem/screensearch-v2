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
import { api, type ArchiveSettings, type SearchEvent } from "./api";

type Citation = Extract<SearchEvent, { kind: "citation" }>;
type DetailTab = "text" | "metadata" | "source";
type ModalName = "privacy" | "settings" | null;
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
  const applications = useMemo(
    () => [...new Set(citations.map((citation) => citation.application))].sort(),
    [citations],
  );
  const filteredCitations = useMemo(
    () => citations.filter((citation) => {
      if (applicationFilter !== "all" && citation.application !== applicationFilter) return false;
      if (dateFilter === "any") return true;
      const age = referenceTime - new Date(citation.capturedAt).getTime();
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
    function focusSearch(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchInput.current?.focus();
      }
    }
    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
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
          <kbd>Ctrl K</kbd>
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
          <IconButton label="Settings" onClick={() => setModal("settings")}><Gear /></IconButton>
        </div>
      </header>

      <div className="work-area">
        <nav className="rail" aria-label="Primary navigation">
          <IconButton label="Search" active onClick={() => searchInput.current?.focus()}><MagnifyingGlass /></IconButton>
          <IconButton label="Recent evidence" onClick={() => document.querySelector(".timeline-pane")?.scrollTo({ top: 0, behavior: "smooth" })}><ClockCounterClockwise /></IconButton>
          <IconButton label="Visual evidence" onClick={() => selected && setSelectedId(selected.chunkId)}><ImageSquare /></IconButton>
          <span className="rail-spacer" />
          <IconButton label="Privacy and exclusions" onClick={() => setModal("privacy")}><ShieldCheck /></IconButton>
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
            <button className="filter-button" type="button" onClick={() => setModal("privacy")}>
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
      {modal && <SettingsModal name={modal} paused={paused} onClose={() => setModal(null)} onCapture={() => capture.mutate()} />}
    </main>
  );
}

function EvidenceDetail({
  citation,
  tab,
  answer,
  searching,
  onTabChange,
  onGenerate,
}: {
  citation: Citation;
  tab: DetailTab;
  answer: string;
  searching: boolean;
  onTabChange: (tab: DetailTab) => void;
  onGenerate: () => void;
}) {
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
          <div className="tabs" role="tablist" aria-label="Evidence details">
            {(["text", "metadata", "source"] as const).map((value) => (
              <button
                key={value}
                className={tab === value ? "active" : ""}
                type="button"
                role="tab"
                aria-selected={tab === value}
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
          <small>{answer ? "Grounded in the evidence above" : "Requires an installed local GGUF model"}</small>
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

function TimelineItem({ citation, selected, onSelect }: { citation: Citation; selected: boolean; onSelect: () => void }) {
  return (
    <button className={`timeline-item ${selected ? "selected" : ""}`} type="button" onClick={onSelect}>
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

function SettingsModal({ name, paused, onClose, onCapture }: { name: Exclude<ModalName, null>; paused: boolean; onClose: () => void; onCapture: () => void }) {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ["archive-settings"], queryFn: api.archiveSettings });
  const [draft, setDraft] = useState<SettingsDraft>();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const current = draft ?? settingsDraft(settings.data);

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
  const modalError = settings.error || save.error || deleteAll.error;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title" onMouseDown={(event) => event.stopPropagation()}>
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

function dayLabel(value: string) {
  const date = new Date(value);
  const today = new Date();
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const captured = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const difference = Math.round((start - captured) / 86_400_000);
  if (difference === 0) return "Today";
  if (difference === 1) return "Yesterday";
  return "Earlier";
}

function formatTime(value: string) {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(new Date(value));
}

function formatDateTime(value: string) {
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(new Date(value));
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
