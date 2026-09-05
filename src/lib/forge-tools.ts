import type { AiToolDefinition } from "$lib/types";

type Schema = Record<string, unknown>;

const objectSchema = (properties: Schema = {}, required: string[] = []): Schema => ({
  type: "object",
  properties,
  ...(required.length ? { required } : {}),
  additionalProperties: false,
});

const enumString = (values: readonly string[], description?: string): Schema => ({
  type: "string",
  enum: [...values],
  ...(description ? { description } : {}),
});
const boundedString = (description?: string, minLength?: number, maxLength?: number): Schema => ({
  type: "string",
  ...(description ? { description } : {}),
  ...(minLength === undefined ? {} : { minLength }),
  ...(maxLength === undefined ? {} : { maxLength }),
});
const integer = (description?: string, minimum?: number): Schema => ({
  type: "integer",
  ...(description ? { description } : {}),
  ...(minimum === undefined ? {} : { minimum }),
});
const string = (description?: string) => ({ type: "string", ...(description ? { description } : {}) });
const boolean = (description?: string) => ({ type: "boolean", ...(description ? { description } : {}) });
const stringArray = (description?: string) => ({ type: "array", items: { type: "string" }, ...(description ? { description } : {}) });
const objectArray = (description?: string) => ({ type: "array", items: { type: "object" }, ...(description ? { description } : {}) });

const interceptionDirections = ["request", "response"] as const;
const interceptionMatchTypes = ["url", "domain", "ipAddress", "protocol", "fileExtension", "httpMethod", "contentType", "request", "cookieName", "cookieValue", "anyHeader", "body", "paramName", "paramValue", "listenerPort", "inScope"] as const;
const interceptionRelationships = ["matches", "doesNotMatch", "contains", "doesNotContain", "isInScope", "isNotInScope"] as const;
const matchReplaceTypes = ["requestHost", "requestHeader", "requestBody", "requestParamName", "requestParamValue", "responseHeader", "responseBody", "responseParamName", "responseParamValue"] as const;
const processingTypes = ["addPrefix", "addSuffix", "matchReplace", "substring", "reverseSubstring", "modifyCase", "encode", "decode", "hash"] as const;
const payloadTypes = ["list", "numbers", "null", "bruteForce", "dates", "characterSubstitution"] as const;

const interceptionRuleSchema = objectSchema({
  id: boundedString("Stable rule ID.", 1, 128),
  enabled: boolean(),
  operator: enumString(["and", "or"]),
  matchType: enumString(interceptionMatchTypes),
  relationship: enumString(interceptionRelationships),
  condition: boundedString("Regex, comma-separated contains values, or empty for inScope.", 0, 512),
}, ["id", "enabled", "operator", "matchType", "relationship", "condition"]);
const interceptionRuleUpdateSchema = objectSchema({
  enabled: boolean(),
  operator: enumString(["and", "or"]),
  matchType: enumString(interceptionMatchTypes),
  relationship: enumString(interceptionRelationships),
  condition: boundedString("Regex, comma-separated contains values, or empty for inScope.", 0, 512),
}, ["enabled", "operator", "matchType", "relationship", "condition"]);
const matchReplaceRuleSchema = objectSchema({
  id: boundedString("Stable rule ID.", 1, 128),
  enabled: boolean(),
  location: enumString(["request", "response"], "Legacy direction; it must agree with type when supplied."),
  type: enumString(matchReplaceTypes),
  match: boundedString("Literal or regular expression to find.", 1, 2048),
  replace: boundedString("Replacement text.", 0, 4096),
  isRegex: boolean(),
}, ["id", "enabled", "type", "match", "replace", "isRegex"]);
const matchReplaceRuleUpdateSchema = objectSchema({
  enabled: boolean(),
  location: enumString(["request", "response"], "Legacy direction; it must agree with type when supplied."),
  type: enumString(matchReplaceTypes),
  match: boundedString("Literal or regular expression to find.", 1, 2048),
  replace: boundedString("Replacement text.", 0, 4096),
  isRegex: boolean(),
}, ["enabled", "type", "match", "replace", "isRegex"]);

const payloadProcessingRuleSchema = objectSchema({
  id: boundedString("Stable processing rule ID.", 1, 128),
  enabled: boolean(),
  type: enumString(processingTypes),
  value: string(),
  match: string(),
  replacement: string(),
  useRegex: boolean(),
  caseSensitive: boolean(),
  start: string(),
  length: string(),
  operation: string(),
}, ["id", "enabled", "type", "value", "match", "replacement", "useRegex", "caseSensitive", "start", "length", "operation"]);
const payloadProcessingRuleUpdateSchema = objectSchema({
  enabled: boolean(),
  type: enumString(processingTypes),
  value: string(),
  match: string(),
  replacement: string(),
  useRegex: boolean(),
  caseSensitive: boolean(),
  start: string(),
  length: string(),
  operation: string(),
}, ["enabled", "type", "value", "match", "replacement", "useRegex", "caseSensitive", "start", "length", "operation"]);

