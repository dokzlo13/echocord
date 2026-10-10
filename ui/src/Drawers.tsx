import {
  ActionIcon,
  Button,
  Drawer,
  PasswordInput,
  Popover,
  Tabs,
  TextInput,
} from "@mantine/core";
import {
  type ReactElement,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
import styles from "./App.module.css";
import type { ControlAdapter, ViewState } from "./adapter";
import type {
  ControlSnapshot,
  Selection,
  Server,
  SourceKind,
} from "./generated";
import { Icon } from "./Icons";
import { locksRoute } from "./presentation";
import { CaptureCounters } from "./Telemetry";

export type DrawerName = "source" | "target" | "bot" | "details";

export function PickerPopover({
  name,
  title,
  opened,
  close,
  trigger,
  children,
}: {
  name: "source" | "target";
  title: string;
  opened: boolean;
  close: () => void;
  trigger: ReactElement;
  children: ReactNode;
}) {
  const content = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (opened)
      content.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [opened]);
  useEffect(() => {
    if (!opened) return;
    const dismissWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", dismissWithEscape);
    return () => window.removeEventListener("keydown", dismissWithEscape);
  }, [opened, close]);
  const padding = { top: 8, right: 12, bottom: 78, left: 12 };
  return (
    <Popover
      opened={opened}
      onDismiss={close}
      closeOnEscape={false}
      position={name === "source" ? "bottom-start" : "top-end"}
      width="min(344px, 80vw)"
      offset={8}
      floatingStrategy="fixed"
      middlewares={{ flip: { padding }, shift: { padding, crossAxis: true } }}
      trapFocus={false}
      returnFocus={false}
      transitionProps={{ duration: 0 }}
    >
      <Popover.Target>{trigger}</Popover.Target>
      <Popover.Dropdown
        ref={content}
        className={styles.pickerContent}
        aria-label={title}
        aria-labelledby={`${name}-picker-title`}
        aria-modal={false}
      >
        <div className={styles.pickerHeader}>
          <h2 id={`${name}-picker-title`}>{title}</h2>
          <ActionIcon
            variant="subtle"
            aria-label={`Close ${title.toLowerCase()}`}
            onClick={close}
          >
            <Icon name="close" />
          </ActionIcon>
        </div>
        <div className={styles.pickerBody}>{children}</div>
      </Popover.Dropdown>
    </Popover>
  );
}

function DrawerViewport({ children }: { children: ReactNode }) {
  return <div className={styles.drawerViewport}>{children}</div>;
}

export function Sheet({
  title,
  close,
  children,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
}) {
  const content = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const initial = content.current?.querySelector<HTMLElement>(
      "input, [data-autofocus]",
    );
    initial?.focus();
  }, []);
  return (
    <Drawer.Root
      opened
      onClose={close}
      position="right"
      size="calc(100% - 24px)"
      scrollAreaComponent={DrawerViewport}
      trapFocus={false}
      returnFocus={false}
      lockScroll={false}
      closeOnClickOutside={false}
      transitionProps={{ duration: 0 }}
      classNames={{
        inner: styles.drawerInner,
        content: styles.drawerContent,
        header: styles.drawerHeader,
        body: styles.drawerBody,
      }}
    >
      <Drawer.Content
        ref={(node) => {
          // Mantine 9.7.1 hardcodes aria-modal; this sheet leaves transport accessible.
          node?.setAttribute("aria-modal", "false");
          content.current = node;
        }}
      >
        <Drawer.Header>
          <Drawer.Title>{title}</Drawer.Title>
          <Drawer.CloseButton
            aria-label={`Close ${title.toLowerCase()}`}
            data-autofocus
          />
        </Drawer.Header>
        <Drawer.Body>{children}</Drawer.Body>
      </Drawer.Content>
    </Drawer.Root>
  );
}

export function ServerIcon({ server }: { server: Server | undefined }) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  if (server?.iconUrl && failedUrl !== server.iconUrl)
    return (
      <img
        className={styles.serverIcon}
        src={server.iconUrl}
        alt=""
        width="28"
        height="28"
        referrerPolicy="no-referrer"
        onError={() => setFailedUrl(server.iconUrl)}
      />
    );
  return (
    <span className={styles.serverIcon} aria-hidden="true">
      {server?.name
        .split(/\s+/)
        .slice(0, 2)
        .map((part) => part[0])
        .join("") ?? "D"}
    </span>
  );
}

