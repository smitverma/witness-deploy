export type ProxyState = {
  port: number;
  bindAddress: string;
  running: boolean;
  connectionCount: number;
  intercepting: boolean;
  certificateStatus: string;
};

export type ProjectState = {
  currentProjectPath: string | null;
  archivePath: string | null;
  name: string | null;
  temporary: boolean;
  dirty: boolean;
  autosaveIntervalSeconds: number;
};

export type RecentProject = {
  name: string;
  path: string;
  lastOpened: string;
};

export type SettingsState = {
  theme: string;
  proxyPort: number;
  proxyBindAddress: string;
  proxyIntercepting: boolean;
  proxyInterceptMode: InterceptionMode;
  interceptContentTypes: InterceptionContentType[];
  requestInterceptionRules: InterceptionRule[];
  responseInterceptionRules: InterceptionRule[];
  matchReplaceRules: MatchReplaceRule[];
  certificateDirectory: string;
  autosaveIntervalSeconds: number;
  compressionMode: "decompressAll" | "decompressText" | "passThrough";
  interceptInScopeOnly: boolean;
  upstreamTimeoutSeconds: number;
  upstreamProxy: UpstreamProxyConfig;
  workerThreads: number;
  historyLimit: number;
  fontSize: number;
  messageEditorFontSize: number;
  layoutSplitPercent: number;
  showLogsTab: boolean;
  aiEnabled: boolean;
  aiBaseUrl: string;
  aiModelName: string;
  aiRequestTimeoutSeconds: number;
  aiTurnStepLimit: number;
  aiEnterToSend: boolean;
  aiApiKeyConfigured: boolean;
  aiApiKeyPrefix: string;
  aiApiKeySuffix: string;
  shortcutModifier: "command" | "control";
};

export type UpstreamProxyConfig = {
  enabled: boolean;
  kind: "http" | "socks5";
  host: string;
  port: number;
  username: string;
  password: string;
};

export type SettingsPatch = Partial<SettingsState>;

export type AiKeyStatus = {
  configured: boolean;
  prefix: string;
  suffix: string;
  pending: boolean;
  operation?: "save" | "delete";
  error?: string;
};

export type AiRuntimeStatus = {
  ready: boolean;
  initializing: boolean;
  error?: string;
};

export type AiChatMessage = {
  role: "system" | "user" | "assistant" | "tool";
  content?: string | null;
  toolCalls?: AiToolCall[];
  toolCallId?: string;
};

export type AiToolCall = {
  id: string;
  type: "function" | string;
  function: {
    name: string;
    arguments: string;
  };
};

export type AiToolDefinition = {
  type: "function";
  function: {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
  };
};

export type AiInferenceResponse = {
  message: AiChatMessage;
  finishReason: string | null;
  usage: {
    promptTokens: number | null;
    completionTokens: number | null;
    totalTokens: number | null;
  } | null;
};

export type ForgeMessageSnapshot = AiChatMessage & {
  uiTimestamp: number;
  uiEvent?: "approved" | "trusted" | "session-trusted" | "rejected";
  uiToolName?: string;
};

export type ForgeChatSnapshot = {
  id: string;
  title: string;
  messages: ForgeMessageSnapshot[];
};

export type ForgeWorkspaceState = {
  chats: ForgeChatSnapshot[];
  activeChatId: string;
  draft: string;
};

export type AiConnectionResult = {
  ok: boolean;
  message: string;
};

export type SettingsSection = "proxy" | "display" | "storage" | "certificates" | "ai" | "miscellaneous" | "keyboard" | "updates" | "about";

export type InterceptionMode =
  | "allRequests"
  | "allResponses"
  | "requestsAndResponses"
  | "none";

export type InterceptionContentType =
  | "html"
  | "javascript"
  | "css"
  | "json"
  | "xml"
  | "images"
  | "fonts"
  | "media"
  | "documents"
  | "other";

export type InterceptionRuleMatchType =
  | "url"
  | "domain"
  | "ipAddress"
  | "protocol"
  | "fileExtension"
  | "httpMethod"
  | "contentType"
  | "request"
  | "cookieName"
  | "cookieValue"
  | "anyHeader"
  | "body"
  | "paramName"
  | "paramValue"
  | "listenerPort"
  | "inScope";