const payloadListSchema = objectSchema({ text: string(), builtin: string(), url: string() }, ["text", "builtin", "url"]);
const payloadNumbersSchema = objectSchema({ mode: enumString(["sequential", "random"]), from: string(), to: string(), step: string(), count: string() }, ["mode", "from", "to", "step", "count"]);
const payloadNullSchema = objectSchema({ mode: enumString(["count", "infinite"]), count: string() }, ["mode", "count"]);
const payloadBruteForceSchema = objectSchema({ characterSet: string(), minLength: string(), maxLength: string() }, ["characterSet", "minLength", "maxLength"]);
const payloadDatesSchema = objectSchema({
  from: string(),
  to: string(),
  step: string(),
  unit: enumString(["days", "weeks", "months", "years"]),
  formatMode: enumString(["preset", "custom"]),
  format: string(),
  customFormat: string(),
}, ["from", "to", "step", "unit", "formatMode", "format", "customFormat"]);
const payloadCharacterSubstitutionSchema = objectSchema({
  mappings: { type: "array", items: objectSchema({ from: string(), to: string() }, ["from", "to"]) },
  caseSensitive: boolean(),
  itemsText: string(),
  newItem: string(),
  builtin: string(),
}, ["mappings", "caseSensitive", "itemsText", "newItem", "builtin"]);
const payloadWarehouseSchema = objectSchema({
  type: enumString(payloadTypes),
  list: payloadListSchema,
  numbers: payloadNumbersSchema,
  nullPayload: payloadNullSchema,
  bruteForce: payloadBruteForceSchema,
  dates: payloadDatesSchema,
  characterSubstitution: payloadCharacterSubstitutionSchema,
  processing: { type: "array", items: payloadProcessingRuleSchema },
}, ["type", "list", "numbers", "nullPayload", "bruteForce", "dates", "characterSubstitution", "processing"]);
const fuzzGeneratorSchema = {
  oneOf: [
    objectSchema({ type: enumString(["list"]), list: payloadListSchema }, ["type", "list"]),
    objectSchema({ type: enumString(["numbers"]), numbers: payloadNumbersSchema }, ["type", "numbers"]),
    objectSchema({ type: enumString(["null"]), nullPayload: payloadNullSchema }, ["type", "nullPayload"]),
    objectSchema({ type: enumString(["bruteForce"]), bruteForce: payloadBruteForceSchema }, ["type", "bruteForce"]),
    objectSchema({ type: enumString(["dates"]), dates: payloadDatesSchema }, ["type", "dates"]),
    objectSchema({ type: enumString(["characterSubstitution"]), characterSubstitution: payloadCharacterSubstitutionSchema }, ["type", "characterSubstitution"]),
  ],
};
const replayTabDefinition = {
  type: "object",
  properties: { title: string(), request: string("Complete raw HTTP request text."), tls: boolean() },
  required: ["request"],
  additionalProperties: false,
};
const organizerItemInputSchema = objectSchema({
  title: string("Optional entry title."),
  folderId: string("Organizer folder ID. Use an empty string for Unfiled."),
  stageId: string("Project stage ID. Use an empty string for no stage."),
  request: string("Complete raw HTTP request text."),
  response: string("Complete raw HTTP response text."),
  tls: boolean("Whether the entry uses HTTPS."),
  source: string("Origin label for the saved entry."),
  notes: string("Optional Organizer notes."),
  tags: stringArray("Entry tags."),
}, ["request"]);
const organizerItemPatchSchema = objectSchema({
  title: string("Entry title."),
  folderId: string("Organizer folder ID. Use an empty string for Unfiled."),
  stageId: string("Project stage ID. Use an empty string for no stage."),
  request: string("Complete raw HTTP request text."),
  response: string("Complete raw HTTP response text."),
  tls: boolean("Whether the entry uses HTTPS."),
  source: string("Origin label for the saved entry."),
  notes: string("Organizer notes."),
  tags: stringArray("Entry tags."),
});

function tool(name: string, description: string, parameters: Schema = objectSchema()): AiToolDefinition {
  return {
    type: "function",
    function: { name, description, parameters },
  };
}