export function SourcePicker({
  snapshot,
  draft,
  busy,
  refreshing,
  choose,
  refresh,
}: {
  snapshot: ControlSnapshot;
  draft: Selection;
  busy: boolean;
  refreshing: boolean;
  choose: (sourceId: string) => void;
  refresh: () => void;
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<SourceKind>(
    () =>
      snapshot.sources.find((source) => source.id === draft.sourceId)?.kind ??
      "application",
  );
  const sources = snapshot.sources.filter(
    (source) =>
      source.kind === kind &&
      `${source.name} ${source.detail ?? ""}`
        .toLocaleLowerCase()
        .includes(search.toLocaleLowerCase()),
  );
  const panel = (
    <>
      <p className={styles.drawerHint}>
        {kind === "application"
          ? "Browser capture can include other tabs and windows. Keep calls in another app. Local playback continues."
          : "Choose a Windows recording endpoint, then apply."}
      </p>
      <TextInput
        aria-label={
          kind === "application"
            ? "Search applications"
            : "Search recording inputs"
        }
        placeholder={
          kind === "application"
            ? "Search applications"
            : "Search recording inputs"
        }
        leftSection={<Icon name="search" />}
        value={search}
        onChange={(event) => setSearch(event.currentTarget.value)}
        data-autofocus
      />
      <div className={styles.drawerList}>
        {sources.map((source) => (
          <button
            type="button"
            className={styles.choice}
            key={source.id}
            disabled={!source.available || busy}
            aria-pressed={draft.sourceId === source.id}
            onClick={() => choose(source.id)}
          >
            <span className={styles.choiceTitle}>{source.name}</span>
            <span className={styles.secondary}>
              {source.detail ??
                (source.kind === "application"
                  ? "Application and child processes"
                  : "Recording endpoint")}
              {!source.available && " · Unavailable"}
            </span>
            <span className={styles.selectionTag}>
              {source.id === snapshot.selection.sourceId
                ? "Current"
                : source.id === draft.sourceId
                  ? "Queued"
                  : !source.available
                    ? "Unavailable"
                    : ""}
            </span>
          </button>
        ))}
        {sources.length === 0 && (
          <p>
            {refreshing
              ? "Looking for audio sources…"
              : snapshot.sourceDiscovery.phase === "error"
                ? "Source list unavailable. Try Refresh sources."
                : search.trim()
                  ? "No sources match your search."
                  : snapshot.sourceDiscovery.phase === "idle"
                    ? "Refresh sources to find available audio."
                    : kind === "application"
                      ? "No applications found. Open an app window, then refresh."
                      : "No recording inputs found. Enable the device in Windows Sound settings, then refresh."}
          </p>
        )}
      </div>
    </>
  );
  return (
    <>
      <Tabs
        value={kind}
        keepMounted={false}
        onChange={(value) => {
          if (value === "application" || value === "recording") {
            setKind(value);
            setSearch("");
          }
        }}
        className={styles.sourceTabs}
      >
        <Tabs.List aria-label="Source types" className={styles.sourceTabList}>
          <Tabs.Tab value="application" className={styles.sourceTab}>
            Applications
          </Tabs.Tab>
          <Tabs.Tab value="recording" className={styles.sourceTab}>
            Recording inputs
          </Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="application" className={styles.sourcePanel}>
          {kind === "application" && panel}
        </Tabs.Panel>
        <Tabs.Panel value="recording" className={styles.sourcePanel}>
          {kind === "recording" && panel}
        </Tabs.Panel>
      </Tabs>
      <div className={styles.drawerFooter}>
        <Button variant="default" disabled={refreshing} onClick={refresh}>
          {refreshing ? "Refreshing…" : "Refresh sources"}
        </Button>
        {refreshing && <p role="status">Looking for audio sources…</p>}
        {snapshot.sourceDiscovery.phase === "error" && (
          <p role="alert" className={styles.warning}>
            {snapshot.sourceDiscovery.message}
          </p>
        )}
      </div>
    </>
  );
}

export function RoomPicker({
  snapshot,
  draft,
  busy,
  choose,
  refresh,
  setup,
}: {
  snapshot: ControlSnapshot;
  draft: Selection;
  busy: boolean;
  choose: (serverId: string, channelId: string) => void;
  refresh: () => void;
  setup: () => void;
}) {
  const [search, setSearch] = useState("");
  const query = search.trim().toLocaleLowerCase();
  const servers = snapshot.servers
    .map((server) => ({
      ...server,
      channels: [...server.channels]
        .sort((a, b) => a.position - b.position)
        .filter((room) =>
          `${server.name} ${room.categoryName ?? ""} ${room.name}`
            .toLocaleLowerCase()
            .includes(query),
        ),
    }))
    .filter((server) => server.channels.length > 0 || !query);
  return (
    <>
      <p className={styles.drawerHint}>Pick a room, then apply.</p>
      <TextInput
        aria-label="Search servers and rooms"
        placeholder="Search servers and rooms"
        leftSection={<Icon name="search" />}
        value={search}
        onChange={(event) => setSearch(event.currentTarget.value)}
        data-autofocus
      />
      <div className={styles.drawerList} data-searching={Boolean(query)}>
        {snapshot.bot.status !== "ready" && (
          <div className={styles.empty}>
            <p>
              Set up your Discord bot to discover its servers and voice rooms.
            </p>
            <Button onClick={setup}>Set up bot</Button>
          </div>
        )}
        {snapshot.bot.status === "ready" &&
          servers.map((server) => {
            const categories = [
              ...new Set(server.channels.map((room) => room.categoryName)),
            ];
            return (
              <section
                className={styles.serverGroup}
                key={server.id}
                aria-label={server.name}
              >
                <h3>
                  <ServerIcon server={server} />
                  <span>{server.name}</span>
                </h3>
                {categories.map((category) => (
                  <div
                    key={category ?? "uncategorized"}
                    className={styles.category}
                  >
                    <h4>{category ?? "Voice rooms"}</h4>
                    {server.channels
                      .filter((room) => room.categoryName === category)
                      .map((room) => {
                        const current =
                          snapshot.selection.serverId === server.id &&
                          snapshot.selection.channelId === room.id;
                        const next =
                          draft.serverId === server.id &&
                          draft.channelId === room.id;
                        return (
                          <button
                            type="button"
                            key={room.id}
                            className={styles.roomChoice}
                            disabled={busy}
                            aria-pressed={next}
                            aria-description={`${server.name}, ${category ?? "Voice rooms"}`}
                            onClick={() => choose(server.id, room.id)}
                          >
                            <Icon name="speaker" />
                            <span>{room.name}</span>
                            <span className={styles.selectionTag}>
                              {current ? "Current" : next ? "Queued" : ""}
                            </span>
                          </button>
                        );
                      })}
                  </div>
                ))}
                {server.channels.length === 0 && (
                  <p>
                    No accessible voice rooms. Check the bot's channel
                    permissions.
                  </p>
                )}
              </section>
            );
          })}
        {snapshot.bot.status === "ready" && servers.length === 0 && (
          <p>
            {query
              ? "No rooms match your search."
              : "No servers found. Invite this bot to your server, then refresh."}
          </p>
        )}
      </div>
      <div className={styles.drawerFooter}>
        <Button
          variant="default"
          disabled={
            busy || locksRoute(snapshot) || snapshot.bot.status !== "ready"
          }
          onClick={refresh}
        >
          Refresh rooms
        </Button>
        {snapshot.discovery.phase === "loading" && (
          <p role="status">Loading servers and rooms…</p>
        )}
        {snapshot.discovery.phase === "error" && (
          <p role="alert">{snapshot.discovery.message}</p>
        )}
      </div>
    </>
  );
}

export function BotSetup({
  adapter,
  view,
  close,
}: {
  adapter: ControlAdapter;
  view: ViewState;
  close: () => void;
}) {
  const [token, setToken] = useState("");
  const [empty, setEmpty] = useState(false);
  const snapshot = view.snapshot;
  const locked =
    !snapshot || locksRoute(snapshot) || Boolean(view.routeOperation);
  const pending = view.configuringBot;
  const checking =
    snapshot?.bot.status === "checking" ||
    view.pending.includes("recheck-configuration");
  const disabled = locked || pending || checking || view.phase !== "ready";
  return (
    <>
      <p className={styles.drawerHint}>
        Use a Discord bot token. Echocord tests it and saves it encrypted on
        this Windows account.
      </p>
      <p className={styles.botState} role="status">
        {snapshot?.bot.status === "ready"
          ? `Bot ready: ${snapshot.bot.name ?? "Discord bot"}`
          : snapshot?.bot.status === "checking"
            ? "Checking bot…"
            : "Bot setup needed"}
      </p>
      {snapshot?.bot.message && <p>{snapshot.bot.message}</p>}
      {locked && (
        <p className={styles.warning}>
          Stop the session before changing bot configuration. Mute and stop
          remain available below.
        </p>
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (disabled) return;
          if (!token.trim()) {
            setEmpty(true);
            return;
          }
          void adapter.configureBot(token).then((saved) => {
            if (saved) {
              setToken("");
              close();
            }
          });
        }}
      >
        <PasswordInput
          label="Bot token"
          description="The saved token cannot be read back."
          autoComplete="off"
          value={token}
          maxLength={4096}
          disabled={disabled}
          error={empty ? "Enter your bot token." : undefined}
          onChange={(event) => {
            setToken(event.currentTarget.value);
            setEmpty(false);
          }}
        />
        {view.configurationError && (
          <p role="alert" className={styles.warning}>
            {view.configurationError.message}
          </p>
        )}
        <Button type="submit" mt="sm" disabled={disabled}>
          {pending ? "Testing & saving…" : "Test & Save"}
        </Button>
      </form>
      <div className={styles.setupActions}>
        <Button
          variant="default"
          disabled={disabled}
          onClick={() =>
            void adapter.dispatch({ type: "recheck-configuration" })
          }
        >
          Recheck bot
        </Button>
        <Button
          variant="default"
          disabled={
            disabled ||
            snapshot?.bot.status !== "ready" ||
            view.pending.includes("refresh-catalog")
          }
          onClick={() => void adapter.dispatch({ type: "refresh-catalog" })}
        >
          Refresh rooms
        </Button>
      </div>
      <p className={styles.secondary}>
        Invite the bot to your server with access to the voice room and the
        Connect and Speak permissions. After setup, choose an input and room,
        then apply the selection.
      </p>
    </>
  );
}

export function AudioDetails({
  adapter,
  snapshot,
  messages,
}: {
  adapter: ControlAdapter;
  snapshot: ControlSnapshot | null;
  messages: string[];
}) {
  return (
    <div className={styles.detailContent}>
      {messages.map((message) => (
        <p className={styles.warning} key={message}>
          {message}
        </p>
      ))}
      <h3>Audio measurements</h3>
      <p>
        Input peak is before output gain. Mix peak measures audio after gain and
        mute, before Discord. Neither meter confirms what a listener hears.
      </p>
      <p>
        The capture queue uses 48 kHz engine PCM: 1,920 frames is 40 ms
        capacity. Queue duration is buffered capture audio, not total local or
        listener delay.
      </p>
      <CaptureCounters adapter={adapter} />
      <p className={styles.secondary}>
        These local capture and reader counters are not Discord packet loss.
      </p>
      <h3>Current route</h3>
      <p>
        {snapshot?.sources.find(
          (source) => source.id === snapshot.selection.sourceId,
        )?.name ?? "Audio source not available"}
      </p>
      <p>
        {snapshot?.servers.find(
          (server) => server.id === snapshot.selection.serverId,
        )?.name ?? "Server not available"}
      </p>
      <p>
        {snapshot?.servers
          .flatMap((server) => server.channels)
          .find((room) => room.id === snapshot.selection.channelId)?.name ??
          "Voice room not available"}
      </p>
      <h3>Routing help</h3>
      <p>
        {snapshot?.sources.find(
          (source) => source.id === snapshot.selection.sourceId,
        )?.kind === "application"
          ? "Application capture includes the selected app and its child processes. Browser capture can include other tabs and windows; keep calls in another app. Original local playback continues."
          : "Send your audio app to VB-Cable's playback endpoint and capture its recording endpoint here. Echocord does not play audio locally."}{" "}
        Closing the window keeps the session in the tray; use Quit in the tray
        to exit.
      </p>
    </div>
  );
}
