import { invoke, isTauri } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { useState } from "react";

const HELP_LINKS = {
  portal: "https://discord.com/developers/applications",
  guide: "https://github.com/dokzlo13/echocord#discord-bot-setup",
} as const;

export function HelpLink({
  to,
  children,
}: {
  to: keyof typeof HELP_LINKS;
  children: string;
}) {
  const [failed, setFailed] = useState(false);
  return (
    <>
      <a
        className="help-link"
        href={HELP_LINKS[to]}
        aria-label={`${children} (opens in browser)`}
        target="_blank"
        rel="noopener noreferrer"
        onClick={async (event) => {
          if (!isTauri()) return;
          event.preventDefault();
          setFailed(false);
          try {
            await invoke("open_setup_link", { link: to });
          } catch {
            setFailed(true);
          }
        }}
      >
        {children}
        <ExternalLink size={13} aria-hidden />
      </a>
      {failed && (
        <span className="field-error help-link-error" role="alert">
          Could not open your browser. Open {HELP_LINKS[to]} manually.
        </span>
      )}
    </>
  );
}
