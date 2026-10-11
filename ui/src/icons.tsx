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
    <img
      className="mark"
      src="/brand/echocord-symbol.svg"
      width="23"
      height="12"
      alt=""
      aria-hidden
      draggable={false}
    />
  );
}
