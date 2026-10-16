import { TriangleAlert } from "lucide-react";
import type { AppError } from "./generated";

export function SaveStatus({
  warning,
  busy,
  retry,
  details,
}: {
  warning: AppError;
  busy: boolean;
  retry: () => void;
  details?: (() => void) | undefined;
}) {
  const reasonId = details ? "save-reason-main" : "save-reason-settings";
  return (
    <section
      className="save-notice"
      data-compact={Boolean(details)}
      aria-label="Save status"
    >
      <TriangleAlert size={16} aria-hidden />
      <div className="save-copy">
        <strong>Changes not saved</strong>
        <span id={reasonId} className={details ? "sr-only" : "save-reason"}>
          {warning.message} Changes still apply now. Next launch uses your last
          saved setup.
        </span>
      </div>
      {details && (
        <button
          type="button"
          className="btn btn-small btn-quiet"
          onClick={details}
        >
          Details
        </button>
      )}
      <button
        type="button"
        className="btn btn-small save-retry"
        aria-disabled={busy}
        aria-describedby={reasonId}
        onClick={() => {
          if (!busy) retry();
        }}
      >
        {busy ? "Saving…" : "Retry save"}
      </button>
    </section>
  );
}
