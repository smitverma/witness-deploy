export type ShortcutPlatform = "macos" | "windows" | "linux";
export type ShortcutModifier = "command" | "control";

export type ShortcutTab =
  | "Proxy"
  | "History"
  | "Site Map"
  | "Replay"
  | "Fuzz"
  | "Organizer"
  | "ID+"
  | "Decoder"
  | "Comparer"
  | "Scope"
  | "AI"
  | "Logs"
  | "Settings";

export type ShortcutScope = "global" | ShortcutTab;

export type ShortcutDefinition = {
  id: string;
  action: string;
  scope: ShortcutScope;
  key: string;
  primary?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
  ctrl?: boolean;
  label: string;
  description: string;
  availability: string;
  allowInEditable?: boolean;
  destructive?: boolean;
  repeatable?: boolean;
  priority?: number;
};

const primary = (key: string, values: Omit<ShortcutDefinition, "key" | "primary">): ShortcutDefinition => ({
  ...values,
  key,
  primary: true,
});

const plain = (key: string, values: Omit<ShortcutDefinition, "key">): ShortcutDefinition => ({
  ...values,
  key,
});

// The PRD lists two same-scope collisions. This fixed-key implementation resolves
// them explicitly: Proxy Organizer transfer uses M+O, while every Fuzz transfer
// uses M+I; Replay M+F remains the canonical send action.
const globalShortcuts: ShortcutDefinition[] = [
  primary("s", {
    id: "global.project.save",
    action: "project.save",
    scope: "global",
    label: "Save project",
    description: "Save the current project and workspace.",
    availability: "A project is open; otherwise this shortcut does nothing.",
  }),
  primary(",", {
    id: "global.settings.open",
    action: "settings.open",
    scope: "global",
    label: "Open Settings",
    description: "Open Settings at the last selected section.",
    availability: "Available whenever Witness is open.",
  }),
  primary("/", {
    id: "global.shortcuts.toggle",
    action: "shortcuts.toggle",
    scope: "global",
    label: "Show shortcut reference",
    description: "Open or close the shortcut reference for the current tab.",
    availability: "Available whenever Witness is open.",
    allowInEditable: true,
  }),
  plain("Escape", {
    id: "global.escape",
    action: "transient.close",
    scope: "global",
    label: "Close or cancel the topmost transient state",
    description: "Close a modal, context menu, dialog, or safe tab state.",
    availability: "Always available unless a non-cancellable progress state owns Escape.",
    allowInEditable: true,
  }),
];

const proxyShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "proxy.forward",
    action: "proxy.forward",
    scope: "Proxy",
    label: "Forward selected interception",
    description: "Forward the selected pending interception, applying edits when it changed.",
    availability: "A pending interception is selected.",
  }),
  primary("d", {
    id: "proxy.drop",
    action: "proxy.drop",
    scope: "Proxy",
    label: "Drop selected interception",
    description: "Drop the selected pending interception.",
    availability: "A pending interception is selected.",
    destructive: true,
  }),
  primary("f", {
    id: "proxy.forwardAll",
    action: "proxy.forwardAll",
    scope: "Proxy",
    shift: true,
    label: "Forward all pending interceptions",
    description: "Forward every pending interception in the queue.",
    availability: "At least one pending interception exists.",
  }),
  primary("d", {
    id: "proxy.dropAll",
    action: "proxy.dropAll",
    scope: "Proxy",
    shift: true,
    label: "Drop all pending interceptions",
    description: "Drop the pending interception queue after confirmation.",
    availability: "At least one pending interception exists.",
    destructive: true,
  }),
  plain("ArrowUp", {
    id: "proxy.selectPrevious",
    action: "proxy.selectPrevious",
    scope: "Proxy",
    label: "Select previous pending interception",
    description: "Move to the previous pending interception.",
    availability: "The pending interception list has a selection.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "proxy.selectNext",
    action: "proxy.selectNext",
    scope: "Proxy",
    label: "Select next pending interception",
    description: "Move to the next pending interception.",
    availability: "The pending interception list has a selection.",
    repeatable: true,
  }),
  primary("r", {
    id: "proxy.sendReplay",
    action: "transfer.proxy.replay",
    scope: "Proxy",
    label: "Send request to Replay",
    description: "Create or activate a Replay request from the selected interception.",
    availability: "A request is available from the selected interception.",
  }),
  primary("i", {
    id: "proxy.sendFuzz",
    action: "transfer.proxy.fuzz",
    scope: "Proxy",
    label: "Send request to Fuzz",
    description: "Create a Fuzz tab from the selected interception request.",
    availability: "A request is available from the selected interception.",
  }),
  primary("u", {
    id: "proxy.sendDecoder",
    action: "transfer.proxy.decoder",
    scope: "Proxy",
    label: "Send request or selected text to Decoder",
    description: "Send selected text when available, otherwise the selected request.",
    availability: "A selected interception or text selection is available.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "proxy.saveOrganizer",
    action: "transfer.proxy.organizer",
    scope: "Proxy",
    label: "Save selected request to Organizer",
    description: "Save the selected interception request in Organizer.",
    availability: "A request is available from the selected interception.",
  }),
];