export const forgeTools: AiToolDefinition[] = [
  tool("capabilities_read", "List Forge tools and their approval requirements."),
  tool("context_read", "Read the current project, active workspace view, selected tab, and attached context."),
  tool("navigate", "Open a workspace view or application settings section.", objectSchema({ view: string("Proxy, History, Site Map, Replay, Fuzz, Organizer, ID+, Decoder, Comparer, Scope, Forge, Logs, or Settings."), section: string("Optional Settings section.") }, ["view"])),
  tool("workspace_read", "Read the current saved workspace state."),
  tool("workspace_reset", "Reset the current workspace to its initial state."),

  tool("project_state_read", "Read the current project state."),
  tool("project_list_recent", "List recently opened projects."),
  tool("project_create", "Create and open a project.", objectSchema({ name: string(), path: string() }, ["name", "path"])),
  tool("project_open", "Open an existing project.", objectSchema({ path: string() }, ["path"])),
  tool("project_close", "Close the current project."),
  tool("project_create_temporary", "Create a temporary project session."),
  tool("project_save", "Save the current project.", objectSchema({ destination: string("Optional destination path.") })),
  tool("project_delete", "Delete a project from disk.", objectSchema({ path: string() }, ["path"])),

  tool("proxy_status_read", "Read proxy status and connection information."),
  tool("proxy_start", "Start the proxy."),
  tool("proxy_stop", "Stop the proxy."),
  tool("proxy_settings_read", "Read proxy and interception settings."),
  tool("proxy_settings_update", "Update proxy and interception settings.", objectSchema({ patch: { type: "object" } }, ["patch"])),
  tool("proxy_interception_rule_create", "Add a request or response interception rule.", objectSchema({ direction: enumString(interceptionDirections), rule: interceptionRuleSchema }, ["direction", "rule"])),
  tool("proxy_interception_rule_update", "Replace the mutable fields of an interception rule.", objectSchema({ direction: enumString(interceptionDirections), id: boundedString("Existing rule ID.", 1, 128), rule: interceptionRuleUpdateSchema }, ["direction", "id", "rule"])),
  tool("proxy_interception_rule_delete", "Delete a request or response interception rule.", objectSchema({ direction: enumString(interceptionDirections), id: boundedString("Existing rule ID.", 1, 128) }, ["direction", "id"])),
  tool("proxy_interception_rule_reorder", "Move an interception rule to an explicit zero-based array index.", objectSchema({ direction: enumString(interceptionDirections), id: boundedString("Existing rule ID.", 1, 128), toIndex: integer("Zero-based destination index.", 0) }, ["direction", "id", "toIndex"])),
  tool("proxy_interception_rule_set_enabled", "Set an interception rule's enabled state.", objectSchema({ direction: enumString(interceptionDirections), id: boundedString("Existing rule ID.", 1, 128), enabled: boolean() }, ["direction", "id", "enabled"])),
  tool("proxy_match_replace_rule_create", "Add a request or response match/replace rule.", objectSchema({ rule: matchReplaceRuleSchema }, ["rule"])),
  tool("proxy_match_replace_rule_update", "Replace the mutable fields of a match/replace rule.", objectSchema({ id: boundedString("Existing rule ID.", 1, 128), rule: matchReplaceRuleUpdateSchema }, ["id", "rule"])),
  tool("proxy_match_replace_rule_delete", "Delete a match/replace rule.", objectSchema({ id: boundedString("Existing rule ID.", 1, 128) }, ["id"])),
  tool("proxy_match_replace_rule_reorder", "Move a match/replace rule to an explicit zero-based array index.", objectSchema({ id: boundedString("Existing rule ID.", 1, 128), toIndex: integer("Zero-based destination index.", 0) }, ["id", "toIndex"])),
  tool("proxy_match_replace_rule_set_enabled", "Set a match/replace rule's enabled state.", objectSchema({ id: boundedString("Existing rule ID.", 1, 128), enabled: boolean() }, ["id", "enabled"])),
  tool("intercept_queue_read", "List messages currently waiting in the interception queue."),
  tool("intercept_entry_read", "Read one intercepted message.", objectSchema({ id: string() }, ["id"])),
  tool("intercept_entry_resolve", "Forward, drop, or apply changes to an intercepted message.", objectSchema({ id: string(), action: string("forward, drop, or modify"), raw: string("Optional modified message text.") }, ["id", "action"])),

  tool("replay_tabs_list", "List every Replay tab and its current status."),
  tool("replay_tab_read", "Read a Replay tab, including request, response, history, and identity configuration.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("replay_active_tab_read", "Read the active Replay tab."),
  tool("replay_tab_create", "Create one Replay tab from scratch.", objectSchema({ title: string(), request: string("Complete raw HTTP request text."), tls: boolean() }, ["request"])),
  tool("replay_tabs_create", "Create multiple Replay tabs from complete raw HTTP requests.", objectSchema({ tabs: { type: "array", items: replayTabDefinition, description: "Replay tab definitions with request as a complete raw HTTP request string." }, requests: stringArray("Complete raw HTTP requests."), tls: boolean() }, [])),
  tool("replay_tab_duplicate", "Duplicate a Replay tab.", objectSchema({ tabId: integer(), title: string() }, ["tabId"])),
  tool("replay_tabs_duplicate", "Duplicate multiple Replay tabs.", objectSchema({ tabIds: { type: "array", items: { type: "integer" } } }, ["tabIds"])),
  tool("replay_tab_update", "Update a Replay tab title, request, or TLS mode.", objectSchema({ tabId: integer(), title: string(), request: string(), tls: boolean() }, ["tabId"])),
  tool("replay_protocol_set", "Change a Replay tab between HTTP and HTTPS.", objectSchema({ tabId: integer(), tls: boolean("true for HTTPS, false for HTTP") }, ["tabId", "tls"])),
  tool("replay_tab_delete", "Delete a Replay tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("replay_tabs_delete", "Delete multiple Replay tabs.", objectSchema({ tabIds: { type: "array", items: { type: "integer" } } }, ["tabIds"])),
  tool("replay_tab_select", "Select a Replay tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("replay_request_patch", "Apply structured edits to a Replay request. Supports replaceText, setHeader, removeHeader, setJsonValue, removeJsonValue, setQueryParameter, and removeQueryParameter.", objectSchema({ tabId: integer(), operations: objectArray("Ordered request edit operations.") }, ["tabId", "operations"])),
  tool("replay_request_history_read", "Read request versions stored for a Replay tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("replay_request_history_restore", "Restore a stored request version.", objectSchema({ tabId: integer(), index: integer() }, ["tabId", "index"])),
  tool("replay_send", "Send a Replay request and return its response.", objectSchema({ tabId: integer() }, [])),
  tool("replay_cancel", "Cancel an active Replay request.", objectSchema({ tabId: integer() }, [])),
  tool("replay_response_read", "Read the response currently shown in a Replay tab.", objectSchema({ tabId: integer() }, [])),
  tool("replay_response_clear", "Clear a Replay response.", objectSchema({ tabId: integer() }, [])),
  tool("replay_identity_configure", "Configure selected identities for a Replay tab.", objectSchema({ tabId: integer(), groupId: string(), identityIds: stringArray() }, ["tabId", "groupId", "identityIds"])),
  tool("replay_open_from_history", "Create a Replay tab from a History entry.", objectSchema({ historyId: string() }, ["historyId"])),
  tool("replay_open_from_organizer", "Create a Replay tab from an Organizer item.", objectSchema({ itemId: string() }, ["itemId"])),
  tool("replay_send_to_fuzz", "Create a Fuzz tab from a Replay tab without starting it.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("replay_send_to_decoder", "Open a Replay request or response in Decoder.", objectSchema({ tabId: integer(), part: string("request or response") }, ["tabId"])),
  tool("replay_send_to_comparer", "Add a Replay request or response to Comparer.", objectSchema({ tabId: integer(), part: string("request or response") }, ["tabId"])),
  tool("replay_save_to_organizer", "Save one Replay tab to Organizer with optional folder, stage, title, tags, and notes.", objectSchema({ tabId: integer(), part: string("request or response"), title: string("Optional Organizer entry title."), folderId: string("Optional Organizer folder ID; empty means Unfiled."), stageId: string("Optional Organizer stage ID; empty means no stage."), tags: stringArray("Optional Organizer tags."), notes: string("Optional Organizer notes.") }, ["tabId"])),

  tool("fuzz_tabs_list", "List every Fuzz tab and its scan status."),
  tool("fuzz_tab_read", "Read a Fuzz tab, request template, input configuration, and scans.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_active_tab_read", "Read the active Fuzz tab."),
  tool("fuzz_tab_create", "Create a Fuzz tab from scratch.", objectSchema({ title: string(), scanName: string("Optional default name for the next Fuzz scan."), request: string("Complete raw HTTP request text."), tls: boolean() }, ["request"])),
  tool("fuzz_tab_duplicate", "Duplicate a Fuzz tab.", objectSchema({ tabId: integer(), title: string() }, ["tabId"])),
  tool("fuzz_tab_update", "Update a Fuzz tab title, scan name, request, TLS mode, or mode.", objectSchema({ tabId: integer(), title: string(), scanName: string("Default name for the next Fuzz scan."), request: string(), tls: boolean(), mode: string("single, spread, map, or combine") }, ["tabId"])),
  tool("fuzz_tab_delete", "Delete a Fuzz tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_tab_select", "Select a Fuzz tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_positions_set", "Mark one or more exact request values as Fuzz positions.", objectSchema({ tabId: integer(), values: stringArray("Exact values, in occurrence order.") }, ["tabId", "values"])),
  tool("fuzz_positions_clear", "Remove all Fuzz positions from a tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_mode_set", "Set Fuzz mode.", objectSchema({ tabId: integer(), mode: enumString(["single", "spread", "map", "combine"], "Fuzz attack mode.") }, ["tabId", "mode"])),
  tool("fuzz_warehouse_read", "Read the complete typed input generator and processing configuration for a Fuzz tab or marked position.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0) }, ["tabId"])),
  tool("fuzz_generator_set", "Set one typed Fuzz generator while preserving the target warehouse's processing rules.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), generator: fuzzGeneratorSchema }, ["tabId", "generator"])),
  tool("fuzz_warehouse_set", "Set the complete typed input generator and processing configuration for a Fuzz tab or marked position.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), warehouse: payloadWarehouseSchema }, ["tabId", "warehouse"])),
  tool("fuzz_payload_processing_rules_read", "Read ordered payload processing rules for a Fuzz tab or marked position.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0) }, ["tabId"])),
  tool("fuzz_payload_processing_rule_create", "Add a typed payload processing rule.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), rule: payloadProcessingRuleSchema }, ["tabId", "rule"])),
  tool("fuzz_payload_processing_rule_update", "Replace the mutable fields of a payload processing rule.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), id: boundedString("Existing rule ID.", 1, 128), rule: payloadProcessingRuleUpdateSchema }, ["tabId", "id", "rule"])),
  tool("fuzz_payload_processing_rule_delete", "Delete a payload processing rule.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), id: boundedString("Existing rule ID.", 1, 128) }, ["tabId", "id"])),
  tool("fuzz_payload_processing_rule_reorder", "Move a payload processing rule to an explicit zero-based array index.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), id: boundedString("Existing rule ID.", 1, 128), toIndex: integer("Zero-based destination index.", 0) }, ["tabId", "id", "toIndex"])),
  tool("fuzz_payload_processing_rule_set_enabled", "Set a payload processing rule's enabled state.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), id: boundedString("Existing rule ID.", 1, 128), enabled: boolean() }, ["tabId", "id", "enabled"])),
  tool("fuzz_plan_preview", "Preview generated Fuzz values and request count without sending requests.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_payload_preview", "Generate and process sample payloads for one Fuzz warehouse without sending requests.", objectSchema({ tabId: integer(), positionIndex: integer("Optional zero-based marked position index.", 0), sampleLimit: integer("Number of samples to return, from 1 to 50.", 1) }, ["tabId"])),
  tool("fuzz_start", "Start a configured Fuzz scan. A scanName is required unless the tab already has one.", objectSchema({ tabId: integer(), scanName: string("Name to store for this scan and show on its result tab.") }, [])),
  tool("fuzz_stop", "Stop a running Fuzz scan.", objectSchema({ tabId: integer(), scanId: string() }, [])),
  tool("fuzz_resume", "Resume a stopped Fuzz scan.", objectSchema({ tabId: integer(), scanId: string() }, [])),
  tool("fuzz_scans_read", "List scans for a Fuzz tab.", objectSchema({ tabId: integer() }, ["tabId"])),
  tool("fuzz_results_read", "Read results from a Fuzz scan.", objectSchema({ tabId: integer(), scanId: string(), limit: integer() }, ["tabId", "scanId"])),
  tool("fuzz_result_open_in_replay", "Open a Fuzz result request in Replay.", objectSchema({ tabId: integer(), scanId: string(), resultId: string() }, ["tabId", "scanId", "resultId"])),
  tool("fuzz_result_save_to_organizer", "Save a Fuzz result to Organizer.", objectSchema({ tabId: integer(), scanId: string(), resultId: string() }, ["tabId", "scanId", "resultId"])),

  tool("history_search", "Search project History.", objectSchema({ search: string(), host: string(), method: string(), statusMin: integer(), statusMax: integer(), limit: integer() })),
  tool("history_read", "Read one History entry and its full request and response.", objectSchema({ historyId: string() }, ["historyId"])),
  tool("history_delete", "Delete a History entry.", objectSchema({ historyId: string() }, ["historyId"])),
  tool("history_clear", "Clear all project History.", objectSchema()),
  tool("history_open_in_fuzz", "Create a Fuzz tab from a History entry.", objectSchema({ historyId: string() }, ["historyId"])),
  tool("history_save_to_organizer", "Save a History entry to Organizer.", objectSchema({ historyId: string() }, ["historyId"])),
  tool("history_compare", "Open a History request or response in Comparer.", objectSchema({ historyId: string(), part: string("request or response") }, ["historyId", "part"])),
  tool("history_decode", "Open a History request or response in Decoder.", objectSchema({ historyId: string(), part: string("request or response") }, ["historyId", "part"])),
  tool("site_map_read", "Read discovered Site Map branches and endpoints.", objectSchema({ search: string(), inScopeOnly: boolean(), limit: integer() })),
  tool("site_map_endpoint_open", "Open a Site Map endpoint in Replay.", objectSchema({ historyId: string() }, ["historyId"])),

  tool("scope_read", "Read current Scope entries."),
  tool("scope_entry_add", "Add a Scope entry.", objectSchema({ pattern: string(), isRegex: boolean(), includeSubdomains: boolean(), isInScope: boolean() }, ["pattern"])),
  tool("scope_entry_update", "Update a Scope entry.", objectSchema({ id: integer(), pattern: string(), isRegex: boolean(), includeSubdomains: boolean(), isInScope: boolean() }, ["id", "pattern"])),
  tool("scope_entry_delete", "Delete a Scope entry.", objectSchema({ id: integer() }, ["id"])),
  tool("scope_entries_import", "Import Scope entries.", objectSchema({ entries: stringArray() }, ["entries"])),

  tool("organizer_read", "Read Organizer folders and items."),
  tool("organizer_state_read", "Read Organizer folders, entry summaries, project tags, stages, and the current view."),
  tool("organizer_items_list", "List and filter Organizer entries without loading full message bodies.", objectSchema({ search: string("Search title, method, host, path, notes, tags, request, or response."), folderId: string("Folder ID, all, or unfiled."), stageId: string("Stage ID or empty for entries with no stage."), tag: string("Case-insensitive tag filter."), sort: string("updated, created, title, or host."), limit: integer("Maximum entries to return, from 1 to 500.") })),
  tool("organizer_item_read", "Read one Organizer entry, including its complete request and response.", objectSchema({ id: string() }, ["id"])),
  tool("organizer_folder_create", "Create an Organizer folder.", objectSchema({ name: string(), parentId: string() }, ["name"])),
  tool("organizer_folder_update", "Rename or move an Organizer folder.", objectSchema({ id: string(), name: string(), parentId: string() }, ["id", "name"])),
  tool("organizer_folder_delete", "Delete an Organizer folder.", objectSchema({ id: string() }, ["id"])),
  tool("organizer_item_create", "Create an Organizer item from a complete request and optional metadata.", objectSchema({ input: organizerItemInputSchema }, ["input"])),
  tool("organizer_item_update", "Replace an Organizer item and its metadata.", objectSchema({ id: string(), input: organizerItemInputSchema }, ["id", "input"])),
  tool("organizer_item_patch", "Update selected Organizer entry fields while preserving all unspecified fields.", objectSchema({ id: string(), patch: organizerItemPatchSchema }, ["id", "patch"])),
  tool("organizer_item_delete", "Delete an Organizer item.", objectSchema({ id: string() }, ["id"])),
  tool("organizer_item_move", "Move an Organizer item to a folder and optionally assign a stage.", objectSchema({ id: string(), folderId: string("Folder ID; empty means Unfiled."), stageId: string("Stage ID; empty means no stage.") }, ["id"])),
  tool("organizer_view_update", "Change the Organizer folder, tag, search, or sort view.", objectSchema({ folderId: string("Folder ID, all, or unfiled."), tag: string("Tag filter; empty clears it."), query: string("Search query."), sort: string("updated, created, title, or host.") })),
  tool("organizer_tag_create", "Create a project-level Organizer tag definition.", objectSchema({ name: string(), color: string("Hex color such as #5794ef.") }, ["name"])),
  tool("organizer_tag_update", "Rename or recolor a project-level Organizer tag definition.", objectSchema({ name: string("Current tag name."), newName: string("Optional replacement tag name."), color: string("Optional hex color such as #5794ef.") }, ["name"])),
  tool("organizer_tag_delete", "Delete a project-level Organizer tag definition.", objectSchema({ name: string() }, ["name"])),
  tool("organizer_stage_create", "Create a project-level Organizer stage.", objectSchema({ name: string(), color: string("Hex color such as #5794ef.") }, ["name"])),
  tool("organizer_stage_update", "Rename or recolor an Organizer stage.", objectSchema({ id: string(), name: string(), color: string("Hex color such as #5794ef.") }, ["id"])),
  tool("organizer_stage_delete", "Delete an Organizer stage after its entries are unassigned.", objectSchema({ id: string() }, ["id"])),
  tool("organizer_stage_reorder", "Reorder an Organizer stage. Omit beforeId or use an empty string to move it to the end.", objectSchema({ id: string(), beforeId: string() }, ["id"])),
  tool("organizer_import", "Import Organizer data.", objectSchema({ bundle: { type: "object" } }, ["bundle"])),
  tool("organizer_export", "Export Organizer data."),

  tool("identity_groups_read", "Read identity groups and identities."),
  tool("identity_group_create", "Create an identity group.", objectSchema({ input: { type: "object" } }, ["input"])),
  tool("identity_group_update", "Update an identity group.", objectSchema({ id: string(), input: { type: "object" } }, ["id", "input"])),
  tool("identity_group_delete", "Delete an identity group.", objectSchema({ id: string() }, ["id"])),
  tool("identity_create", "Create an identity.", objectSchema({ input: { type: "object" } }, ["input"])),
  tool("identity_update", "Update an identity.", objectSchema({ id: string(), input: { type: "object" } }, ["id", "input"])),
  tool("identity_delete", "Delete an identity.", objectSchema({ id: string() }, ["id"])),
  tool("identity_injection_preview", "Read the resolved injection descriptor for an identity.", objectSchema({ identityId: string() }, ["identityId"])),

  tool("decoder_state_read", "Read Decoder state."),
  tool("decoder_input_set", "Set Decoder input.", objectSchema({ input: string() }, ["input"])),
  tool("decoder_transform", "Run one Decoder transformation.", objectSchema({ input: string(), operation: string(), padding: boolean() }, ["input", "operation"])),
  tool("decoder_output_use_in_replay", "Open Decoder output in Replay."),
  tool("decoder_output_use_in_fuzz", "Open Decoder output in Fuzz."),
  tool("comparer_state_read", "Read Comparer state."),
  tool("comparer_inputs_set", "Set Comparer inputs.", objectSchema({ left: string(), right: string() })),
  tool("comparer_run", "Run a comparison.", objectSchema({ granularity: string("character, line, or word") })),

  tool("settings_read", "Read application settings."),
  tool("settings_update", "Update application settings.", objectSchema({ patch: { type: "object" } }, ["patch"])),
  tool("certificate_generate", "Generate the local certificate authority."),
  tool("logs_read", "Read application logs."),
  tool("logs_clear", "Clear application logs."),
];

const replayTabUpdateTool = forgeTools.find((entry) => entry.function.name === "replay_tab_update");
if (replayTabUpdateTool) {
  replayTabUpdateTool.function.description = "Update a Replay tab title/name, TLS mode, complete request, or targeted request fields.";
  replayTabUpdateTool.function.parameters = objectSchema({
    tabId: integer(),
    title: string("New Replay tab title."),
    name: string("Alternative name for the Replay tab."),
    tls: boolean(),
    request: string("Complete raw HTTP request text for replacement."),
    host: string("Hostname or host:port to set in the request Host header."),
    operations: objectArray("Targeted request edits: replaceText, setHeader, removeHeader, setJsonValue, removeJsonValue, setQueryParameter, or removeQueryParameter."),
  }, ["tabId"]);
}

export const forgeReadToolNames = new Set([
  "capabilities_read", "context_read", "workspace_read", "project_state_read", "project_list_recent",
  "proxy_status_read", "proxy_settings_read", "intercept_queue_read", "intercept_entry_read",
  "replay_tabs_list", "replay_tab_read", "replay_active_tab_read", "replay_request_history_read", "replay_response_read",
  "fuzz_tabs_list", "fuzz_tab_read", "fuzz_active_tab_read", "fuzz_warehouse_read", "fuzz_payload_processing_rules_read", "fuzz_plan_preview", "fuzz_payload_preview", "fuzz_scans_read", "fuzz_results_read",
  "history_search", "history_read", "site_map_read", "scope_read", "organizer_read", "organizer_state_read", "organizer_items_list", "organizer_item_read", "identity_groups_read",
  "identity_injection_preview", "decoder_state_read", "comparer_state_read", "settings_read", "logs_read",
]);

export const forgeActionToolNames = new Set(forgeTools.map((entry) => entry.function.name).filter((name) => !forgeReadToolNames.has(name)));

export function forgeActionPresentation(name: string, args: Record<string, unknown>) {
  const id = typeof args.tabId === "number" ? ` #${args.tabId}` : "";
  const labels: Record<string, string> = {
    navigate: "Navigate Forge",
    workspace_reset: "Reset workspace",
    replay_tab_create: "Create Replay tab",
    replay_tabs_create: "Create Replay tabs",
    replay_tab_duplicate: "Duplicate Replay tab",
    replay_tabs_duplicate: "Duplicate Replay tabs",
    replay_tab_update: "Update Replay tab",
    replay_protocol_set: "Change Replay protocol",
    replay_tab_delete: "Delete Replay tab",
    replay_tabs_delete: "Delete Replay tabs",
    replay_tab_select: "Select Replay tab",
    replay_request_patch: "Edit Replay request",
    replay_request_history_restore: "Restore Replay request",
    replay_send: "Send Replay request",
    replay_cancel: "Cancel Replay request",
    replay_response_clear: "Clear Replay response",
    replay_identity_configure: "Configure Replay identities",
    replay_open_from_history: "Open History entry in Replay",
    replay_open_from_organizer: "Open Organizer item in Replay",
    replay_send_to_fuzz: "Open Replay tab in Fuzz",
    replay_send_to_decoder: "Open Replay data in Decoder",
    replay_send_to_comparer: "Open Replay data in Comparer",
    replay_save_to_organizer: "Save Replay data to Organizer",
    fuzz_tab_create: "Create Fuzz tab",
    fuzz_tab_duplicate: "Duplicate Fuzz tab",
    fuzz_tab_update: "Update Fuzz tab",
    fuzz_tab_delete: "Delete Fuzz tab",
    fuzz_tab_select: "Select Fuzz tab",
    fuzz_positions_set: "Set Fuzz positions",
    fuzz_positions_clear: "Clear Fuzz positions",
    fuzz_mode_set: "Set Fuzz mode",
    fuzz_start: "Start Fuzz scan",
    fuzz_stop: "Stop Fuzz scan",
    fuzz_resume: "Resume Fuzz scan",
    fuzz_result_open_in_replay: "Open Fuzz result in Replay",
    fuzz_result_save_to_organizer: "Save Fuzz result to Organizer",
    history_delete: "Delete History entry",
    history_clear: "Clear project History",
    history_open_in_fuzz: "Open History entry in Fuzz",
    history_save_to_organizer: "Save History entry to Organizer",
    history_compare: "Compare History data",
    history_decode: "Decode History data",
    site_map_endpoint_open: "Open Site Map endpoint in Replay",
    scope_entry_add: "Add Scope entry",
    scope_entry_update: "Update Scope entry",
    scope_entry_delete: "Delete Scope entry",
    scope_entries_import: "Import Scope entries",
    organizer_folder_create: "Create Organizer folder",
    organizer_folder_update: "Update Organizer folder",
    organizer_folder_delete: "Delete Organizer folder",
    organizer_view_update: "Update Organizer view",
    organizer_item_create: "Create Organizer item",
    organizer_item_update: "Update Organizer item",
    organizer_item_patch: "Edit Organizer item",
    organizer_item_delete: "Delete Organizer item",
    organizer_item_move: "Move Organizer item",
    organizer_tag_create: "Create Organizer tag",
    organizer_tag_update: "Update Organizer tag",
    organizer_tag_delete: "Delete Organizer tag",
    organizer_stage_create: "Create Organizer stage",
    organizer_stage_update: "Update Organizer stage",
    organizer_stage_delete: "Delete Organizer stage",
    organizer_stage_reorder: "Reorder Organizer stages",
    organizer_import: "Import Organizer data",
    identity_group_create: "Create identity group",
    identity_group_update: "Update identity group",
    identity_group_delete: "Delete identity group",
    identity_create: "Create identity",
    identity_update: "Update identity",
    identity_delete: "Delete identity",
    decoder_input_set: "Set Decoder input",
    decoder_transform: "Run Decoder transformation",
    decoder_output_use_in_replay: "Open Decoder output in Replay",
    decoder_output_use_in_fuzz: "Open Decoder output in Fuzz",
    comparer_inputs_set: "Set Comparer inputs",
    comparer_run: "Run comparison",
    settings_update: "Update application settings",
    certificate_generate: "Generate certificate authority",
    logs_clear: "Clear application logs",
    proxy_start: "Start proxy",
    proxy_stop: "Stop proxy",
    proxy_settings_update: "Update proxy settings",
    intercept_entry_resolve: "Resolve intercepted message",
    proxy_interception_rule_create: "Add interception rule",
    proxy_interception_rule_update: "Update interception rule",
    proxy_interception_rule_delete: "Delete interception rule",
    proxy_interception_rule_reorder: "Reorder interception rule",
    proxy_interception_rule_set_enabled: "Enable or disable interception rule",
    proxy_match_replace_rule_create: "Add match/replace rule",
    proxy_match_replace_rule_update: "Update match/replace rule",
    proxy_match_replace_rule_delete: "Delete match/replace rule",
    proxy_match_replace_rule_reorder: "Reorder match/replace rule",
    proxy_match_replace_rule_set_enabled: "Enable or disable match/replace rule",
    fuzz_generator_set: "Configure Fuzz generator",
    fuzz_warehouse_set: "Configure Fuzz inputs",
    fuzz_payload_processing_rule_create: "Add payload processing rule",
    fuzz_payload_processing_rule_update: "Update payload processing rule",
    fuzz_payload_processing_rule_delete: "Delete payload processing rule",
    fuzz_payload_processing_rule_reorder: "Reorder payload processing rule",
    fuzz_payload_processing_rule_set_enabled: "Enable or disable payload processing rule",
  };
  return {
    title: `${labels[name] ?? name}${id}`,
    detail: JSON.stringify(args).slice(0, 500),
  };
}
