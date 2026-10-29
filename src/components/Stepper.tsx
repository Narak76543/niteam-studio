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
            <span className="stepper__label t-micro">
              <span className="stepper__num" aria-hidden="true">
                {i + 1}
              </span>
              {label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