const historyShortcuts: ShortcutDefinition[] = [
  plain("ArrowUp", {
    id: "history.selectPrevious",
    action: "history.selectPrevious",
    scope: "History",
    label: "Select previous History entry",
    description: "Select the previous visible History entry.",
    availability: "History contains entries.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "history.selectNext",
    action: "history.selectNext",
    scope: "History",
    label: "Select next History entry",
    description: "Select the next visible History entry.",
    availability: "History contains entries.",
    repeatable: true,
  }),
  primary("c", {
    id: "history.copyRequest",
    action: "history.copyRequest",
    scope: "History",
    label: "Copy selected request",
    description: "Copy the selected History request without stealing editor text selection.",
    availability: "A History entry is selected and text is not selected in an editor.",
  }),
  primary("d", {
    id: "history.deleteEntry",
    action: "history.deleteEntry",
    scope: "History",
    label: "Delete selected History entry",
    description: "Delete the selected History entry after confirmation.",
    availability: "A History entry is selected.",
    destructive: true,
  }),
  primary("r", {
    id: "history.sendReplay",
    action: "transfer.history.replay",
    scope: "History",
    label: "Send selected request to Replay",
    description: "Create a Replay tab from the selected History request.",
    availability: "A History entry is selected.",
  }),
  primary("i", {
    id: "history.sendFuzz",
    action: "transfer.history.fuzz",
    scope: "History",
    label: "Send selected request to Fuzz",
    description: "Create a Fuzz tab from the selected History request.",
    availability: "A History entry is selected.",
  }),
  primary("u", {
    id: "history.sendDecoder",
    action: "transfer.history.decoder",
    scope: "History",
    label: "Send selected request or text to Decoder",
    description: "Send selected text when available, otherwise the selected request.",
    availability: "A History entry or text selection is available.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "history.saveOrganizer",
    action: "transfer.history.organizer",
    scope: "History",
    label: "Save selected request to Organizer",
    description: "Save the selected History request in Organizer.",
    availability: "A History entry is selected.",
  }),
];

const siteMapShortcuts: ShortcutDefinition[] = [
  plain("ArrowUp", {
    id: "siteMap.selectPrevious",
    action: "siteMap.selectPrevious",
    scope: "Site Map",
    label: "Select previous visible tree row",
    description: "Move the Site Map selection to the previous visible row.",
    availability: "Site Map has visible rows.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "siteMap.selectNext",
    action: "siteMap.selectNext",
    scope: "Site Map",
    label: "Select next visible tree row",
    description: "Move the Site Map selection to the next visible row.",
    availability: "Site Map has visible rows.",
    repeatable: true,
  }),
  plain("Enter", {
    id: "siteMap.openSelected",
    action: "siteMap.openSelected",
    scope: "Site Map",
    label: "Open selected endpoint in History",
    description: "Open the selected endpoint’s History entry.",
    availability: "An endpoint row is selected.",
  }),
  primary("e", {
    id: "siteMap.expandAll",
    action: "siteMap.expandAll",
    scope: "Site Map",
    label: "Expand all visible branches",
    description: "Expand every visible Site Map branch.",
    availability: "Site Map is active.",
  }),
  primary("e", {
    id: "siteMap.collapseAll",
    action: "siteMap.collapseAll",
    scope: "Site Map",
    shift: true,
    label: "Collapse all branches",
    description: "Collapse every Site Map branch.",
    availability: "Site Map is active.",
  }),
  primary("d", {
    id: "siteMap.deleteSelected",
    action: "siteMap.deleteSelected",
    scope: "Site Map",
    label: "Delete selected endpoint history",
    description: "Delete the selected endpoint’s History entry after confirmation.",
    availability: "An endpoint with associated History is selected.",
    destructive: true,
  }),
  primary("r", {
    id: "siteMap.sendReplay",
    action: "transfer.siteMap.replay",
    scope: "Site Map",
    label: "Send selected endpoint to Replay",
    description: "Create a Replay tab from the selected endpoint.",
    availability: "An endpoint is selected.",
  }),
  primary("i", {
    id: "siteMap.sendFuzz",
    action: "transfer.siteMap.fuzz",
    scope: "Site Map",
    label: "Send selected endpoint to Fuzz",
    description: "Create a Fuzz tab from the selected endpoint.",
    availability: "An endpoint is selected.",
  }),
  primary("u", {
    id: "siteMap.sendDecoder",
    action: "transfer.siteMap.decoder",
    scope: "Site Map",
    label: "Send selected endpoint to Decoder",
    description: "Send the selected endpoint request to Decoder.",
    availability: "An endpoint is selected.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "siteMap.saveOrganizer",
    action: "transfer.siteMap.organizer",
    scope: "Site Map",
    label: "Save selected endpoint to Organizer",
    description: "Save the selected endpoint request in Organizer.",
    availability: "An endpoint is selected.",
  }),
];

const replayShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "replay.send",
    action: "replay.send",
    scope: "Replay",
    label: "Send active Replay request",
    description: "Send the active Replay request using the same path as the Send button.",
    availability: "An active Replay request is valid and not already sending.",
  }),
  primary("f", {
    id: "replay.search",
    action: "replay.search",
    scope: "Replay",
    shift: true,
    label: "Search Replay tabs",
    description: "Search request, response, and identity-response content across Replay tabs.",
    availability: "Replay is active.",
    allowInEditable: true,
  }),
  primary("n", {
    id: "replay.newTab",
    action: "replay.newTab",
    scope: "Replay",
    label: "Create empty Replay tab",
    description: "Create a new empty Replay tab.",
    availability: "Replay is active.",
  }),
  primary("d", {
    id: "replay.duplicateTab",
    action: "replay.duplicateTab",
    scope: "Replay",
    label: "Duplicate active Replay tab",
    description: "Create a copy of the active Replay tab.",
    availability: "An active Replay tab exists.",
  }),
  primary("w", {
    id: "replay.closeTab",
    action: "replay.closeTab",
    scope: "Replay",
    label: "Close active Replay tab",
    description: "Close the active Replay tab and add it to closed-tab history.",
    availability: "An active Replay tab exists.",
  }),
  primary("w", {
    id: "replay.reopenTab",
    action: "replay.reopenTab",
    scope: "Replay",
    shift: true,
    label: "Reopen last closed Replay tab",
    description: "Restore the most recently closed Replay tab.",
    availability: "A closed Replay tab is available.",
  }),
  primary("[", {
    id: "replay.previousRequest",
    action: "replay.previousRequest",
    scope: "Replay",
    label: "Previous Replay request history",
    description: "Show the previous request in the active Replay history.",
    availability: "The active Replay tab has request history.",
  }),
  primary("]", {
    id: "replay.nextRequest",
    action: "replay.nextRequest",
    scope: "Replay",
    label: "Next Replay request history",
    description: "Show the next request in the active Replay history.",
    availability: "The active Replay tab has request history.",
  }),
  primary("i", {
    id: "replay.configureIdentities",
    action: "replay.configureIdentities",
    scope: "Replay",
    shift: true,
    label: "Configure identities",
    description: "Open ID+ configuration for the active Replay request.",
    availability: "An active Replay tab exists.",
  }),
  primary("r", {
    id: "replay.sendToReplay",
    action: "transfer.replay.replay",
    scope: "Replay",
    label: "Send active request to Replay",
    description: "Create another Replay tab with the active request.",
    availability: "An active Replay tab exists.",
  }),
  primary("i", {
    id: "replay.sendToFuzz",
    action: "transfer.replay.fuzz",
    scope: "Replay",
    label: "Send active request to Fuzz",
    description: "Create a Fuzz tab with the active request.",
    availability: "An active Replay tab exists.",
  }),
  primary("u", {
    id: "replay.sendToDecoder",
    action: "transfer.replay.decoder",
    scope: "Replay",
    label: "Send active request to Decoder",
    description: "Send the active Replay request to Decoder.",
    availability: "An active Replay tab exists.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "replay.saveToOrganizer",
    action: "transfer.replay.organizer",
    scope: "Replay",
    label: "Save active request to Organizer",
    description: "Save the active Replay request in Organizer.",
    availability: "An active Replay tab exists.",
  }),
];

const fuzzShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "fuzz.search",
    action: "fuzz.search",
    scope: "Fuzz",
    shift: true,
    label: "Search Fuzz tabs",
    description: "Search request content across Fuzz tabs.",
    availability: "Fuzz is active.",
    allowInEditable: true,
  }),
  primary("Enter", {
    id: "fuzz.launch",
    action: "fuzz.launch",
    scope: "Fuzz",
    label: "Launch or resume Fuzz run",
    description: "Launch or resume the active Fuzz configuration.",
    availability: "The active Fuzz tab has a valid, launchable configuration.",
    allowInEditable: true,
  }),
  primary(".", {
    id: "fuzz.stop",
    action: "fuzz.stop",
    scope: "Fuzz",
    label: "Stop active Fuzz run",
    description: "Stop the active Fuzz scan.",
    availability: "A Fuzz scan is running.",
  }),
  primary("r", {
    id: "fuzz.results",
    action: "fuzz.results",
    scope: "Fuzz",
    shift: true,
    label: "Show or return to Fuzz results",
    description: "Open the latest Fuzz results or return to setup.",
    availability: "A Fuzz scan exists.",
  }),
  primary("n", {
    id: "fuzz.newTab",
    action: "fuzz.newTab",
    scope: "Fuzz",
    label: "Create empty Fuzz tab",
    description: "Create a new empty Fuzz tab.",
    availability: "Fuzz is active.",
  }),
  primary("d", {
    id: "fuzz.duplicateTab",
    action: "fuzz.duplicateTab",
    scope: "Fuzz",
    label: "Duplicate active Fuzz tab",
    description: "Create a copy of the active Fuzz tab.",
    availability: "An active Fuzz tab exists.",
  }),
  primary("w", {
    id: "fuzz.closeTab",
    action: "fuzz.closeTab",
    scope: "Fuzz",
    label: "Close active Fuzz tab",
    description: "Close the active Fuzz tab and add it to closed-tab history.",
    availability: "An active Fuzz tab exists and has no running scan.",
  }),
  primary("w", {
    id: "fuzz.reopenTab",
    action: "fuzz.reopenTab",
    scope: "Fuzz",
    shift: true,
    label: "Reopen last closed Fuzz tab",
    description: "Restore the most recently closed Fuzz tab.",
    availability: "A closed Fuzz tab is available.",
  }),
  primary("r", {
    id: "fuzz.sendToReplay",
    action: "transfer.fuzz.replay",
    scope: "Fuzz",
    label: "Send selected result to Replay",
    description: "Create a Replay tab from the selected Fuzz result.",
    availability: "Results view has a selected result.",
  }),
  primary("i", {
    id: "fuzz.sendToFuzz",
    action: "transfer.fuzz.fuzz",
    scope: "Fuzz",
    label: "Send selected result to Fuzz",
    description: "Create a Fuzz tab from the selected Fuzz result.",
    availability: "Results view has a selected result.",
  }),
  primary("u", {
    id: "fuzz.sendToDecoder",
    action: "transfer.fuzz.decoder",
    scope: "Fuzz",
    label: "Send selected result to Decoder",
    description: "Send the selected result response to Decoder.",
    availability: "Results view has a selected result.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "fuzz.saveToOrganizer",
    action: "transfer.fuzz.organizer",
    scope: "Fuzz",
    label: "Save selected result to Organizer",
    description: "Save the selected Fuzz result request and response in Organizer.",
    availability: "Results view has a selected result.",
  }),
];

