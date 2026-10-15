/** Thin 3-step stepper for the New Project wizard. */
export function Stepper({ steps, current }: { steps: string[]; current: number }) {
  return (
    <ol className="stepper" aria-label="Steps">
      {steps.map((label, i) => {
        const state = i < current ? "done" : i === current ? "active" : "todo";
        return (
          <li
            key={label}
            className={`stepper__step stepper__step--${state}`}
            aria-current={i === current ? "step" : undefined}
          >
            <span className="stepper__bar" />
            <span className="t-micro">
              {i + 1}. {label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
