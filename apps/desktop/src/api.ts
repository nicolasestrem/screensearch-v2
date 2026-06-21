import { Channel, invoke } from "@tauri-apps/api/core";

export interface HealthStatus {
  version: string;
  status: string;
}

export interface CaptureResult {
  captureId: string;
  duplicate: boolean;
}

export interface NormalizedRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CaptureAsset {
  mediaType: string;
  content: number[];
}

export type SearchEvent =
  | {
      kind: "citation";
      captureId: string;
      chunkId: string;
      excerpt: string;
      score: number;
      capturedAt: string;
      application: string;
      windowTitle: string;
      width: number;
      height: number;
      bounds: NormalizedRect[];
      matchKind: "lexical" | "semantic" | "hybrid";
      ocrModelId: string;
      embeddingModelId: string;
    }
  | { kind: "token"; text: string }
  | { kind: "completed"; citationCount: number };

export const api = {
  health: () => invoke<HealthStatus>("health"),
  capture: () => invoke<CaptureResult>("capture_once"),
  processJobs: (maximum = 10) => invoke<number>("process_jobs", { maximum }),
  captureAsset: (captureId: string) => invoke<CaptureAsset>("capture_asset", { captureId }),
  search: async (query: string, receive: (event: SearchEvent) => void) => {
    const onEvent = new Channel<SearchEvent>();
    onEvent.onmessage = receive;
    await invoke<void>("search", { query, onEvent });
  },
};
