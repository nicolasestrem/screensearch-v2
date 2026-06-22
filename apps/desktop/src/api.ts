import { Channel, invoke } from "@tauri-apps/api/core";

export interface HealthStatus {
  version: string;
  status: string;
  capturePaused: boolean;
  captureState: "capturing" | "paused" | "backpressured";
  queueDepth: number;
  oldestPendingAgeSeconds: number;
  retryCount: number;
  deadLetterCount: number;
  queueHighWater: number;
  captureCount: number;
  assetBytes: number;
  ocrBlockCount: number;
  searchChunkCount: number;
}

export interface ArchiveSettings {
  retentionDays: number | null;
  diskBudgetBytes: number | null;
  excludedApplications: string[];
  excludedTitles: string[];
  captureCount: number;
  assetBytes: number;
}

export interface SettingsUpdateResult {
  settings: ArchiveSettings;
  capturesDeleted: number;
  assetsScheduled: number;
}

export interface DeleteResult {
  capturesDeleted: number;
  assetsScheduled: number;
}

export interface ShellSettings {
  hotkey: string;
}

export const DEFAULT_HOTKEY = "CmdOrCtrl+Shift+Space";

export interface CaptureResult {
  captureId: string;
  duplicate: boolean;
  skippedReason: string;
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

export interface GenerationModel {
  id: string;
  displayName: string;
  source: "local" | "hf" | "bundled" | string;
  repository: string;
  filename: string;
  relativePath: string;
  contentHash: string;
  byteLength: number;
  architecture: string;
  quantization: string;
  contextTokens: number;
  supportsVision: boolean;
  active: boolean;
}

export interface AutomationStatus {
  enabled: boolean;
  abortAvailable: boolean;
  abortActive: boolean;
  running: boolean;
}

export interface AutomationTarget {
  processId: number;
  windowHandle: number;
  executableName: string;
  displayTitle: string;
}

export type AutomationAction =
  | { kind: "uia_invoke"; automationId: string }
  | { kind: "uia_set_value"; automationId: string; value: string }
  | { kind: "key_chord"; modifiers: Array<"control" | "alt" | "shift">; key: string }
  | { kind: "type_text"; text: string };

export interface AutomationPlan {
  target: AutomationTarget;
  actions: AutomationAction[];
}

export interface AutomationApproval {
  approvalId: string;
  expiresAt: string;
  actionCount: number;
}

export type SearchEvent =
  | {
      kind: "plan";
      originalQuery: string;
      retrievalQuery: string;
      timezoneLabel: string;
      capturedAfter: string;
      capturedBefore: string;
      sourceTerms: string[];
    }
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
  | {
      kind: "completed";
      citationCount: number;
      answerStatus: "evidence_only" | "answered" | "no_evidence" | "model_missing" | "cancelled" | "generation_failed" | string;
      answerMessage: string;
    };

export const isTauri = "__TAURI_INTERNALS__" in window;
let previewPaused = false;
let previewHotkey = DEFAULT_HOTKEY;
let previewModels: GenerationModel[] = [];
let previewAutomationStatus: AutomationStatus = {
  enabled: false,
  abortAvailable: true,
  abortActive: false,
  running: false,
};
let previewAutomationApproval: AutomationApproval | null = null;
let previewSettings: ArchiveSettings = {
  retentionDays: null,
  diskBudgetBytes: null,
  excludedApplications: [],
  excludedTitles: [],
  captureCount: 247,
  assetBytes: 1_342_177_280,
};

function previewCitation(
  index: number,
  application: string,
  title: string,
  hoursAgo: number,
): Extract<SearchEvent, { kind: "citation" }> {
  return {
    kind: "citation",
    captureId: `preview-capture-${index}`,
    chunkId: `preview-chunk-${index}`,
    excerpt: index === 0
      ? "The real evidence loop captured seven live screenshots, completed durable OCR jobs, and returned positioned text evidence."
      : "ScreenSearch keeps screenshots, OCR text, and semantic matches on this device for private recall.",
    score: 0.94 - index * 0.07,
    capturedAt: new Date(Date.now() - hoursAgo * 3_600_000).toISOString(),
    application,
    windowTitle: title,
    width: 2560,
    height: 1080,
    bounds: [{ x: 0.64, y: 0.19, width: 0.22, height: 0.22 }],
    matchKind: index % 3 === 0 ? "hybrid" : index % 2 === 0 ? "semantic" : "lexical",
    ocrModelId: "windows-media-ocr",
    embeddingModelId: "fastembed-all-minilm-l6-v2-q-384-v1",
  };
}

const previewCitations = [
  previewCitation(0, "Codex", "Design V2 architecture", 1),
  previewCitation(1, "Microsoft Edge", "NVIDIA Nemotron model overview", 2),
  previewCitation(2, "Visual Studio Code", "screensearch-v2 — App.tsx", 5),
  previewCitation(3, "PowerShell", "ScreenSearch V2 verification", 28),
  previewCitation(4, "Microsoft Edge", "Local model documentation", 31),
  previewCitation(5, "Codex", "Truthful evidence loop", 74),
];

export const api = {
  health: () => isTauri
    ? invoke<HealthStatus>("health")
    : Promise.resolve({
      version: "0.1.0-preview",
      status: "ready",
      capturePaused: previewPaused,
      captureState: previewPaused ? "paused" : "capturing",
      queueDepth: 0,
      oldestPendingAgeSeconds: 0,
      retryCount: 0,
      deadLetterCount: 0,
      queueHighWater: 100,
      captureCount: previewSettings.captureCount,
      assetBytes: previewSettings.assetBytes,
      ocrBlockCount: 1_482,
      searchChunkCount: 1_201,
    }),
  capture: () => {
    if (!import.meta.env.DEV) {
      return Promise.reject(new Error("manual capture is available only in diagnostics builds"));
    }
    return isTauri
      ? invoke<CaptureResult>("capture_once")
      : Promise.resolve({ captureId: "preview-capture-new", duplicate: false, skippedReason: "" });
  },
  processJobs: (maximum = 10) => {
    if (!import.meta.env.DEV) {
      return Promise.reject(new Error("manual job processing is available only in diagnostics builds"));
    }
    return isTauri ? invoke<number>("process_jobs", { maximum }) : Promise.resolve(0);
  },
  captureAsset: async (captureId: string) => {
    if (isTauri) return invoke<CaptureAsset>("capture_asset", { captureId });
    const content = [...new Uint8Array(await (await fetch("/qa-capture.png")).arrayBuffer())];
    return { mediaType: "image/png", content };
  },
  setCapturePaused: async (paused: boolean) => {
    if (isTauri) return invoke<boolean>("set_capture_paused", { paused });
    previewPaused = paused;
    return paused;
  },
  archiveSettings: () => isTauri
    ? invoke<ArchiveSettings>("archive_settings")
    : Promise.resolve({ ...previewSettings }),
  updateArchiveSettings: async (settings: Omit<ArchiveSettings, "captureCount" | "assetBytes">) => {
    if (isTauri) {
      return invoke<SettingsUpdateResult>("update_archive_settings", {
        retentionDays: settings.retentionDays,
        diskBudgetBytes: settings.diskBudgetBytes,
        excludedApplications: settings.excludedApplications,
        excludedTitles: settings.excludedTitles,
      });
    }
    previewSettings = { ...previewSettings, ...settings };
    return { settings: { ...previewSettings }, capturesDeleted: 0, assetsScheduled: 0 };
  },
  deleteAllCaptures: async (confirmed: boolean) => {
    if (isTauri) return invoke<DeleteResult>("delete_all_captures", { confirmed });
    const capturesDeleted = previewSettings.captureCount;
    previewSettings = { ...previewSettings, captureCount: 0, assetBytes: 0 };
    previewPaused = true;
    return { capturesDeleted, assetsScheduled: capturesDeleted };
  },
  getShellSettings: () => isTauri
    ? invoke<ShellSettings>("get_shell_settings")
    : Promise.resolve({ hotkey: previewHotkey }),
  setShellSettings: async (hotkey: string) => {
    if (isTauri) return invoke<ShellSettings>("set_shell_settings", { hotkey });
    previewHotkey = hotkey;
    return { hotkey: previewHotkey };
  },
  search: async (
    query: string,
    generateAnswer: boolean,
    receive: (event: SearchEvent) => void,
  ) => {
    if (!isTauri) {
      receive({
        kind: "plan",
        originalQuery: query,
        retrievalQuery: query.toLowerCase().replace(/[^\da-z]+/g, " ").trim(),
        timezoneLabel: Intl.DateTimeFormat().resolvedOptions().timeZone || "Local time",
        capturedAfter: "",
        capturedBefore: "",
        sourceTerms: [],
      });
      previewCitations.forEach(receive);
      if (generateAnswer) {
        receive({ kind: "token", text: "The selected evidence shows the V2 architecture and its locally verified capture, OCR, and search pipeline." });
      }
      receive({
        kind: "completed",
        citationCount: previewCitations.length,
        answerStatus: generateAnswer ? "answered" : "evidence_only",
        answerMessage: "",
      });
      return;
    }
    const onEvent = new Channel<SearchEvent>();
    onEvent.onmessage = receive;
    await invoke<void>("search", { query, generateAnswer, onEvent });
  },
  generationModels: () => isTauri
    ? invoke<GenerationModel[]>("generation_models")
    : Promise.resolve(previewModels),
  importLocalGenerationModel: async (sourcePath: string, displayName: string, select: boolean) => {
    if (isTauri) {
      return invoke<GenerationModel>("import_local_generation_model", { sourcePath, displayName, select });
    }
    const model: GenerationModel = {
      id: displayName.toLowerCase().replace(/\W+/g, "-"),
      displayName,
      source: "local",
      repository: "",
      filename: sourcePath.split(/[\\/]/).pop() ?? "model.gguf",
      relativePath: sourcePath,
      contentHash: "preview",
      byteLength: 2_147_021_472,
      architecture: "Preview",
      quantization: "Q4_K_M",
      contextTokens: 2048,
      supportsVision: false,
      active: select,
    };
    previewModels = select ? previewModels.map((item) => ({ ...item, active: false })) : previewModels;
    previewModels = [...previewModels.filter((item) => item.id !== model.id), model];
    return model;
  },
  downloadGenerationModel: async (repository: string, filename: string, displayName: string, select: boolean) => {
    if (isTauri) {
      return invoke<GenerationModel>("download_generation_model", { repository, filename, displayName, select });
    }
    return api.importLocalGenerationModel(filename, displayName, select);
  },
  selectGenerationModel: async (modelId: string) => {
    if (isTauri) return invoke<GenerationModel>("select_generation_model", { modelId });
    previewModels = previewModels.map((model) => ({ ...model, active: model.id === modelId }));
    const selected = previewModels.find((model) => model.active);
    if (!selected) throw new Error("model not found");
    return selected;
  },
  deleteGenerationModel: async (modelId: string) => {
    if (isTauri) return invoke<boolean>("delete_generation_model", { modelId });
    const before = previewModels.length;
    previewModels = previewModels.filter((model) => model.id !== modelId || model.active);
    return previewModels.length !== before;
  },
  unloadGenerationModel: () => isTauri
    ? invoke<boolean>("unload_generation_model")
    : Promise.resolve(true),
  automationStatus: () => isTauri
    ? invoke<AutomationStatus>("automation_status")
    : Promise.resolve({ ...previewAutomationStatus }),
  setAutomationEnabled: async (enabled: boolean) => {
    if (isTauri) return invoke<AutomationStatus>("set_automation_enabled", { enabled });
    previewAutomationStatus = { ...previewAutomationStatus, enabled };
    return { ...previewAutomationStatus };
  },
  automationForegroundTarget: () => isTauri
    ? invoke<AutomationTarget>("automation_foreground_target")
    : Promise.resolve({
      processId: 4242,
      windowHandle: 9001,
      executableName: "fixture.exe",
      displayTitle: "Preview automation fixture",
    }),
  approveAutomation: async (plan: AutomationPlan) => {
    if (isTauri) return invoke<AutomationApproval>("approve_automation", { plan });
    previewAutomationApproval = {
      approvalId: `preview-${Date.now()}`,
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
      actionCount: plan.actions.length,
    };
    return previewAutomationApproval;
  },
  executeAutomation: async (approvalId: string, plan: AutomationPlan) => {
    if (isTauri) return invoke<string>("execute_automation", { approvalId, plan });
    if (!previewAutomationApproval || previewAutomationApproval.approvalId !== approvalId) {
      throw { code: "approval_missing", message: "Approval is missing or already consumed." };
    }
    previewAutomationApproval = null;
    return `preview executed ${plan.actions.length} action(s)`;
  },
  abortAutomation: async () => {
    if (isTauri) return invoke<boolean>("abort_automation");
    previewAutomationStatus = { ...previewAutomationStatus, abortActive: true };
    return true;
  },
  resetAutomationAbort: async () => {
    if (isTauri) return invoke<boolean>("reset_automation_abort");
    previewAutomationStatus = { ...previewAutomationStatus, abortActive: false };
    return false;
  },
};
