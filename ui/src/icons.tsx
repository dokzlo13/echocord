import { AppWindow, Cable, Headphones } from "lucide-react";
import type { Glyph } from "./model";

/** Native catalogs currently supply identity text; these glyphs describe source kind. */
export function SourceGlyph({
  glyph,
  tint,
  size = 20,
}: {
  glyph: Glyph;
  tint: string;
  size?: number;
}) {
  const Icon =
    glyph === "application"
      ? AppWindow
      : glyph === "playback"
        ? Headphones
        : Cable;
  return (
    <span
      className="glyph"
      style={{ "--tint": tint } as React.CSSProperties}
      aria-hidden
    >
      <Icon size={size} strokeWidth={2} />
    </span>
  );
}
export function ServerTile({
  initials,
  tint,
  iconUrl,
}: {
  initials: string;
  tint: string;
  iconUrl?: string | null;
}) {
  return (
    <span
      className="server-tile"
      style={{ "--tint": tint } as React.CSSProperties}
      aria-hidden
    >
      {iconUrl ? <img src={iconUrl} alt="" /> : initials}
    </span>
  );
}
export function Mark() {
  return (
    <svg
      className="mark"
      viewBox="0 0 24 24"
      width="18"
      height="18"
      aria-hidden
    >
      <circle cx="7" cy="12" r="3" fill="currentColor" />
      <path
        d="M12 6.5a7.5 7.5 0 0 1 0 11"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M16 3.5a11.5 11.5 0 0 1 0 17"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity=".55"
      />
    </svg>
  );
}
