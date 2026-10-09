// Which attention items are still outstanding. Projection patches upsert a
// resolved item rather than tombstoning it, so every list, count and control
// filters on `resolvedAtMs` here instead of trusting membership (SPEC §7.2).

import type { AttentionView } from "../contracts/generated/AttentionView";

export function isOpen(item: AttentionView): boolean {
  return item.resolvedAtMs === null;
}

/** Unresolved items, in projection order. */
export function openItems(attention: readonly AttentionView[]): AttentionView[] {
  return attention.filter(isOpen);
}

/** "Needs attention": unresolved and unacknowledged. */
export function needsAttention(attention: readonly AttentionView[]): AttentionView[] {
  return attention.filter((item) => isOpen(item) && item.acknowledgedAtMs === null);
}

/** "Awaiting action": unresolved and acknowledged. */
export function awaitingAction(attention: readonly AttentionView[]): AttentionView[] {
  return attention.filter((item) => isOpen(item) && item.acknowledgedAtMs !== null);
}

export function openItemsFor(attention: readonly AttentionView[], sessionId: string): AttentionView[] {
  return attention.filter((item) => item.sessionId === sessionId && isOpen(item));
}

/**
 * The item a worker selection opens: the first one needing attention, else
 * the first one awaiting action. A resolved item is never selected.
 */
export function selectableItem(attention: readonly AttentionView[], sessionId: string): AttentionView | null {
  const open = openItemsFor(attention, sessionId);
  return open.find((item) => item.acknowledgedAtMs === null) ?? open[0] ?? null;
}

export function canAcknowledge(item: AttentionView | null | undefined): boolean {
  return item !== null && item !== undefined && isOpen(item) && item.acknowledgedAtMs === null;
}

/** Mark handled resolves an outstanding item; a resolved one has nothing left to handle. */
export function canMarkHandled(item: AttentionView | null | undefined): boolean {
  return item !== null && item !== undefined && isOpen(item);
}

const CATEGORY_TEXT: Record<AttentionView["category"], string> = {
  TURN_COMPLETE: "Turn complete",
  INPUT_REQUIRED: "Input required",
  APPROVAL_REQUIRED: "Approval required",
  ERROR: "Error",
  BLOCKED: "Blocked",
  HANDOFF_READY: "Handoff ready",
  OWNER_DECISION_REQUIRED: "Decision required",
};

export function categoryText(category: AttentionView["category"]): string {
  return CATEGORY_TEXT[category];
}

export function openCountText(count: number): string {
  return count === 0 ? "no open attention" : `${count} open attention`;
}

export function itemStateText(item: AttentionView): string {
  if (!isOpen(item)) return "Resolved";
  return item.acknowledgedAtMs === null ? "Needs attention" : "Acknowledged, awaiting action";
}
