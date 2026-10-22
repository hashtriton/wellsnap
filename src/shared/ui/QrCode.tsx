import { useMemo } from "react";
import { qrShape } from "./qr";

/** Offline QR code; always dark on white so phone cameras read it in both themes. */
export function QrCode({ text, size = 148, label }: { text: string; size?: number; label: string }) {
  const shape = useMemo(() => qrShape(text), [text]);
  return (
    <svg
      className="qr"
      width={size}
      height={size}
      viewBox={`0 0 ${shape.size} ${shape.size}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label={label}
    >
      <rect width={shape.size} height={shape.size} fill="#ffffff" />
      <path d={shape.path} fill="#111318" />
    </svg>
  );
}
