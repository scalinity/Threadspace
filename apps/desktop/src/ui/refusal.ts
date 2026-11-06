// Readable text for every reason code a Return can carry (SPEC §13.2). The
// codes come from the route itself (`crates/surfaces`), the provider inventory
// it runs (`crates/provider-claude` inventory errors) and, for a session with
// no proven tab, the surface status discovery recorded (`apps/agent-macos`
// discovery, `crates/journal` identity). An unknown code is shown raw rather
// than guessed at.

const REFUSAL_TEXT: Record<string, string> = {
  OK: "Returned to the session's exact current surface.",

  // Session and binding resolution.
  SESSION_NOT_FOUND: "This session is no longer in the journal.",
  NO_NATIVE_SURFACE: "This session has no native surface to return to.",
  BINDING_NOT_FOUND: "The chosen attachment is no longer one of this session's live bindings. Choose again.",
  NO_LIVE_MAPPING: "No live attachment or current inventory row ties this session to a running process.",
  MULTIPLE_ATTACHMENTS: "This session has several live attachments. Choose one; the newest is never picked for you.",
  BINDING_STALE: "The binding changed after it was loaded, so it was not used.",
  BINDING_CHANGED_DURING_ROUTE: "The binding changed while the Return was running, so the result is not treated as current.",

  // Process revalidation.
  TARGET_GONE: "The session's process or terminal surface no longer exists.",
  PROCESS_UNREADABLE: "The session's process could not be read, so it could not be revalidated.",
  EXECUTABLE_CHANGED: "The process is running a different executable from the one bound.",
  DEVICE_CHANGED: "The process's controlling terminal changed after it was bound.",

  // Provider currentness.
  SESSION_CHANGED: "The process is now running a different session, or is not an interactive client.",
  PROVIDER_CONFLICT: "The provider's inventory lists more than one session for this process.",
  POST_FOCUS_LOOKUP_FAILED: "The tab was focused, but the provider's inventory could not confirm the session afterwards.",

  // Terminal automation and tab join.
  AUTOMATION_DENIED: "Automation of the session's terminal application is not allowed. Allow it in System Settings › Privacy & Security › Automation.",
  AUTOMATION_UNKNOWN: "Automation permission for the session's terminal application could not be determined.",
  TERMINAL_ENUMERATION_FAILED: "Terminal's windows and tabs could not be listed.",
  TERMINAL_GENERATION_CHANGED: "Terminal restarted during the Return, so its earlier tab evidence no longer applies.",
  NO_MATCHING_TAB: "No Terminal tab currently uses this session's terminal device.",
  MULTIPLE_MATCHING_TABS: "More than one Terminal tab matches this session's terminal device.",
  SURFACE_CHANGED: "The Terminal tab changed while it was being focused.",
  READBACK_FAILED: "Focus was attempted, but the front tab read back afterwards was not this session's.",
  ACTIVATION_REFUSED: "The tab was selected, but Terminal did not become the frontmost application.",

  // Ghostty process and terminal join (each Ghostty process is addressed by PID).
  GHOSTTY_GENERATION_CHANGED: "Ghostty restarted or changed its running image during the Return, so its earlier evidence no longer applies.",
  GHOSTTY_STATE_UNKNOWN: "The running Ghostty processes could not be read.",
  GHOSTTY_ENUMERATION_FAILED: "Ghostty's windows, tabs and terminals could not be listed.",
  NO_MATCHING_TERMINAL: "No Ghostty terminal currently uses this session's terminal device.",
  MULTIPLE_MATCHING_TERMINALS: "More than one Ghostty terminal matches this session's terminal device.",
  GHOSTTY_TERMINAL_CHANGED: "This session's terminal device now belongs to a different Ghostty terminal than the one bound.",
  GHOSTTY_RESTART_REQUIRED: "Ghostty was updated on disk, but the running Ghostty is the earlier version. Quit and reopen Ghostty to enable Return.",
  GHOSTTY_IMAGE_UNKNOWN: "The executable this Ghostty process runs could not be identified, so its capabilities are unknown.",
  GHOSTTY_DICTIONARY_UNAVAILABLE: "Ghostty's scripting dictionary has not been read yet, so its capabilities are unknown.",
  GHOSTTY_DICTIONARY_UNSUPPORTED: "This Ghostty version's scripting dictionary lacks the window, tab, terminal or focus terms Return needs.",
  GHOSTTY_AMBIGUOUS_PROCESSES: "Two Ghostty processes run from the same application path, so scripting could reach the wrong one. Quit one of them to enable Return.",
  GHOSTTY_TTY_UNAVAILABLE: "This Ghostty version does not report terminal TTYs, so a session cannot be matched to its pane.",

  // Budget.
  TIMEOUT: "The Return ran out of its two-second budget; a late result is never treated as verified.",

  // Provider inventory command.
  INVENTORY_TIMEOUT: "The provider's session inventory did not answer in time.",
  INVENTORY_FAILED: "The provider's session inventory command failed.",
  INVENTORY_SPAWN_FAILED: "The provider's session inventory command could not be started.",
  INVENTORY_TRUNCATED: "The provider's session inventory output was too large to trust.",
  INVENTORY_PARSE_FAILED: "The provider's session inventory output could not be read.",

  // Why discovery has no proven tab for a live session.
  SURFACE_UNPROVEN: "The session is live, but no Terminal tab has been proven for it yet.",
  SURFACE_NOT_ATTEMPTED: "No Terminal tab match has been attempted for this session yet.",
  UNSUPPORTED_KIND: "This session is not an interactive terminal client.",
  MULTIPLE_TERMINAL_PROCESSES: "More than one Terminal process is running, so the tab could not be attributed.",
  TERMINAL_NOT_RUNNING: "Terminal is not running.",
  TERMINAL_STATE_UNKNOWN: "Whether Terminal is running could not be determined.",
  AUTOMATION_NOT_AUTHORIZED: "Terminal automation is not allowed, so the session's tab could not be proven.",
  AUTOMATION_STATE_UNKNOWN: "Terminal automation permission could not be determined, so the session's tab could not be proven.",
  PROCESS_CHANGED_DURING_SURFACE_JOIN: "The process changed while its terminal surface was being matched.",
  NO_SURFACE_APPLICATION: "Neither Ghostty nor Terminal is running.",
  NO_MATCHING_SURFACE: "No Ghostty terminal or Terminal tab currently uses this session's terminal device.",
  MULTIPLE_MATCHING_SURFACES: "More than one terminal surface matches this session's terminal device.",
  GHOSTTY_AUTOMATION_NOT_AUTHORIZED: "Ghostty automation is not allowed, so the session's pane could not be proven. Use Allow Ghostty access.",
  GHOSTTY_AUTOMATION_STATE_UNKNOWN: "Ghostty automation permission could not be determined, so the session's pane could not be proven.",
};

/** Every code with dedicated text. */
export const KNOWN_REFUSAL_CODES: readonly string[] = Object.keys(REFUSAL_TEXT);

export function refusalText(code: string): string {
  return Object.hasOwn(REFUSAL_TEXT, code) ? (REFUSAL_TEXT[code] as string) : `Return refused with an unrecognized code: ${code}.`;
}