export type InterceptionRuleRelationship =
  | "matches"
  | "doesNotMatch"
  | "contains"
  | "doesNotContain"
  | "isInScope"
  | "isNotInScope";

export type InterceptionRule = {
  id: string;
  enabled: boolean;
  operator: "and" | "or";
  matchType: InterceptionRuleMatchType;
  relationship: InterceptionRuleRelationship;
  condition: string;
};

export type MatchReplaceRuleType =
  | "requestHost"
  | "requestHeader"
  | "requestBody"
  | "requestParamName"
  | "requestParamValue"
  | "responseHeader"
  | "responseBody"
  | "responseParamName"
  | "responseParamValue";

export type MatchReplaceRule = {
  id: string;
  enabled: boolean;
  location: "request" | "response"; // legacy, kept for migration
  type: MatchReplaceRuleType;
  match: string;
  replace: string;
  isRegex: boolean;
};

export type CertificateInfo = {
  certificatePath: string;
  generated: boolean;
};

export type AppSnapshot = {
  proxy: ProxyState;
  project: ProjectState;
  settings: SettingsState;
  memoryUsageBytes: number | null;
};

export type HistoryEntry = {
  sequence: number;
  id: string;
  url: string;
  method: string;
  host: string;
  path: string;
  status: number;
  length: number;
  mimeType: string;
  durationMs: number;
  timestamp: string;
  scoped: boolean;
  matchSnippet?: string;
};

export type HistoryFilter = {
  method?: string | null;
  host?: string | null;
  statusMin?: number | null;
  statusMax?: number | null;
  mimeType?: string | null;
  search?: string | null;
  inScopeOnly: boolean;
  sortBy?: string | null;
  sortDescending: boolean;
};

export type HistoryDetail = {
  entry: HistoryEntry;
  request: number[];
  response: number[];
};

export type RepeaterResponse = {
  raw: number[];
  status: number;
  durationMs: number;
  size: number;
};

export type IntruderRange = {
  from: number;
  to: number;
};

export type IntruderResult = {
  id: string;
  sequence: number;
  position: number | null;
  payload: string;
  payloads: string[];
  modifiedRanges: IntruderRange[];
  status: number | null;
  length: number;
  durationMs: number;
  error: string;
  request: Uint8Array;
  response: Uint8Array;
};

export type IntruderSession = {
  id: string;
  template: number[];
  tls: boolean;
  mode: IntruderMode;
  payloadRows: string[][];
  totalRequests: number | null;
  repeatIndefinitely: boolean;
  theme: "dark" | "light";
};

export type IntruderScan = {
  name: string;
  session: IntruderSession;
  startedAt: string;
  completedAt: string | null;
  running: boolean;
  stopped: boolean;
  stopRequested: boolean;
  currentRequestId: string | null;
  nextPayloadIndex: number;
  results: IntruderResult[];
  selectedResultId: string | null;
  error: string;
  persistenceError: string;
};

export type FuzzScanRecord = {
  id: string;
  sourceTabId: number;
  name: string;
  startedAt: string;
  completedAt: string | null;
  createdAt: string;
  updatedAt: string;
};

export type PayloadType =
  | "list"
  | "numbers"
  | "null"
  | "bruteForce"
  | "dates"
  | "characterSubstitution";

export type IntruderMode = "single" | "spread" | "map" | "combine";

export type PayloadProcessingRuleType =
  | "addPrefix"
  | "addSuffix"
  | "matchReplace"
  | "substring"
  | "reverseSubstring"
  | "modifyCase"
  | "encode"
  | "decode"
  | "hash";

export type PayloadProcessingRule = {
  id: string;
  enabled: boolean;
  type: PayloadProcessingRuleType;
  value: string;
  match: string;
  replacement: string;
  useRegex: boolean;
  caseSensitive: boolean;
  start: string;
  length: string;
  operation: string;
};

