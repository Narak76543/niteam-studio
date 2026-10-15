/** 4 px progress bar. `value` in 0..1; omit for an indeterminate bar. */
export function ProgressBar({ value, label }: { value?: number; label: string }) {
  const indeterminate = value === undefined;
  const clamped = Math.min(1, Math.max(0, value ?? 0));
  return (
    <div
      className={indeterminate ? "progress progress--indeterminate" : "progress"}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(clamped * 100)}
    >
      <div
        className="progress__fill"
        style={indeterminate ? undefined : { width: "100%", transform: `scaleX(${clamped})` }}
      />
    </div>
  );
}