const organizerShortcuts: ShortcutDefinition[] = [
  plain("ArrowUp", {
    id: "organizer.selectPrevious",
    action: "organizer.selectPrevious",
    scope: "Organizer",
    label: "Select previous saved entry",
    description: "Move to the previous visible Organizer entry.",
    availability: "Organizer has visible entries.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "organizer.selectNext",
    action: "organizer.selectNext",
    scope: "Organizer",
    label: "Select next saved entry",
    description: "Move to the next visible Organizer entry.",
    availability: "Organizer has visible entries.",
    repeatable: true,
  }),
  plain("Enter", {
    id: "organizer.openSelected",
    action: "organizer.openSelected",
    scope: "Organizer",
    label: "Open selected entry",
    description: "Focus the selected Organizer entry.",
    availability: "A saved Organizer entry is selected.",
  }),
  primary("d", {
    id: "organizer.deleteSelected",
    action: "organizer.deleteSelected",
    scope: "Organizer",
    label: "Delete selected entry",
    description: "Delete the selected Organizer entry after confirmation.",
    availability: "A saved Organizer entry is selected.",
    destructive: true,
  }),
  primary("g", {
    id: "organizer.createFolder",
    action: "organizer.createFolder",
    scope: "Organizer",
    label: "Create folder",
    description: "Create a top-level Organizer folder.",
    availability: "Organizer is active.",
  }),
  primary("r", {
    id: "organizer.sendReplay",
    action: "transfer.organizer.replay",
    scope: "Organizer",
    label: "Send selected entry to Replay",
    description: "Create a Replay tab from the selected Organizer entry.",
    availability: "A saved Organizer entry is selected.",
  }),
  primary("i", {
    id: "organizer.sendFuzz",
    action: "transfer.organizer.fuzz",
    scope: "Organizer",
    label: "Send selected entry to Fuzz",
    description: "Create a Fuzz tab from the selected Organizer entry.",
    availability: "A saved Organizer entry is selected.",
  }),
  primary("u", {
    id: "organizer.sendDecoder",
    action: "transfer.organizer.decoder",
    scope: "Organizer",
    label: "Send selected entry to Decoder",
    description: "Send the selected Organizer request to Decoder.",
    availability: "A saved Organizer entry is selected.",
    allowInEditable: true,
  }),
  primary("o", {
    id: "organizer.duplicateSelected",
    action: "transfer.organizer.organizer",
    scope: "Organizer",
    label: "Send selected entry to Organizer",
    description: "Duplicate the selected Organizer entry using save semantics.",
    availability: "A saved Organizer entry is selected.",
  }),
  primary("e", {
    id: "organizer.export",
    action: "organizer.export",
    scope: "Organizer",
    shift: true,
    label: "Export Organizer JSON",
    description: "Export Organizer data as JSON.",
    availability: "Organizer has exportable data.",
  }),
  primary("i", {
    id: "organizer.import",
    action: "organizer.import",
    scope: "Organizer",
    shift: true,
    label: "Import Organizer JSON",
    description: "Open the existing Organizer import flow.",
    availability: "Organizer is active.",
  }),
];