export type PayloadWarehouse = {
  type: PayloadType;
  list: {
    text: string;
    builtin: string;
    url: string;
  };
  numbers: {
    mode: "sequential" | "random";
    from: string;
    to: string;
    step: string;
    count: string;
  };
  nullPayload: {
    mode: "count" | "infinite";
    count: string;
  };
  bruteForce: {
    characterSet: string;
    minLength: string;
    maxLength: string;
  };
  dates: {
    from: string;
    to: string;
    step: string;
    unit: "days" | "weeks" | "months" | "years";
    formatMode: "preset" | "custom";
    format: string;
    customFormat: string;
  };
  characterSubstitution: {
    mappings: { from: string; to: string }[];
    caseSensitive: boolean;
    itemsText: string;
    newItem: string;
    builtin: string;
  };
  processing: PayloadProcessingRule[];
};

export type IntruderTab = {
  kind: "setup";
  id: number;
  title: string;
  groupId: string | null;
  request: Uint8Array;
  tls: boolean;
  mode: IntruderMode;
  scanName: string;
  warehouse: PayloadWarehouse;
  positionWarehouses: PayloadWarehouse[];
  selectedPayloadPosition: number;
  scans: IntruderScan[];
  activeScanId: string | null;
  error: string;
};

export type IntruderResultTab = {
  kind: "result";
  id: number;
  title: string;
  groupId: string | null;
  sourceTabId: number;
  scanId: string;
};

export type IntruderWorkspaceTab = IntruderTab | IntruderResultTab;

export type IntruderState = {
  tabs: IntruderWorkspaceTab[];
  activeTabId: number;
  nextTabId: number;
  editorDraft: { tabId: number; value: string } | null;
};

export type TabGroup = {
  id: string;
  name: string;
  color: string;
  collapsed: boolean;
};

export type DecoderWorkspaceState = {
  input: string;
  recipe: { id: number; operation: string }[];
  stageOutputs: string[];
  detected: string;
  padding: boolean;
  filter: string;
  notice: string;
  nextStepId: number;
};

export type ComparerWorkspaceState = {
  left: string;
  right: string;
  granularity: "character" | "line" | "word";
  layout: "side" | "stacked";
};

export type SiteMapWorkspaceState = {
  search: string;
  inScopeOnly: boolean;
  collapsed: string[];
  selectedEntryId: string | null;
  selectedRowKey: string | null;
};

export type OrganizerWorkspaceState = {
  selectedFolderId: string | "all" | "unfiled";
  selectedItemId: string | null;
  query: string;
  selectedTag: string;
  sort: "updated" | "created" | "title" | "host";
  draft: OrganizerItem | null;
  requestText: string;
  responseText: string;
  tagText: string;
  tagDefinitions: OrganizerTagDefinition[];
  stages: OrganizerStage[];
};

export type IdentityWorkspaceState = {
  selectedGroupId: string | null;
  selectedIdentityId: string | null;
  groupDraft: IdentityGroupInput | null;
  identityDraft: Identity | null;
};

export type ReplayTabSnapshot = {
  id: number;
  title: string;
  groupId: string | null;
  request: string;
  response: string;
  tls: boolean;
  history: string[];
  historyIndex: number;
  identityConfig: {
    groupId: string;
    groupName: string;
    identityIds: string[];
  } | null;
  identityResponses: Record<string, {
    executionId: string;
    identityId: string;
    name: string;
    color: string;
    raw: string;
    status: number | null;
    durationMs: number | null;
    size: number | null;
    error: string | null;
  }>;
  activeIdentityResponseId: string | null;
};

export type IntruderSessionSnapshot = Omit<IntruderSession, "template"> & {
  template: string;
};

export type IntruderResultSnapshot = Omit<IntruderResult, "request" | "response"> & {
  request: string;
  response: string;
};

export type IntruderScanSnapshot = Omit<IntruderScan, "session" | "results" | "running" | "stopRequested" | "currentRequestId"> & {
  session: IntruderSessionSnapshot;
  running: false;
  stopRequested: false;
  currentRequestId: null;
  results: IntruderResultSnapshot[];
};

export type IntruderTabSnapshot = Omit<IntruderTab, "request" | "scans"> & {
  request: string;
  scans: IntruderScanSnapshot[];
};

export type IntruderResultTabSnapshot = IntruderResultTab;
export type IntruderWorkspaceTabSnapshot = IntruderTabSnapshot | IntruderResultTabSnapshot;

