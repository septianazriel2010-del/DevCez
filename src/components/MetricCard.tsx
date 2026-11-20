import type { LucideIcon } from "lucide-react";
import { ArrowUpRight } from "lucide-react";

interface MetricCardProps {
  label: string;
  value: string;
  detail: string;
  icon: LucideIcon;
  tone: "blue" | "violet" | "green" | "amber";
  progress?: number;
}

export default function MetricCard({
  label,
  value,
  detail,
  icon: Icon,
  tone,
  progress,
}: MetricCardProps) {
  return (
    <article className="metric-card panel">
      <div className="metric-topline">
        <span className="metric-label">{label}</span>
        <span className={`metric-icon ${tone}`}>
          <Icon size={18} strokeWidth={1.8} />
        </span>
      </div>
      <div className="metric-value-row">
        <strong className="metric-value">{value}</strong>
        <span className="metric-detail"><ArrowUpRight size={13} /> {detail}</span>
      </div>
      {typeof progress === "number" ? (
        <div className="progress-track" aria-label={`${label} ${Math.round(progress)} percent`}>
          <div className={`progress-fill ${tone}`} style={{ width: `${Math.min(100, Math.max(0, progress))}%` }} />
        </div>
      ) : (
        <div className="metric-bottom-note">Live system snapshot</div>
      )}
    </article>
  );
}