const identityShortcuts: ShortcutDefinition[] = [
  primary("g", {
    id: "identity.createGroup",
    action: "identity.createGroup",
    scope: "ID+",
    label: "Create identity group",
    description: "Open the identity-group creation dialog.",
    availability: "ID+ is active.",
  }),
  primary("i", {
    id: "identity.createIdentity",
    action: "identity.createIdentity",
    scope: "ID+",
    label: "Create identity",
    description: "Open the identity creation dialog for the selected group.",
    availability: "An identity group is selected.",
  }),
  plain("ArrowUp", {
    id: "identity.selectPrevious",
    action: "identity.selectPrevious",
    scope: "ID+",
    label: "Select previous group or identity",
    description: "Move through the focused ID+ list.",
    availability: "The corresponding ID+ list has focus or a selection.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "identity.selectNext",
    action: "identity.selectNext",
    scope: "ID+",
    label: "Select next group or identity",
    description: "Move through the focused ID+ list.",
    availability: "The corresponding ID+ list has focus or a selection.",
    repeatable: true,
  }),
  primary("d", {
    id: "identity.deleteSelected",
    action: "identity.deleteSelected",
    scope: "ID+",
    label: "Delete selected group or identity",
    description: "Delete the focused ID+ selection after confirmation.",
    availability: "A group or identity is selected.",
    destructive: true,
  }),
  primary("e", {
    id: "identity.export",
    action: "identity.export",
    scope: "ID+",
    shift: true,
    label: "Export identity JSON",
    description: "Export identity data as JSON.",
    availability: "At least one identity group exists.",
  }),
  primary("i", {
    id: "identity.import",
    action: "identity.import",
    scope: "ID+",
    shift: true,
    label: "Import identity JSON",
    description: "Open the existing identity import flow.",
    availability: "ID+ is active.",
  }),
];

const decoderShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "decoder.focusFilter",
    action: "decoder.focusFilter",
    scope: "Decoder",
    label: "Focus operation filter",
    description: "Move focus to the Decoder operation filter.",
    availability: "The Decoder operation list is visible.",
    allowInEditable: true,
  }),
  primary("Enter", {
    id: "decoder.run",
    action: "decoder.run",
    scope: "Decoder",
    label: "Run recipe now",
    description: "Run the current Decoder recipe immediately.",
    availability: "The recipe has at least one step.",
    allowInEditable: true,
  }),
  primary("Backspace", {
    id: "decoder.clear",
    action: "decoder.clear",
    scope: "Decoder",
    label: "Clear recipe",
    description: "Clear the Decoder recipe without clearing source input.",
    availability: "The recipe has steps.",
    allowInEditable: true,
  }),
  primary("r", {
    id: "decoder.reverse",
    action: "decoder.reverse",
    scope: "Decoder",
    shift: true,
    label: "Reverse recipe",
    description: "Reverse the Decoder recipe when every step is reversible.",
    availability: "The recipe contains reversible steps.",
    allowInEditable: true,
  }),
  primary("u", {
    id: "decoder.useOutput",
    action: "decoder.useOutput",
    scope: "Decoder",
    shift: true,
    label: "Use final output as input",
    description: "Replace Decoder input with the final output.",
    availability: "Final output is available.",
    allowInEditable: true,
  }),
  primary("c", {
    id: "decoder.copyOutput",
    action: "decoder.copyOutput",
    scope: "Decoder",
    shift: true,
    label: "Copy final output",
    description: "Copy the Decoder final output.",
    availability: "Final output is non-empty.",
    allowInEditable: true,
  }),
];

const comparerShortcuts: ShortcutDefinition[] = [
  primary("l", {
    id: "comparer.focusLeft",
    action: "comparer.focusLeft",
    scope: "Comparer",
    label: "Focus left editor",
    description: "Move focus to the Comparer left editor.",
    availability: "Comparer is active.",
    allowInEditable: true,
  }),
  primary("r", {
    id: "comparer.focusRight",
    action: "comparer.focusRight",
    scope: "Comparer",
    label: "Focus right editor",
    description: "Move focus to the Comparer right editor.",
    availability: "Comparer is active.",
    allowInEditable: true,
  }),
  primary("Enter", {
    id: "comparer.recompute",
    action: "comparer.recompute",
    scope: "Comparer",
    label: "Recompute comparison",
    description: "Refresh the comparison immediately.",
    availability: "At least one comparer editor contains content.",
    allowInEditable: true,
  }),
  primary("Backspace", {
    id: "comparer.clear",
    action: "comparer.clear",
    scope: "Comparer",
    label: "Clear both inputs",
    description: "Clear both Comparer inputs.",
    availability: "Comparer is active.",
    allowInEditable: true,
  }),
  primary("\\", {
    id: "comparer.toggleLayout",
    action: "comparer.toggleLayout",
    scope: "Comparer",
    label: "Toggle comparison layout",
    description: "Switch between side-by-side and stacked comparison layout.",
    availability: "Comparer is active.",
  }),
];

const scopeShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "scope.focusFilter",
    action: "scope.focusFilter",
    scope: "Scope",
    label: "Focus Scope filter",
    description: "Move focus to the Scope filter control.",
    availability: "Scope has a filter/search control.",
    allowInEditable: true,
  }),
  primary("n", {
    id: "scope.create",
    action: "scope.create",
    scope: "Scope",
    label: "Create scope entry",
    description: "Open the Scope create flow.",
    availability: "A project is open.",
  }),
  plain("ArrowUp", {
    id: "scope.selectPrevious",
    action: "scope.selectPrevious",
    scope: "Scope",
    label: "Select previous scope entry",
    description: "Move to the previous Scope entry.",
    availability: "Scope contains entries.",
    repeatable: true,
  }),
  plain("ArrowDown", {
    id: "scope.selectNext",
    action: "scope.selectNext",
    scope: "Scope",
    label: "Select next scope entry",
    description: "Move to the next Scope entry.",
    availability: "Scope contains entries.",
    repeatable: true,
  }),
  primary("e", {
    id: "scope.edit",
    action: "scope.edit",
    scope: "Scope",
    label: "Edit selected scope entry",
    description: "Open the Scope editor for the selected entry.",
    availability: "A Scope entry is selected.",
  }),
  primary("d", {
    id: "scope.delete",
    action: "scope.delete",
    scope: "Scope",
    label: "Delete selected scope entry",
    description: "Delete the selected Scope entry after confirmation.",
    availability: "A Scope entry is selected.",
    destructive: true,
  }),
  primary("Enter", {
    id: "scope.submit",
    action: "scope.submit",
    scope: "Scope",
    label: "Submit active Scope form",
    description: "Submit the active Scope create or edit form.",
    availability: "A valid Scope form is open.",
    allowInEditable: true,
  }),
];

const forgeShortcuts: ShortcutDefinition[] = [
  primary("l", {
    id: "forge.focusComposer",
    action: "forge.focusComposer",
    scope: "AI",
    label: "Focus Forge composer",
    description: "Move focus to the Forge composer.",
    availability: "Forge is visible.",
    allowInEditable: true,
  }),
  plain("Enter", {
    id: "forge.send",
    action: "forge.send",
    scope: "AI",
    label: "Send Forge message",
    description: "Send the focused Forge composer message.",
    availability: "The Forge composer is focused, contains a sendable message, and is idle.",
    allowInEditable: true,
  }),
  plain("Escape", {
    id: "forge.stop",
    action: "forge.stop",
    scope: "AI",
    label: "Stop active Forge reply",
    description: "Stop a Forge reply after two Escape presses.",
    availability: "Forge is generating a reply; the first Escape is only an arming press.",
    allowInEditable: true,
  }),
  primary("n", {
    id: "forge.newChat",
    action: "forge.newChat",
    scope: "AI",
    label: "Create new Forge chat",
    description: "Create a new Forge chat.",
    availability: "Forge is visible.",
  }),
  primary("[", {
    id: "forge.previousChat",
    action: "forge.previousChat",
    scope: "AI",
    label: "Previous Forge chat",
    description: "Open the previous Forge chat.",
    availability: "More than one Forge chat exists.",
  }),
  primary("]", {
    id: "forge.nextChat",
    action: "forge.nextChat",
    scope: "AI",
    label: "Next Forge chat",
    description: "Open the next Forge chat.",
    availability: "More than one Forge chat exists.",
  }),
  primary("Backspace", {
    id: "forge.deleteChat",
    action: "forge.deleteChat",
    scope: "AI",
    shift: true,
    label: "Delete active Forge chat",
    description: "Delete the active Forge chat after confirmation.",
    availability: "More than one chat exists or deletion is otherwise allowed.",
    destructive: true,
  }),
];

const logsShortcuts: ShortcutDefinition[] = [
  primary("f", {
    id: "logs.focusFilter",
    action: "logs.focusFilter",
    scope: "Logs",
    label: "Focus module filter",
    description: "Move focus to the Logs module filter.",
    availability: "Logs is visible.",
    allowInEditable: true,
  }),
  primary("e", {
    id: "logs.export",
    action: "logs.export",
    scope: "Logs",
    shift: true,
    label: "Export logs",
    description: "Export the available log entries.",
    availability: "At least one log is available.",
  }),
  primary("Backspace", {
    id: "logs.clear",
    action: "logs.clear",
    scope: "Logs",
    shift: true,
    label: "Clear logs",
    description: "Clear logs after confirmation.",
    availability: "At least one log is available.",
    destructive: true,
  }),
];

const settingsShortcuts: ShortcutDefinition[] = [
  primary("ArrowUp", {
    id: "settings.previousSection",
    action: "settings.previousSection",
    scope: "Settings",
    label: "Move to previous Settings section",
    description: "Move up in the Settings sidebar.",
    availability: "Settings is active outside an editor.",
    repeatable: true,
  }),
  primary("ArrowDown", {
    id: "settings.nextSection",
    action: "settings.nextSection",
    scope: "Settings",
    label: "Move to next Settings section",
    description: "Move down in the Settings sidebar.",
    availability: "Settings is active outside an editor.",
    repeatable: true,
  }),
  plain("Enter", {
    id: "settings.openSection",
    action: "settings.openSection",
    scope: "Settings",
    label: "Open focused Settings section",
    description: "Open the focused Settings sidebar section.",
    availability: "A Settings section button has focus.",
  }),
];

export const SHORTCUT_GROUP_ORDER: ShortcutScope[] = [
  "global",
  "Proxy",
  "History",
  "Site Map",
  "Replay",
  "Fuzz",
  "Organizer",
  "ID+",
  "Decoder",
  "Comparer",
  "Scope",
  "AI",
  "Logs",
  "Settings",
];

