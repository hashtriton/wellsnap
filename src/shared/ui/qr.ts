// Offline QR codes for links, drawn as one SVG path.
import qrcode from "qrcode-generator";

export interface QrShape {
  /** Modules per side including the quiet zone. */
  size: number;
  /** SVG path of the dark modules, one unit per module. */
  path: string;
}

/**
 * ASCII only: links never need more, and Byte mode then matches every scanner. The quiet
 * zone is the 4 modules the spec asks for, so dark card surroundings do not confuse phone scanners.
 */
export function qrShape(text: string, quietZone = 4): QrShape {
  if (!text || !/^[\x20-\x7e]+$/.test(text)) throw new Error("QR text must be non-empty printable ASCII");
  const qr = qrcode(0, "M");
  qr.addData(text, "Byte");
  qr.make();
  const count = qr.getModuleCount();
  const segments: string[] = [];
  for (let row = 0; row < count; row++) {
    let col = 0;
    while (col < count) {
      if (!qr.isDark(row, col)) {
        col++;
        continue;
      }
      const start = col;
      while (col < count && qr.isDark(row, col)) col++;
      segments.push(`M${start + quietZone} ${row + quietZone}h${col - start}v1h-${col - start}z`);
    }
  }
  return { size: count + quietZone * 2, path: segments.join("") };
}
