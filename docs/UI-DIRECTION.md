# First-build interface direction

The pinned frontend-design and playwright-cli skills were evaluated on the existing fixture UI before production UI assignment. Both were usable with project constraints. The browser skill ran through the locked Playwright 1.64.0 runtime using isolated Chromium; no alpha/global CLI or personal browser profile was required. CLI evaluation needed an explicit bundled-Chromium configuration and a short private temporary path for browser IPC sockets. Screenshots and keyboard evidence remain under ignored `.trash/`.

## Adopted layout and tokens

Use a compact, left-aligned route form above a persistent session-control area. Recording device and pre-output meter come first, then server and voice channel, then concise bot/discovery status. Keep connection action, Discord volume, and Mute output visible at the supported minimum window size; allow the route section to scroll independently if necessary. Default client size is approximately 480 by 600 logical pixels, subject to native review.

Palette: window `#181818`, surfaces `#242424`, text `#F4F4F4`, secondary text `#BDBDBD`, control border `#737373`, action/focus accent `#72CBD3`. Accent buttons use dark text, approximately 9.47:1 contrast with the proposed window color. Verify disabled, focus, hover, and error states separately. Use Segoe UI Variable with local system fallbacks; 14/20 body, 12/16 secondary, 16/22 semibold status/title. Start with 16px outer padding, 8–16px gaps, 36px controls, restrained corners, and visible 2px focus cues.

Remove repeated headings/tagline/tutorial copy. Keep routing help available where useful, without consuming the main session area. No hero, remote fonts, decorative dashboards, or unrelated player controls.

## State and interaction rules

- Startup is disconnected with capture stopped. Configuration and catalog discovery are independent of voice joining.
- Display connection, capture health, and output state separately. Connected remains visible when muted or when input fails.
- Use Output enabled for control state; Sending requires actual sender evidence and never implies listener audibility.
- Use a stable Mute output toggle with `aria-pressed`; announce significant state changes, not every meter sample. Volume exposes percentage text and keyboard adjustment.
- Empty catalogs, unavailable saved selections, discovery errors, missing/invalid credentials, and capture-open errors have clear next actions. Never silently select another input.
- Bot settings show backend-loaded status and bot identity, Recheck configuration, and Refresh servers. No token or private path appears in snapshots. Keep modal focus valid even when controls are disabled during a session.
- Disconnecting and cleanup-incomplete prevent another Connect. Settings remain readable. Close hides to tray; explicit Quit ends the session.

## Evaluation findings to fix

At 480 by 600 CSS pixels the draft had no horizontal overflow, but mute began below the viewport. Keyboard users could reach it only by scrolling. The connected-state settings modal targeted a disabled control for initial focus. White text on its original mint button had about 1.78:1 contrast. These are required fixes before production acceptance, not grounds to abandon the route-first layout.

Browser DPR/zoom tests do not establish native Windows display/text scaling or Narrator behavior. Production UI work must use generated Rust types, a real control adapter, fixture-only browser tests, screenshot review, and a separate native smoke. Preserve the old prototype as historical review material; it is not the production application.

Sources: [Windows typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography), [Fluent layout](https://fluent2.microsoft.design/layout), [OBS mixer organization](https://obsproject.com/kb/audio-mixer-guide), [WAI toggle buttons](https://www.w3.org/WAI/ARIA/apg/patterns/button/), [Mantine contrast](https://mantine.dev/core/button/#autocontrast), [Windows text scaling](https://learn.microsoft.com/en-us/windows/apps/develop/input/text-scaling), and the pinned skills in [.agents/SOURCES.md](../.agents/SOURCES.md).