export const SHORTCUT_GROUP_LABELS: Record<ShortcutScope, string> = {
  global: "Global",
  Proxy: "Proxy / Intercept",
  History: "History",
  "Site Map": "Site Map",
  Replay: "Replay",
  Fuzz: "Fuzz",
  Organizer: "Organizer",
  "ID+": "ID+",
  Decoder: "Decoder",
  Comparer: "Comparer",
  Scope: "Scope",
  AI: "Forge / AI",
  Logs: "Logs",
  Settings: "Settings",
};

export const SHORTCUTS: readonly ShortcutDefinition[] = [
  ...globalShortcuts,
  ...proxyShortcuts,
  ...historyShortcuts,
  ...siteMapShortcuts,
  ...replayShortcuts,
  ...fuzzShortcuts,
  ...organizerShortcuts,
  ...identityShortcuts,
  ...decoderShortcuts,
  ...comparerShortcuts,
  ...scopeShortcuts,
  ...forgeShortcuts,
  ...logsShortcuts,
  ...settingsShortcuts,
];

const punctuationByCode: Record<string, string> = {
  Slash: "/",
  Comma: ",",
  Period: ".",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  Semicolon: ";",
  Quote: "'",
};

export function detectShortcutPlatform(): ShortcutPlatform {
  if (typeof navigator !== "undefined" && /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)) {
    return "macos";
  }
  if (typeof navigator !== "undefined" && /win/i.test(navigator.platform || navigator.userAgent)) {
    return "windows";
  }
  return "linux";
}

export function defaultShortcutModifier(platform = detectShortcutPlatform()): ShortcutModifier {
  return platform === "macos" ? "command" : "control";
}

export function normalizeShortcutModifier(
  value: unknown,
  platform = detectShortcutPlatform(),
): ShortcutModifier {
  if (platform !== "macos") return "control";
  return value === "control" ? "control" : "command";
}

export function normalizeShortcutKey(key: string): string {
  if (/^[a-z]$/i.test(key)) return key.toLowerCase();
  if (/^digit[0-9]$/i.test(key)) return key.slice(-1);
  if (/^numpad[0-9]$/i.test(key)) return key.slice(-1);
  return key;
}

export function normalizeEventKey(event: Pick<KeyboardEvent, "key" | "code">): string {
  const codeKey = punctuationByCode[event.code];
  if (codeKey) return codeKey;
  const key = event.key;
  if (/^.$/.test(key) && /[a-z]/i.test(key)) return key.toLowerCase();
  if (key === "Esc") return "Escape";
  if (key === "Spacebar") return " ";
  return normalizeShortcutKey(key);
}

export function shortcutSignature(definition: ShortcutDefinition): string {
  return [
    definition.scope,
    definition.priority ?? 0,
    normalizeShortcutKey(definition.key),
    definition.primary ? "primary" : "",
    definition.shift ? "shift" : "",
    definition.alt ? "alt" : "",
    definition.meta ? "meta" : "",
    definition.ctrl ? "ctrl" : "",
  ].join("|");
}

export function validateShortcutRegistry(definitions: readonly ShortcutDefinition[] = SHORTCUTS): string[] {
  const errors: string[] = [];
  const ids = new Set<string>();
  const signatures = new Map<string, ShortcutDefinition>();
  for (const definition of definitions) {
    if (!definition.id.trim()) errors.push("Shortcut definitions require a stable id");
    if (ids.has(definition.id)) errors.push(`Duplicate shortcut id: ${definition.id}`);
    ids.add(definition.id);
    if (!definition.action.trim()) errors.push(`Shortcut ${definition.id} requires an action id`);
    if (!definition.label.trim()) errors.push(`Shortcut ${definition.id} requires a label`);
    if (!definition.scope) errors.push(`Shortcut ${definition.id} requires a scope`);
    if (!definition.key.trim()) errors.push(`Shortcut ${definition.id} requires a key`);
    if (definition.primary && (definition.meta || definition.ctrl)) {
      errors.push(`Shortcut ${definition.id} cannot combine primary with meta or ctrl`);
    }
    const signature = shortcutSignature(definition);
    const previous = signatures.get(signature);
    if (previous) {
      errors.push(`Shortcut collision in ${definition.scope}: ${previous.id} and ${definition.id}`);
    } else {
      signatures.set(signature, definition);
    }
  }
  return errors;
}

export function assertValidShortcutRegistry(definitions: readonly ShortcutDefinition[] = SHORTCUTS): void {
  const errors = validateShortcutRegistry(definitions);
  if (errors.length) throw new Error(errors.join("; "));
}

function platformModifierLabel(platform: ShortcutPlatform, modifier: ShortcutModifier): string {
  if (platform === "macos") return modifier === "command" ? "⌘" : "⌃";
  return "Ctrl";
}

