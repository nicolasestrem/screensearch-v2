import { FormEvent, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { api, type SearchEvent } from "./api";

export function App() {
  const [query, setQuery] = useState("What was visible on screen?");
  const [events, setEvents] = useState<SearchEvent[]>([]);
  const health = useQuery({ queryKey: ["health"], queryFn: api.health, refetchInterval: 5_000 });
  const capture = useMutation({ mutationFn: api.capture });
  const search = useMutation({
    mutationFn: async (value: string) => {
      setEvents([]);
      await api.search(value, (event) => setEvents((current) => [...current, event]));
    },
  });

  const answer = useMemo(
    () => events.filter((event) => event.kind === "token").map((event) => event.text).join(""),
    [events],
  );
  const citations = events.filter((event) => event.kind === "citation");

  function submit(event: FormEvent) {
    event.preventDefault();
    if (query.trim()) void search.mutateAsync(query);
  }

  return (
    <main className="shell">
      <header>
        <div>
          <p className="eyebrow">LOCAL MEMORY, EXPLICITLY YOURS</p>
          <h1>ScreenSearch <span>V2</span></h1>
        </div>
        <div className={`status ${health.data?.status === "ready" ? "ready" : "offline"}`} role="status" aria-live="polite">
          <i /> {health.data ? `Daemon ${health.data.version}` : "Daemon offline"}
        </div>
      </header>

      <section className="capture-card">
        <div>
          <p className="label">Live index</p>
          <h2>Capture and OCR now run automatically.</h2>
          <p>This remains a diagnostics surface while the real evidence loop is verified.</p>
        </div>
        <div className="actions">
          <button className="secondary" onClick={() => capture.mutate()} disabled={capture.isPending}>Capture now</button>
          <small role="status" aria-live="polite">
            {capture.data ? `${capture.data.duplicate ? "Already indexed" : "Captured"} ${capture.data.captureId.slice(0, 8)}` : ""}
          </small>
        </div>
      </section>

      <form className="search" onSubmit={submit}>
        <label htmlFor="query">Search your screen memory</label>
        <div>
          <input id="query" value={query} onChange={(event) => setQuery(event.target.value)} />
          <button type="submit" disabled={search.isPending}>{search.isPending ? "Searching…" : "Ask"}</button>
        </div>
      </form>

      <section className="result">
        <p className="label">Visual evidence</p>
        {citations.length > 0 && (
          <div className="citations">
            {citations.map((citation) => <EvidenceCard key={citation.chunkId} citation={citation} />)}
          </div>
        )}
        {citations.length === 0 && <p className="answer empty">Search to inspect indexed screenshots and matching OCR regions.</p>}
        <div className="answer-block">
          <p className="label">Cited answer</p>
          <p className={answer ? "answer" : "answer empty"} aria-live="polite">{answer || "The local generator will only answer from evidence returned above."}</p>
        </div>
      </section>

      {(health.error || capture.error || search.error) && (
        <aside className="error" role="alert">{String(health.error || capture.error || search.error)}</aside>
      )}
    </main>
  );
}

type CitationEvent = Extract<SearchEvent, { kind: "citation" }>;

function EvidenceCard({ citation }: { citation: CitationEvent }) {
  const [imageUrl, setImageUrl] = useState<string>();
  const [assetError, setAssetError] = useState<string>();

  useEffect(() => {
    let active = true;
    let url: string | undefined;
    void api.captureAsset(citation.captureId)
      .then((asset) => {
        if (!active) return;
        url = URL.createObjectURL(new Blob([new Uint8Array(asset.content)], { type: asset.mediaType }));
        setImageUrl(url);
      })
      .catch((error: unknown) => {
        if (active) setAssetError(String(error));
      });
    return () => {
      active = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [citation.captureId]);

  return (
    <article className="evidence-card">
      <div className="screenshot">
        {imageUrl ? <img src={imageUrl} alt={`Capture from ${citation.application}`} /> : <div className="image-state">{assetError || "Loading capture…"}</div>}
        {imageUrl && citation.bounds.map((bounds, index) => (
          <span
            className="match-box"
            key={`${citation.chunkId}-${index}`}
            style={{
              left: `${bounds.x * 100}%`,
              top: `${bounds.y * 100}%`,
              width: `${bounds.width * 100}%`,
              height: `${bounds.height * 100}%`,
            }}
          />
        ))}
      </div>
      <div className="evidence-copy">
        <div className="evidence-meta">
          <strong>{citation.application}</strong>
          <time dateTime={citation.capturedAt}>{new Date(citation.capturedAt).toLocaleString()}</time>
          <span>{citation.matchKind}</span>
        </div>
        {citation.windowTitle && <p className="window-title">{citation.windowTitle}</p>}
        <p>{citation.excerpt}</p>
      </div>
    </article>
  );
}
