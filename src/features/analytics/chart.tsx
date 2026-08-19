import { barGeometry } from "@/features/analytics/model";

export function AnalyticsBarChart({
  amounts,
  label,
}: {
  amounts: string[];
  label: string;
}) {
  const geometry = barGeometry(amounts);
  return (
    <svg
      aria-hidden="true"
      className="h-56 w-full"
      role="presentation"
      viewBox={geometry.viewBox}
    >
      <title>{label}</title>
      <line
        className="text-border"
        stroke="currentColor"
        strokeWidth="1"
        x1="16"
        x2="624"
        y1={geometry.zeroY}
        y2={geometry.zeroY}
      />
      {geometry.bars.map((bar, index) => (
        <rect
          className="text-primary"
          fill="currentColor"
          height={bar.height}
          key={`${bar.x}:${index}`}
          width={bar.width}
          x={bar.x}
          y={bar.y}
        />
      ))}
    </svg>
  );
}
