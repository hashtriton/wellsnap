// The WellSnap app mark (same drawing as src-tauri/icons/app-icon.svg), inline so windows need no network
// or asset requests.
import { useId } from "react";

export function AppMark({ size = 32, title }: { size?: number; title?: string }) {
  const id = useId().replace(/:/g, "");
  return (
    <svg
      width={size}
      height={size}
      viewBox="64 64 896 896"
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      focusable="false"
    >
      <defs>
        <linearGradient id={`${id}bg`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#3D6BFF" />
          <stop offset="1" stopColor="#2440C9" />
        </linearGradient>
        <linearGradient id={`${id}sh`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFFFFF" stopOpacity="0.18" />
          <stop offset="0.5" stopColor="#FFFFFF" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={`${id}bd`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#FF8A3D" />
          <stop offset="1" stopColor="#FF5A14" />
        </linearGradient>
      </defs>
      <rect x="64" y="64" width="896" height="896" rx="208" fill={`url(#${id}bg)`} />
      <rect x="64" y="64" width="896" height="896" rx="208" fill={`url(#${id}sh)`} />
      <g fill="none" stroke="#FFFFFF" strokeWidth="64" strokeLinecap="round" strokeLinejoin="round">
        <path d="M268 420 V318 Q268 268 318 268 H420" />
        <path d="M604 268 H706 Q756 268 756 318 V420" />
        <path d="M268 604 V706 Q268 756 318 756 H420" />
      </g>
      <circle cx="724" cy="724" r="148" fill={`url(#${id}bd)`} stroke="#FFFFFF" strokeWidth="36" />
      <path
        d="M696 668 L742 636 V812"
        fill="none"
        stroke="#FFFFFF"
        strokeWidth="44"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