export type IntruderStateSnapshot = Omit<IntruderState, "tabs"> & {
  tabs: IntruderWorkspaceTabSnapshot[];
};

export type WorkspaceSnapshot = {
  version: 1;
  savedAt: string;
  activeTab: string;
  use24HourClock: boolean;
  decoderInput: string;
  decoder: DecoderWorkspaceState;
  comparer: ComparerWorkspaceState;
  siteMap: SiteMapWorkspaceState;
  organizer: OrganizerWorkspaceState;
  identity: IdentityWorkspaceState;
  historyFilter: HistoryFilter;
  historyDetailId: string | null;
  historyInspectorsVisible: boolean;
  activeReplayId: number;
  nextReplayId: number;
  replayTabs: ReplayTabSnapshot[];
  replayDraft: { tabId: number; value: string } | null;
  fuzz: IntruderStateSnapshot;
  tabGroups: TabGroup[];
  settingsSection: SettingsSection;
  forge: ForgeWorkspaceState;
};

export type WitnessEvent = {
  kind: string;
  payload: Record<string, unknown>;
};

export type InterceptEntry = {
  id: string;
  kind: "request" | "response";
  raw: Uint8Array;
  requestRaw?: Uint8Array;
  url: string;
  host: string;
  method: string;
  status: number | null;
  length: number;
  receivedAt: number;
};

export type DecodeResult = {
  output: string;
  detected: string;
  steps: string[];
};

export type DiffChunk = { kind: "equal" | "insert" | "delete"; text: string };
export type DiffResult = {
  chunks: DiffChunk[];
  additions: number;
  deletions: number;
  unchanged: number;
};

export type ScopeEntry = {
  id: number;
  pattern: string;
  isRegex: boolean;
  includeSubdomains: boolean;
  isInScope: boolean;
};

export type ScopeSnapshot = { entries: ScopeEntry[] };

export type LogEntry = {
  level: "debug" | "info" | "warn" | "error" | string;
  module: string;
  message: string;
  timestamp: string;
};

export type TrafficStats = {
  requestsProcessed: number;
  totalRequestsSent: number;
  totalResponsesReceived: number;
  packetLossPercent: number;
  bytesSent: number;
  bytesReceived: number;
  volumeTransferredBytes: number;
  uptimeSeconds: number;
};

export type OrganizerFolder = {
  id: string;
  name: string;
  parentId: string | null;
  createdAt: string;
  updatedAt: string;
};

export type OrganizerItem = {
  id: string;
  title: string;
  folderId: string | null;
  stageId: string | null;
  request: number[];
  response: number[];
  tls: boolean;
  source: string;
  method: string;
  host: string;
  path: string;
  status: number | null;
  notes: string;
  tags: string[];
  createdAt: string;
  updatedAt: string;
};

export type OrganizerItemInput = {
  title: string;
  folderId: string | null;
  stageId: string | null;
  request: number[];
  response: number[];
  tls: boolean;
  source: string;
  notes: string;
  tags: string[];
};

export type OrganizerTagDefinition = {
  name: string;
  color: string;
};

export type OrganizerStage = {
  id: string;
  name: string;
  color: string;
};

export type OrganizerBundle = {
  version: number;
  folders: OrganizerFolder[];
  items: OrganizerItem[];
};

export type IdentityInjectionType = "cookie" | "header" | "queryParameter";

export type IdentityGroup = {
  id: string;
  name: string;
  description: string;
  injectionType: IdentityInjectionType;
  injectionKey: string;
};

export type IdentityGroupInput = {
  name: string;
  description: string;
  injectionType: IdentityInjectionType;
  injectionKey: string;
};

export type Identity = {
  id: string;
  groupId: string;
  name: string;
  color: string;
  notes: string;
  authValue: string;
};

export type IdentityInput = {
  groupId: string;
  name: string;
  color: string;
  notes: string;
  authValue: string;
};

export type IdentityBundle = {
  version: number;
  groups: IdentityGroup[];
  identities: Identity[];
};

export type IdentityInjectionDescriptor = {
  injectionType: IdentityInjectionType;
  injectionKey: string;
  authValue: string;
};
