import type { Icon } from "@tabler/icons-react";
import type { ReactNode } from "react";

interface EmptyStateProps {
  icon: Icon;
  title: string;
  description: string;
  action?: ReactNode;
  compact?: boolean;
}

export function EmptyState({
  icon: StateIcon,
  title,
  description,
  action,
  compact,
}: EmptyStateProps) {
  return (
    <section className={compact ? "empty-state empty-state--compact" : "empty-state"}>
      <div className="empty-state__icon" aria-hidden="true">
        <StateIcon size={compact ? 22 : 30} stroke={1.6} />
      </div>
      <div className="empty-state__copy">
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
      {action}
    </section>
  );
}
