import type { InputReadiness } from "../contracts/generated/InputReadiness";
import type { SessionVerification } from "../contracts/generated/SessionVerification";
import type { SurfaceResult } from "../contracts/generated/SurfaceResult";

// The three independent route axes (SPEC §4.10). Only the strongest value on
// each axis reads as settled; everything else stays visibly uncertain.
const STRONGEST = new Set<string>(["EXACT_NATIVE_SURFACE", "CURRENT_NATIVE_REVALIDATED", "FOREGROUND_COMPATIBLE"]);

function words(value: string): string {
  return value.toLowerCase().replaceAll("_", " ");
}

function Axis({ label, value }: { label: string; value: string }) {
  const tone = STRONGEST.has(value) ? "axis axis--ok" : "axis axis--uncertain";
  return (
    <span className={tone} title={`${label}: ${value}`}>
      <span className="axis__label">{label}</span> {words(value)}
    </span>
  );
}

export function RouteAxes({
  surface,
  verification,
  readiness,
  reason,
}: {
  surface: SurfaceResult;
  verification: SessionVerification;
  readiness: InputReadiness;
  reason: string;
}) {
  return (
    <span className="axes" role="group" aria-label="Last return result">
      <Axis label="surface" value={surface} />
      <Axis label="session" value={verification} />
      <Axis label="input" value={readiness} />
      {reason !== "OK" ? <span className="axis axis--reason mono">{reason}</span> : null}
    </span>
  );
}
