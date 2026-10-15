import type { ComponentProps, ReactNode } from "react";

type Variant = "primary" | "secondary" | "pill" | "ghost" | "danger";

interface Props extends ComponentProps<"button"> {
  variant?: Variant;
  icon?: ReactNode;
}

export function Button({ variant = "secondary", icon, children, className, type, ...rest }: Props) {
  const cls = `btn btn--${variant}${className ? ` ${className}` : ""}`;
  return (
    <button type={type ?? "button"} className={cls} {...rest}>
      {icon}
      {children}
    </button>
  );
}

/** Icon-only ghost button; `label` is required for accessibility. */
export function IconButton({
  label,
  icon,
  ...rest
}: Omit<Props, "variant" | "children"> & { label: string; icon: ReactNode }) {
  return <Button variant="ghost" aria-label={label} title={label} icon={icon} {...rest} />;
}
