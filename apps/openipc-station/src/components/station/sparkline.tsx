"use client";

import { useId } from "react";

interface SparklineProps {
  data: number[];
  color?: string;
  height?: number;
  min?: number;
  max?: number;
  fill?: boolean;
  strokeWidth?: number;
  className?: string;
}

/**
 * Lightweight SVG area/line chart for dense telemetry readouts.
 * No external dependency — keeps the console fast and on-theme.
 */
export function Sparkline({
  data,
  color = "var(--chart-1)",
  height = 40,
  min,
  max,
  fill = true,
  strokeWidth = 1.5,
  className,
}: SparklineProps) {
  const id = useId();
  const W = 100;
  const H = height;
  const lo = min ?? Math.min(...data, 0);
  const hi = max ?? Math.max(...data, 1);
  const range = hi - lo || 1;

  const pts = data.map((v, i) => {
    const x = (i / Math.max(1, data.length - 1)) * W;
    const y = H - ((v - lo) / range) * (H - 4) - 2;
    return [x, y] as const;
  });

  const line = pts
    .map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`)
    .join(" ");
  const area = `${line} L${W},${H} L0,${H} Z`;
  const last = pts[pts.length - 1];

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      className={className}
      style={{ width: "100%", height }}
      aria-hidden
    >
      <defs>
        <linearGradient id={`g-${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.28" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      {fill && <path d={area} fill={`url(#g-${id})`} />}
      <path
        d={line}
        fill="none"
        stroke={color}
        strokeWidth={strokeWidth}
        vectorEffect="non-scaling-stroke"
        strokeLinejoin="round"
      />
      {last && (
        <circle
          cx={last[0]}
          cy={last[1]}
          r={1.6}
          fill={color}
          vectorEffect="non-scaling-stroke"
        />
      )}
    </svg>
  );
}
