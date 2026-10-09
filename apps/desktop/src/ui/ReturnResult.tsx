import { returnLines, type ReturnAxes } from "./returnControl";

/**
 * A Return's result as separate lines: surface result, session verification,
 * input readiness and, when it did not complete, the refusal in words with
 * its raw code. Only the strongest value on each axis reads as settled
 * (SPEC §4.10). Spans keep it valid inside a row button.
 */
export function ReturnResult({ result, failure, heading }: { result: ReturnAxes | null; failure: string | null; heading: string }) {
  if (failure !== null) {
    return (
      <span className="return-result" role="group" aria-label={heading} data-testid="return-result">
        <span className="return-result__heading">{heading}</span>
        <span className="return-line return-line--uncertain" data-line="request">
          <span className="return-line__label">Request failed</span> {failure}
        </span>
      </span>
    );
  }
  if (result === null) return null;
  return (
    <span className="return-result" role="group" aria-label={heading} data-testid="return-result">
      <span className="return-result__heading">{heading}</span>
      {returnLines(result).map((line) => (
        <span key={line.key} className={`return-line ${line.settled ? "return-line--ok" : "return-line--uncertain"}`} data-line={line.key} data-value={line.value} title={line.value}>
          <span className="return-line__label">{line.label}</span> {line.text}
          {line.key === "refusal" ? <span className="mono return-line__code"> {line.value}</span> : null}
        </span>
      ))}
    </span>
  );
}
