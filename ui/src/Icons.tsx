export function Icon({
  name,
  className,
}: {
  name:
    | "settings"
    | "details"
    | "speaker"
    | "mute"
    | "bridge"
    | "search"
    | "chevron"
    | "arrow"
    | "close";
  className?: string | undefined;
}) {
  const paths = {
    settings:
      "M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0",
    details: "M4 5h16M4 12h16M4 19h16M8 3v4M16 10v4M10 17v4",
    speaker: "M4 9h4l5-4v14l-5-4H4z M17 8a6 6 0 0 1 0 8 M20 5a10 10 0 0 1 0 14",
    mute: "M4 9h4l5-4v14l-5-4H4z M17 9l5 6M22 9l-5 6",
    bridge: "M3 7v10M9 3v18M15 7v10M21 5v14",
    search: "M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0M15 15l6 6",
    close: "M6 6l12 12M18 6L6 18",
    chevron: "M9 5l7 7-7 7",
    arrow: "M4 12h16M14 6l6 6-6 6",
  };
  return (
    <svg
      className={className}
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
