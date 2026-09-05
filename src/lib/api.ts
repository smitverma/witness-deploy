import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  WitnessEvent,
  AppSnapshot,
  HistoryEntry,
  HistoryDetail,
  HistoryFilter,
  RepeaterResponse,
  RecentProject,
  SettingsState,
  SettingsPatch,
  CertificateInfo,
  DecodeResult,
  DiffResult,
  ScopeEntry,
  ScopeSnapshot,
  LogEntry,
  TrafficStats,
  OrganizerBundle,
  OrganizerFolder,
  OrganizerItem,
  OrganizerItemInput,
  Identity,
  IdentityBundle,
  IdentityGroup,
  IdentityGroupInput,
  IdentityInjectionDescriptor,
  IdentityInput,
  AiChatMessage,
  AiConnectionResult,
  AiInferenceResponse,
  AiKeyStatus,
  AiRuntimeStatus,
  AiToolDefinition,
  FuzzScanRecord,
} from "./types";

export const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const commands = {
  snapshot: () => invoke<AppSnapshot>("get_app_snapshot"),
  startProxy: () => invoke<void>("start_proxy"),
  stopProxy: () => invoke<void>("stop_proxy"),
  updateSettings: (patch: SettingsPatch) =>
    invoke<SettingsState>("update_settings", { patch }),
  setAiApiKey: (apiKey: string) =>
    invoke<AiKeyStatus>("set_ai_api_key", { apiKey }),
  deleteAiApiKey: () => invoke<AiKeyStatus>("delete_ai_api_key"),
  getAiApiKeyStatus: () => invoke<AiKeyStatus>("get_ai_api_key_status"),
  getAiRuntimeStatus: () => invoke<AiRuntimeStatus>("get_ai_runtime_status"),
  aiInfer: (
    messages: AiChatMessage[],
    tools: AiToolDefinition[],
    requestId?: string,
  ) =>
    invoke<AiInferenceResponse>("ai_infer", {
      request: { messages, tools, requestId: requestId ?? null },
    }),
  cancelAiInfer: (requestId: string) =>
    invoke<void>("cancel_ai_infer", { requestId }),
  testAiConnection: () => invoke<AiConnectionResult>("test_ai_connection"),
  pickProjectFile: () => invoke<string | null>("pick_project_file"),
  pickProjectSavePath: () => invoke<string | null>("pick_project_save_path"),
  createProject: (name: string, path: string) =>
    invoke<void>("create_project", { name, path }),
  openProject: (path: string) => invoke<void>("open_project", { path }),
  closeProject: () => invoke<void>("close_project"),
  getRecentProjects: () => invoke<RecentProject[]>("get_recent_projects"),
  createTemporaryProject: () => invoke<void>("create_temporary_project"),
  saveTemporaryProject: (name: string, path: string) =>
    invoke<void>("save_temporary_project", { name, path }),
  deleteProject: (path: string) => invoke<void>("delete_project", { path }),
  queryHistory: (filter: HistoryFilter, offset = 0, limit = 100) =>
    invoke<HistoryEntry[]>("query_history", { filter, offset, limit }),
  getHistoryDetail: (id: string) =>
    invoke<HistoryDetail | null>("get_history_detail", { id }),
  deleteHistoryEntry: (id: string) =>
    invoke<boolean>("delete_history_entry", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  createFuzzScan: (id: string, sourceTabId: number, name: string, startedAt: string) =>
    invoke<FuzzScanRecord>("create_fuzz_scan", { id, sourceTabId, name, startedAt }),
  completeFuzzScan: (id: string, completedAt: string) =>
    invoke<FuzzScanRecord>("complete_fuzz_scan", { id, completedAt }),
  getOrganizer: () => invoke<OrganizerBundle>("get_organizer"),
  createOrganizerFolder: (name: string, parentId: string | null) =>
    invoke<OrganizerFolder>("create_organizer_folder", { name, parentId }),
  updateOrganizerFolder: (id: string, name: string, parentId: string | null) =>
    invoke<OrganizerFolder>("update_organizer_folder", { id, name, parentId }),
  deleteOrganizerFolder: (id: string) =>
    invoke<boolean>("delete_organizer_folder", { id }),
  createOrganizerItem: (input: OrganizerItemInput) =>
    invoke<OrganizerItem>("create_organizer_item", { input }),
  updateOrganizerItem: (id: string, input: OrganizerItemInput) =>
    invoke<OrganizerItem>("update_organizer_item", { id, input }),
  deleteOrganizerItem: (id: string) =>
    invoke<boolean>("delete_organizer_item", { id }),
  importOrganizer: (bundle: OrganizerBundle) =>
    invoke<number>("import_organizer", { bundle }),
  exportOrganizerJson: () =>
    invoke<string | null>("export_organizer_json"),
  importOrganizerJson: () =>
    invoke<number | null>("import_organizer_json"),
  getIdentityGroups: () => invoke<IdentityBundle>("get_identity_groups"),
  createIdentityGroup: (input: IdentityGroupInput) =>
    invoke<IdentityGroup>("create_identity_group", { input }),
  updateIdentityGroup: (id: string, input: IdentityGroupInput) =>
    invoke<IdentityGroup>("update_identity_group", { id, input }),
  deleteIdentityGroup: (id: string) =>
    invoke<boolean>("delete_identity_group", { id }),
  createIdentity: (input: IdentityInput) =>
    invoke<Identity>("create_identity", { input }),
  updateIdentity: (id: string, input: IdentityInput) =>
    invoke<Identity>("update_identity", { id, input }),
  deleteIdentity: (id: string) =>
    invoke<boolean>("delete_identity", { id }),
  resolveIdentityInjection: (identityId: string) =>
    invoke<IdentityInjectionDescriptor>("resolve_identity_injection", { identityId }),
  importIdentities: (bundle: IdentityBundle) =>
    invoke<number>("import_identities", { bundle }),
  exportIdentitiesJson: () =>
    invoke<string | null>("export_identities_json"),
  importIdentitiesJson: () =>
    invoke<number | null>("import_identities_json"),
  resolveInterception: (id: string, action: "forward" | "drop" | "modify", raw?: Uint8Array) =>
    invoke<boolean>("resolve_interception", {
      id,
      action,
      raw: raw ? Array.from(raw) : null,
    }),
  sendRepeaterRequest: (
    requestId: string,
    raw: Uint8Array,
    tls: boolean,
    injection?: IdentityInjectionDescriptor,
  ) =>
    invoke<RepeaterResponse>("send_repeater_request", {
      requestId,
      raw: Array.from(raw),
      tls,
      injection: injection ?? null,
    }),
  cancelRepeaterRequest: (requestId: string) =>
    invoke<boolean>("cancel_repeater_request", { requestId }),
  exportCaCertificate: (destination: string) =>
    invoke<void>("export_ca_certificate", { destination }),
  generateCaCertificate: () => invoke<CertificateInfo>("generate_ca_certificate"),
  decoderTransform: (input: string, operation: string, padding = true) =>
    invoke<DecodeResult>("decoder_transform", { input, operation, padding }),
  compareText: (left: string, right: string, granularity: string) =>
    invoke<DiffResult>("compare_text", { left, right, granularity }),
  saveWorkspace: (workspace: string) =>
    invoke<void>("save_workspace", { workspace }),
  getWorkspace: () => invoke<string | null>("get_workspace"),
  saveProject: (destination?: string) =>
    invoke<void>("save_project", { destination: destination ?? null }),
  getScope: () => invoke<ScopeSnapshot>("get_scope"),
  addScopeEntry: (pattern: string, isRegex: boolean, includeSubdomains: boolean, isInScope: boolean) =>
    invoke<ScopeEntry>("add_scope_entry", { pattern, isRegex, includeSubdomains, isInScope }),
  removeScopeEntry: (id: number) => invoke<boolean>("remove_scope_entry", { id }),
  updateScopeEntry: (id: number, pattern: string, isRegex: boolean, includeSubdomains: boolean, isInScope: boolean) =>
    invoke<ScopeEntry>("update_scope_entry", { id, pattern, isRegex, includeSubdomains, isInScope }),
  importScopeEntries: (entries: string[]) =>
    invoke<ScopeSnapshot>("import_scope_entries", { entries }),
  importRequestFile: (path: string) => invoke<number[]>("import_request_file", { path }),
  openInRepeater: (raw: Uint8Array, tls: boolean) =>
    invoke<void>("open_in_repeater", { raw: Array.from(raw), tls }),
  getLogs: (limit = 50) => invoke<LogEntry[]>("get_log_entries", { limit }),
  getTrafficStats: () => invoke<TrafficStats>("get_traffic_stats"),
  clearLogs: () => invoke<void>("clear_log_entries"),
};

export const onWitnessEvent = async (
  callback: (event: WitnessEvent) => void,
): Promise<UnlistenFn> => listen<WitnessEvent>("witness-event", ({ payload }) => callback(payload));