function keyLabel(key: string, platform: ShortcutPlatform): string {
  const labels: Record<string, string> = platform === "macos"
    ? {
        ArrowUp: "↑",
        ArrowDown: "↓",
        ArrowLeft: "←",
        ArrowRight: "→",
        Enter: "↵",
        Backspace: "⌫",
        Escape: "Esc",
        Tab: "⇥",
        " ": "Space",
        "\\": "\\",
      }
    : {
        ArrowUp: "↑",
        ArrowDown: "↓",
        ArrowLeft: "←",
        ArrowRight: "→",
        Enter: "Enter",
        Backspace: "Backspace",
        Escape: "Esc",
        Tab: "Tab",
        " ": "Space",
        "\\": "\\",
      };
  return labels[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

export function formatShortcutParts(
  definition: ShortcutDefinition,
  platform = detectShortcutPlatform(),
  modifier = defaultShortcutModifier(platform),
): string[] {
  const parts: string[] = [];
  if (definition.primary) parts.push(platformModifierLabel(platform, modifier));
  else if (definition.meta) parts.push(platform === "macos" ? "⌘" : "Meta");
  else if (definition.ctrl) parts.push(platform === "macos" ? "⌃" : "Ctrl");
  if (definition.alt) parts.push(platform === "macos" ? "⌥" : "Alt");
  if (definition.shift) parts.push(platform === "macos" ? "⇧" : "Shift");
  const key = keyLabel(definition.key, platform);
  return [...parts, key];
}

export function formatShortcut(
  definition: ShortcutDefinition,
  platform = detectShortcutPlatform(),
  modifier = defaultShortcutModifier(platform),
): string {
  const parts = formatShortcutParts(definition, platform, modifier);
  return platform === "macos" ? parts.join("") : parts.join("+");
}

export function isEditableTarget(target: EventTarget | null): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;
  return Boolean(target.closest("input, textarea, select, [contenteditable=\"true\"], .cm-editor, .cm-scroller, .cm-content, .highlighted, .hex-view, [data-shortcut-editor]"));
}

function expectsMeta(definition: ShortcutDefinition, platform: ShortcutPlatform, modifier: ShortcutModifier): boolean {
  return definition.primary ? platform === "macos" && modifier === "command" : Boolean(definition.meta);
}

function expectsCtrl(definition: ShortcutDefinition, platform: ShortcutPlatform, modifier: ShortcutModifier): boolean {
  return definition.primary ? platform !== "macos" || modifier === "control" : Boolean(definition.ctrl);
}

export function matchesShortcut(
  event: Pick<KeyboardEvent, "key" | "code" | "shiftKey" | "altKey" | "metaKey" | "ctrlKey" | "isComposing" | "repeat">,
  definition: ShortcutDefinition,
  platform = detectShortcutPlatform(),
  modifier = defaultShortcutModifier(platform),
): boolean {
  if (event.isComposing) return false;
  if (!definition.repeatable && event.repeat) return false;
  if (normalizeEventKey(event) !== normalizeShortcutKey(definition.key)) return false;
  if (Boolean(event.shiftKey) !== Boolean(definition.shift)) return false;
  if (Boolean(event.altKey) !== Boolean(definition.alt)) return false;
  if (Boolean(event.metaKey) !== expectsMeta(definition, platform, modifier)) return false;
  if (Boolean(event.ctrlKey) !== expectsCtrl(definition, platform, modifier)) return false;
  return true;
}

export function resolveShortcut(
  event: KeyboardEvent,
  scopes: readonly ShortcutScope[],
  platform = detectShortcutPlatform(),
  modifier = defaultShortcutModifier(platform),
  definitions: readonly ShortcutDefinition[] = SHORTCUTS,
): ShortcutDefinition | null {
  const editable = isEditableTarget(event.target);
  const usePreindexed = definitions === SHORTCUTS;
  for (const scope of scopes) {
    const candidates = usePreindexed
      ? (shortcutsByScope.get(scope) ?? [])
      : [...definitions]
          .filter((candidate) => candidate.scope === scope)
          .sort((left, right) => (right.priority ?? 0) - (left.priority ?? 0));
    const definition = candidates.find((candidate) =>
      (candidate.allowInEditable || !editable) &&
      matchesShortcut(event, candidate, platform, modifier),
    );
    if (definition) return definition;
  }
  return null;
}

export function definitionsForScope(scope: ShortcutScope): ShortcutDefinition[] {
  return SHORTCUTS.filter((definition) => definition.scope === scope);
}

export function shortcutScopeLabel(scope: ShortcutScope): string {
  return SHORTCUT_GROUP_LABELS[scope];
}

// Pre-indexed by scope, sorted by priority desc once at module load.
// resolveShortcut uses this map for the default registry (no per-keypress sort).
const shortcutsByScope: Map<ShortcutScope, ShortcutDefinition[]> = (() => {
  const map = new Map<ShortcutScope, ShortcutDefinition[]>();
  for (const definition of SHORTCUTS) {
    const list = map.get(definition.scope);
    if (list) list.push(definition);
    else map.set(definition.scope, [definition]);
  }
  for (const list of map.values()) {
    list.sort((left, right) => (right.priority ?? 0) - (left.priority ?? 0));
  }
  return map;
})();
