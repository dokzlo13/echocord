/** Development/test control plane only. This module is absent from release builds. */
import type { Transport } from "./adapter";
import type {
  ControlCommand,
  ControlSnapshot,
  DestinationTelemetry,
  InputTelemetry,
  MixTelemetry,
  ServerReservation,
  TimingObservation,
} from "./generated";

function selectedReservations(
  destinations: ControlSnapshot["destinations"],
): ServerReservation[] {
  return destinations
    .filter((d) => d.kind === "discord")
    .map((d) => ({
      serverId: d.serverId,
      channelId: d.channelId,
      destinationId: d.destinationId,

      sessionId: d.sessionId,
      phase:
        d.connection.phase === "cleanup-incomplete"
          ? "blocked"
          : d.connection.phase === "disconnecting"
            ? "leaving"
            : d.connection.phase === "connected"
              ? "connected"
              : d.connection.phase === "connecting" ||
                  d.connection.phase === "recovering"
                ? "joining"
                : "selected",
    }));
}
export const fixtureNames = [
  "first-run",
  "disconnected",
  "connected",
  "muted",
  "many",
  "restored",
  "faults",
  "connecting",
  "cleanup",
  "outputs",
  "rooms",
  "rooms-restored",
  "audio-processing",
  "audio-processing-off",
  "audio-processing-stale",
  "audio-processing-empty",
] as const;
export type FixtureName = (typeof fixtureNames)[number];
export function fixtureSnapshot(
  name: FixtureName = "disconnected",
): ControlSnapshot {
  const sources: ControlSnapshot["sources"] = [
    {
      id: "app:player:1",
      kind: "application",
      name: "Preview player",
      detail: "Example track · selected process tree",
      available: true,
    },
    {
      id: "app:browser:2",
      kind: "application",
      name: "Preview browser",
      detail: "Two windows · selected process tree",
      available: true,
    },
    {
      id: "rec:cable",
      kind: "recording",
      name: "Preview recording input",
      detail: "48 kHz stereo recording",
      available: true,
    },
    {
      id: "rec:unsupported",
      kind: "recording",
      name: "Unsupported input",
      detail:
        "This input has no supported 44.1, 48 or 96 kHz mono/stereo PCM format.",
      available: false,
    },
  ];
  const player = sources[0];
  const browser = sources[1];
  if (!player || !browser) throw Error("Missing owned test catalog");
  const servers: ControlSnapshot["servers"] = [
    {
      id: "guild:1",
      name: "Studio group",
      iconUrl: null,
      channels: [
        {
          id: "room:1",
          name: "Session room",
          categoryName: "Voice rooms",
          position: 0,
        },
        {
          id: "room:2",
          name: "Lounge",
          categoryName: "Voice rooms",
          position: 1,
        },
      ],
    },
    {
      id: "guild:2",
      name: "Вечерняя группа",
      iconUrl: null,
      channels: [
        {
          id: "room:3",
          name: "Сессия",
          categoryName: "Голосовые каналы",
          position: 0,
        },
      ],
    },
  ];
  const inputs: ControlSnapshot["inputs"] =
    name === "first-run"
      ? []
      : [
          {
            stripId: "strip:1",
            sourceId: player.id,
            kind: "application",
            displayName: player.name,
            detail: player.detail,
            phase: "capturing",
            gainDb: -3,
            muted: false,
            colourSlot: 0,
            restored: false,
            fault: null,
          },
        ];
  if (name === "many" || name === "restored" || name === "faults") {
    inputs.push({
      stripId: "strip:2",
      sourceId: browser.id,
      kind: "application",
      displayName: browser.name,
      detail: browser.detail,
      phase: name === "restored" ? "stopped" : "capturing",
      gainDb: 0,
      muted: name === "many",
      colourSlot: 1,
      restored: name === "restored",
      fault: null,
    });
    inputs.push({
      stripId: "strip:3",
      sourceId: "rec:cable",
      kind: "recording",
      displayName: "Preview recording input",
      detail: "48 kHz stereo",
      phase: "stopped",
      gainDb: 0,
      muted: false,
      colourSlot: 2,
      restored: false,
      fault: null,
    });
  }
  let connection: ControlSnapshot["connection"] =
    name === "connected" || name === "muted" || name === "many"
      ? { phase: "connected" }
      : name === "connecting"
        ? { phase: "connecting", stage: "join" }
        : name === "cleanup"
          ? {
              phase: "cleanup-incomplete",
              message: "Audio cleanup has not finished. Retry disconnect.",
            }
          : { phase: "disconnected" };
  if (name === "faults" && inputs[0]) {
    inputs[0].phase = "fault";
    inputs[0].fault = {
      code: "invalid-selection",
      message:
        "The selected application exited. Refresh sources and select its new instance.",
    };
    connection = {
      phase: "error",
      code: "permission-denied",
      message: "The bot cannot speak in this room. Allow Connect and Speak.",
    };
  }
  const telemetry: MixTelemetry = {
    clockMs: 0,
    mixObservedAtMs: null,
    gatewayObservedAtMs: null,
    destinations:
      name === "first-run"
        ? []
        : [
            {
              destinationId: "dest:1",
              observedAtMs: null,
              gatewayObservedAtMs: null,
              sessionId: 1,
              levelDbfs:
                connection.phase === "connected" && name !== "muted"
                  ? -24
                  : null,
              queuedFrames: connection.phase === "connected" ? 480 : 0,
              droppedFrames: 0,
              underrunFrames: 0,
              gatewayPingMs: null,
            },
          ],
    mixPreClip: false,
    inputs: inputs.map((s) => ({
      stripId: s.stripId,
      observedAtMs: null,
      levelDbfs: s.phase === "capturing" ? -18 : null,
      preClip: false,
      queuedFrames: 0,
      droppedFrames: 0,
      underrunFrames: 0,
    })),
    outputLevelDbfs:
      connection.phase === "connected" && name !== "muted" ? -24 : null,
    queuedFrames: connection.phase === "connected" ? 480 : 0,
    droppedFrames: 0,
    underrunFrames: 0,
    gatewayPingMs: null,
    limiterReductionDb: null,
  };
  const snapshot: ControlSnapshot = {
    preferencesSaving: false,
    gateway: {
      phase:
        connection.phase === "connected"
          ? "connected"
          : connection.phase === "connecting"
            ? "connecting"
            : connection.phase === "cleanup-incomplete"
              ? "cleanup-incomplete"
              : "disconnected",
      generation: 1,
      message: null,
    },
    serverReservations: [],
    playbackDevices: [
      {
        id: "play:headphones",
        name: "Preview headphones",
        available: true,
        detail: "48 kHz stereo playback",
      },
      {
        id: "play:speakers",
        name: "Preview speakers",
        available: true,
        detail: "48 kHz stereo playback",
      },
      {
        id: "play:cable",
        name: "Preview cable input",
        available: true,
        detail: "48 kHz stereo playback",
      },
      {
        id: "play:unavailable",
        name: "Unavailable output",
        available: false,
        detail: "This device is disconnected.",
      },
    ],
    playbackDiscovery: { phase: "ready" },
    revision: 1,
    sessionId: 1,
    sources,
    servers,
    inputs,
    destinations:
      name === "first-run"
        ? []
        : [
            {
              destinationId: "dest:1",
              kind: "discord",
              deviceId: null,
              displayName: null,
              sessionId: 1,
              serverId: "guild:1",
              channelId: "room:1",
              connection,
              volumeDb: -6,
              muted: name === "muted",
            },
          ],
    settings: {
      meterStyle: "simple",
      restoreSetup: true,
      keepInTray: true,
      driftCorrection: true,
      mixLimiter: false,
    },
    telemetry,
    selection: {
      sourceId: inputs[0]?.sourceId ?? null,
      serverId: name === "first-run" ? null : "guild:1",
      channelId: name === "first-run" ? null : "room:1",
    },
    bot: {
      status: name === "first-run" ? "unconfigured" : "ready",
      name: name === "first-run" ? null : "Preview bot",
      message: null,
    },
    discovery: { phase: "ready" },
    sourceDiscovery: { phase: "ready" },
    connection,
    capture: {
      phase: "running",
      levelDbfs: -18,
      outputLevelDbfs: telemetry.outputLevelDbfs,
      message: null,
      droppedFrames: 0,
      underrunFrames: 0,
      queuedFrames: telemetry.queuedFrames,
    },
    output: { volumePercent: 50, muted: name === "muted" },
  };
  if (name === "rooms" || name === "rooms-restored") {
    snapshot.servers.push(
      {
        id: "guild:3",
        name: "Listening group",
        iconUrl: null,
        channels: [
          {
            id: "room:4",
            name: "Listening room",
            categoryName: "Voice",
            position: 0,
          },
        ],
      },
      {
        id: "guild:4",
        name: "Late session",
        iconUrl: null,
        channels: [
          {
            id: "room:5",
            name: "Late room",
            categoryName: "Voice",
            position: 0,
          },
        ],
      },
    );
    const first = snapshot.destinations[0];
    if (first) {
      first.connection = {
        phase: name === "rooms" ? "connected" : "disconnected",
      };
      for (const [serverId, channelId, id] of [
        ["guild:2", "room:3", "dest:2"],
        ["guild:3", "room:4", "dest:3"],
      ] as const)
        snapshot.destinations.push({
          ...first,
          destinationId: id,
          serverId,
          channelId,
          volumeDb: -12,
        });
      snapshot.destinations.push({
        ...first,
        destinationId: "local:1",
        kind: "playback",
        serverId: "",
        channelId: "",
        deviceId: "play:headphones",
        displayName: "Preview headphones",
        volumeDb: 0,
      });
    }
    snapshot.telemetry.destinations = snapshot.destinations.map((d) => ({
      destinationId: d.destinationId,
      observedAtMs: null,
      gatewayObservedAtMs: null,
      sessionId: d.sessionId,
      levelDbfs: d.connection.phase === "connected" ? -20 : null,
      queuedFrames: d.connection.phase === "connected" ? 480 : 0,
      droppedFrames: 0,
      underrunFrames: 0,
      gatewayPingMs:
        d.kind === "discord" && d.connection.phase === "connected" ? 38 : null,
    }));
  }
  if (name === "outputs") {
    const discord = snapshot.destinations[0];
    if (discord) discord.connection = { phase: "connected" };
    for (const [id, index] of [
      ["play:headphones", 1],
      ["play:speakers", 2],
      ["play:cable", 3],
    ] as const) {
      const device = snapshot.playbackDevices.find((d) => d.id === id);
      if (device)
        snapshot.destinations.push({
          destinationId: `local:${index}`,
          sessionId: 1,
          kind: "playback",
          deviceId: id,
          displayName: device.name,
          serverId: "",
          channelId: "",
          connection:
            index === 2 ? { phase: "disconnected" } : { phase: "connected" },
          volumeDb: index === 3 ? -12 : 0,
          muted: index === 3,
        });
    }
    snapshot.telemetry.destinations = snapshot.destinations.map((d) => ({
      destinationId: d.destinationId,
      observedAtMs: null,
      gatewayObservedAtMs: null,
      sessionId: d.sessionId,
      levelDbfs: d.connection.phase === "connected" && !d.muted ? -20 : null,
      queuedFrames: 480,
      droppedFrames: 0,
      underrunFrames: 0,
      gatewayPingMs: d.kind === "discord" ? 38 : null,
    }));
  }
  if (name.startsWith("audio-processing")) audioProcessing(snapshot, name);
  snapshot.serverReservations = selectedReservations(snapshot.destinations);
  if (name === "rooms") snapshot.gateway.phase = "connected";
  snapshot.telemetry = freshTelemetry(snapshot.telemetry);
  return snapshot;
}
/** Owned scenario: three capturing sources, one Discord room and two playback devices. */
function audioProcessing(snapshot: ControlSnapshot, name: FixtureName) {
  const idle = name === "audio-processing-empty";
  const strips = [
    ["strip:1", "app:player:1", "Preview player", "application", -3],
    ["strip:2", "app:browser:2", "Preview browser", "application", 0],
    ["strip:3", "rec:cable", "Preview recording input", "recording", -6],
  ] as const;
  snapshot.inputs = strips.map(
    ([stripId, sourceId, displayName, kind, gainDb], colourSlot) => ({
      stripId,
      sourceId,
      kind,
      displayName,
      detail:
        snapshot.sources.find((source) => source.id === sourceId)?.detail ??
        null,
      phase: idle ? "stopped" : "capturing",
      gainDb,
      muted: false,
      colourSlot,
      restored: false,
      fault: null,
    }),
  );
  const connection = idle
    ? ({ phase: "disconnected" } as const)
    : ({ phase: "connected" } as const);
  snapshot.connection = connection;
  snapshot.gateway.phase = idle ? "disconnected" : "connected";
  const output = (
    destinationId: string,
    deviceId: string,
    displayName: string,
    live: boolean,
    volumeDb: number,
  ): ControlSnapshot["destinations"][number] => ({
    destinationId,
    kind: "playback",
    deviceId,
    displayName,
    sessionId: 1,
    serverId: "",
    channelId: "",
    connection: live ? connection : { phase: "disconnected" },
    volumeDb,
    muted: false,
  });
  snapshot.destinations = [
    {
      destinationId: "dest:1",
      kind: "discord",
      deviceId: null,
      displayName: null,
      sessionId: 1,
      serverId: "guild:1",
      channelId: "room:1",
      connection,
      volumeDb: -6,
      muted: false,
    },
    output("local:1", "play:headphones", "Preview headphones", true, 0),
    output("local:2", "play:speakers", "Preview speakers", false, -12),
  ];
  snapshot.settings = {
    ...snapshot.settings,
    driftCorrection: name !== "audio-processing-off",
    mixLimiter: name === "audio-processing-off",
  };
  snapshot.telemetry.inputs = snapshot.inputs.map((input) => ({
    stripId: input.stripId,
    observedAtMs: null,
    levelDbfs: input.phase === "capturing" ? -18 : null,
    preClip: false,
    queuedFrames: 0,
    droppedFrames: 0,
    underrunFrames: 0,
  }));
  snapshot.telemetry.destinations = snapshot.destinations.map((d) => ({
    destinationId: d.destinationId,
    observedAtMs: null,
    gatewayObservedAtMs: null,
    sessionId: d.sessionId,
    levelDbfs: d.connection.phase === "connected" ? -20 : null,
    queuedFrames: d.connection.phase === "connected" ? 720 : 0,
    droppedFrames: 0,
    underrunFrames: 0,
    gatewayPingMs:
      d.kind === "discord" && d.connection.phase === "connected" ? 38 : null,
  }));
  snapshot.telemetry.outputLevelDbfs = idle ? null : -20;
}

/** Illustrative per-stage values for the owned scenario only; never product measurements. */
const SIMULATED_STAGE: Record<string, { ppm: number; target: number }> = {
  "src:strip:1": { ppm: 12.4, target: 720 },
  "src:strip:2": { ppm: -3.1, target: 720 },
  "src:strip:3": { ppm: 41.7, target: 960 },
  "dst:local:1": { ppm: 8.3, target: 720 },
  "dst:local:2": { ppm: -18.6, target: 720 },
};
interface SimulatedStage {
  epoch: string;
  mode: string;
  startedAt: number;
  measured: boolean;
  ppm: number;
  queued: number;
  restarts: number;
  gaps: number;
  lastResetAt: number | null;
  primingUntil: number;
  nextRestart: number;
  frozen: TimingObservation | null;
}

/** Explicit synthetic observations. Control-only emits intentionally retain sample origins. */
export function freshTelemetry(
  value: MixTelemetry,
  clockMs = performance.now(),
): MixTelemetry {
  return {
    ...value,
    clockMs,
    mixObservedAtMs: clockMs,
    gatewayObservedAtMs: value.gatewayPingMs === null ? null : clockMs,
    inputs: value.inputs.map((v) => ({
      ...v,
      observedAtMs: v.levelDbfs === null ? null : clockMs,
    })),
    destinations: value.destinations.map((v) => ({
      ...v,
      observedAtMs: v.levelDbfs === null ? null : clockMs,
      gatewayObservedAtMs: v.gatewayPingMs === null ? null : clockMs,
    })),
  };
}
export class FixtureTransport implements Transport {
  current: ControlSnapshot;
  commands: ControlCommand[] = [];
  reject: unknown = null;
  rejectSave = false;
  failPreferences = false;
  holdConnect = false;
  holdDisconnect = false;
  minimized = 0;
  closed = 0;
  dragged = 0;
  private nextDestination = 0;
  private sampleTimer: ReturnType<typeof setInterval> | undefined;
  private simulateStages = false;
  private staleStage: string | null = null;
  private stages = new Map<string, SimulatedStage>();
  private lifetimes = 0;
  /**
   * Owned audio-processing scenarios only. A new epoch starts when a stage begins, when
   * correction is toggled and when a destination mute resets the playback stage; a source
   * mute and the limiter toggle keep the stage. The recording input restarts every 25 s.
   */
  private simulateTiming(
    key: string,
    active: boolean,
    reset: string,
    now: number,
  ): TimingObservation | undefined {
    if (!active) {
      this.stages.delete(key);
      return undefined;
    }
    const enabled = this.current.settings.driftCorrection;
    const mode = `${enabled ? "on" : "off"}:${reset}`;
    const base = SIMULATED_STAGE[key] ?? { ppm: 5, target: 720 };
    let stage = this.stages.get(key);
    if (!stage || stage.mode !== mode) {
      stage = {
        epoch: `fixture:${++this.lifetimes}:${mode}`,
        mode,
        startedAt: now,
        measured: false,
        ppm: stage?.ppm ?? 0,
        queued: 0,
        restarts: 0,
        gaps: 0,
        lastResetAt: null,
        primingUntil: now + 1400,
        nextRestart:
          key === "src:strip:3" ? now + 9000 : Number.POSITIVE_INFINITY,
        frozen: null,
      };
      this.stages.set(key, stage);
    }
    if (stage.frozen) return stage.frozen;
    const common = {
      epoch: stage.epoch,
      capacityFrames: 4096,
      droppedInputFrames: 0,
      gapOutputFrames: stage.gaps,
      resyncCount: stage.restarts,
      lastResetReason:
        stage.lastResetAt === null ? null : ("stalled-input" as const),
      lastResetAtMs: stage.lastResetAt,
      stateObservedAtMs: now,
      audioObservedAtMs: now,
    };
    let observation: TimingObservation;
    if (!enabled) {
      observation = {
        ...common,
        phase: "off",
        appliedAdjustmentPpm: null,
        // Uncorrected queues wander; illustrative only.
        queuedFrames: Math.round(
          base.target + 360 * Math.sin((now - stage.startedAt) / 9000),
        ),
        targetFrames: null,
      };
    } else {
      if (now >= stage.nextRestart) {
        stage.restarts++;
        stage.gaps += 480;
        stage.lastResetAt = now;
        stage.primingUntil = now + 900;
        stage.queued = 0;
        stage.nextRestart = now + 25_000;
      }
      const priming = now < stage.primingUntil;
      if (priming)
        stage.queued = Math.min(base.target, stage.queued + base.target / 12);
      else {
        stage.measured = true;
        stage.queued = base.target + Math.round(30 * Math.sin(now / 1700));
        stage.ppm += (base.ppm - stage.ppm) * 0.08;
      }
      observation = {
        ...common,
        lastResetReason:
          stage.lastResetAt === null ? null : ("stalled-input" as const),
        phase: priming ? "priming" : "running",
        appliedAdjustmentPpm: stage.measured
          ? stage.ppm + 0.2 * Math.sin(now / 2300)
          : null,
        queuedFrames: Math.round(stage.queued),
        targetFrames: base.target,
      };
    }
    if (key === this.staleStage && now - stage.startedAt > 2500)
      stage.frozen = observation;
    return observation;
  }
  private telemetryListeners = new Set<(value: MixTelemetry) => void>();
  listenTelemetry = async (listener: (value: MixTelemetry) => void) => {
    this.telemetryListeners.add(listener);
    return () => this.telemetryListeners.delete(listener);
  };
  observeTelemetry(
    value = this.current.telemetry,
    clockMs = performance.now(),
  ) {
    this.current.telemetry = freshTelemetry(value, clockMs);
    for (const listener of this.telemetryListeners)
      listener(structuredClone(this.current.telemetry));
  }
  /** Browser fixtures explicitly simulate native audio every 100 ms and gateway reports each second. */
  startTelemetry() {
    this.stopTelemetry();
    this.sampleTimer = setInterval(() => {
      const prior = this.current.telemetry;
      const now = performance.now();
      const inputs: InputTelemetry[] = this.current.inputs.map((input) => {
        const previous = prior.inputs.find((v) => v.stripId === input.stripId);
        return {
          stripId: input.stripId,
          observedAtMs: null,
          levelDbfs:
            input.phase === "capturing" ? (previous?.levelDbfs ?? -18) : null,
          preClip: previous?.preClip ?? false,
          queuedFrames: previous?.queuedFrames ?? 0,
          droppedFrames: previous?.droppedFrames ?? 0,
          underrunFrames: previous?.underrunFrames ?? 0,
        };
      });
      const destinations: DestinationTelemetry[] =
        this.current.destinations.map((destination) => {
          const previous = prior.destinations.find(
            (v) =>
              v.destinationId === destination.destinationId &&
              v.sessionId === destination.sessionId,
          );
          const active =
            destination.connection.phase === "connected" ||
            destination.connection.phase === "recovering";
          return {
            destinationId: destination.destinationId,
            sessionId: destination.sessionId,
            observedAtMs: null,
            gatewayObservedAtMs: previous?.gatewayObservedAtMs ?? null,
            levelDbfs: active
              ? destination.muted
                ? -96
                : (previous?.levelDbfs ?? -24)
              : null,
            queuedFrames: active ? (previous?.queuedFrames ?? 480) : 0,
            droppedFrames: previous?.droppedFrames ?? 0,
            underrunFrames: previous?.underrunFrames ?? 0,
            gatewayPingMs: previous?.gatewayPingMs ?? null,
          };
        });
      let mixPreClip = prior.mixPreClip;
      let limiterReductionDb: number | null = null;
      if (this.simulateStages) {
        for (const [index, input] of inputs.entries()) {
          const strip = this.current.inputs.find(
            (s) => s.stripId === input.stripId,
          );
          const timing = this.simulateTiming(
            `src:${input.stripId}`,
            strip?.phase === "capturing",
            "",
            now,
          );
          if (timing) inputs[index] = { ...input, timing };
        }
        for (const [index, output] of destinations.entries()) {
          const strip = this.current.destinations.find(
            (d) => d.destinationId === output.destinationId,
          );
          const timing = this.simulateTiming(
            `dst:${output.destinationId}`,
            strip?.kind === "playback" &&
              strip.connection.phase === "connected",
            strip?.muted ? "muted" : "",
            now,
          );
          // The playback tile and the timing stage read the same backend queue.
          if (timing)
            destinations[index] = {
              ...output,
              queuedFrames: timing.queuedFrames ?? output.queuedFrames,
              timing,
            };
        }
        // Illustrative overload every 12 s; the indicator reads the sum before limiting.
        const loud =
          this.current.inputs.some((s) => s.phase === "capturing") &&
          now % 12_000 > 3000 &&
          now % 12_000 < 4600;
        mixPreClip = loud;
        if (this.current.settings.mixLimiter)
          limiterReductionDb = loud
            ? 1.4 + Math.abs(Math.sin(now / 300)) * 0.9
            : 0;
      }
      const sample = freshTelemetry(
        { ...prior, inputs, destinations, mixPreClip, limiterReductionDb },
        now,
      );
      sample.destinations = sample.destinations.map((v) => {
        const previous = prior.destinations.find(
          (p) =>
            p.destinationId === v.destinationId && p.sessionId === v.sessionId,
        );
        return previous?.gatewayObservedAtMs != null &&
          now - previous.gatewayObservedAtMs < 1000
          ? { ...v, gatewayObservedAtMs: previous.gatewayObservedAtMs }
          : v;
      });
      this.current.telemetry = sample;
      for (const listener of this.telemetryListeners)
        listener(structuredClone(sample));
    }, 100);
  }
  stopTelemetry() {
    clearInterval(this.sampleTimer);
    this.sampleTimer = undefined;
  }
  private listeners = new Set<(s: ControlSnapshot) => void>();
  private visibility = new Set<(v: boolean) => void>();
  constructor(name: FixtureName = "disconnected") {
    this.current = fixtureSnapshot(name);
    this.simulateStages = name.startsWith("audio-processing");
    this.staleStage = name === "audio-processing-stale" ? "src:strip:2" : null;
  }
  listen = async (f: (s: ControlSnapshot) => void) => {
    this.listeners.add(f);
    return () => this.listeners.delete(f);
  };
  getSnapshot = async () => structuredClone(this.current);
  getVisible = async () => true;
  listenVisibility = async (f: (v: boolean) => void) => {
    this.visibility.add(f);
    return () => this.visibility.delete(f);
  };
  visible(v: boolean) {
    for (const f of this.visibility) f(v);
  }
  emit(patch: Partial<ControlSnapshot>) {
    if (patch.destinations && !patch.serverReservations) {
      const retained = this.current.serverReservations.filter(
        (r) => r.phase === "leaving" || r.phase === "blocked",
      );
      patch.serverReservations = [
        ...selectedReservations(patch.destinations).filter(
          (r) =>
            !retained.some(
              (old) =>
                old.destinationId === r.destinationId &&
                old.serverId === r.serverId &&
                old.channelId === r.channelId &&
                old.sessionId === r.sessionId,
            ),
        ),
        ...retained,
      ];
    }
    if (patch.telemetry) {
      const t = patch.telemetry;
      const d = (patch.destinations ?? this.current.destinations).find(
        (d) => d.kind === "discord",
      );
      if (d) {
        const current = t.destinations.find(
          (v) => v.destinationId === d.destinationId,
        );
        if (current)
          t.destinations = t.destinations.map((v) =>
            v.destinationId === d.destinationId
              ? {
                  ...v,
                  sessionId: d.sessionId,
                  levelDbfs: t.outputLevelDbfs,
                  queuedFrames: t.queuedFrames,
                  droppedFrames: t.droppedFrames,
                  underrunFrames: t.underrunFrames,
                  gatewayPingMs: t.gatewayPingMs,
                }
              : v,
          );
      }
    }
    this.current = {
      ...this.current,
      ...patch,
      revision: this.current.revision + 1,
    };
    for (const f of this.listeners) f(structuredClone(this.current));
  }
  configureBot = async (_token: string) => {
    if (this.rejectSave) throw Error("Fixture credential failure");
    this.emit({
      bot: { status: "ready", name: "Configured bot", message: null },
    });
  };
  minimize = async () => {
    this.minimized++;
  };
  close = async () => {
    this.closed++;
  };
  startDragging = async () => {
    this.dragged++;
  };
  dispatch = async (c: ControlCommand) => {
    this.commands.push(c);
    if (this.reject) throw this.reject;
    const input = (id: string) =>
      this.current.inputs.find((s) => s.stripId === id);
    const dest = (id: string) =>
      this.current.destinations.find((d) => d.destinationId === id);
    switch (c.type) {
      case "add-source": {
        const s = this.current.sources.find((s) => s.id === c.sourceId);
        if (s) {
          const colour =
            [0, 1, 2, 3, 4].find(
              (slot) => !this.current.inputs.some((s) => s.colourSlot === slot),
            ) ?? 5;
          this.current.inputs.push({
            stripId: `strip:${Date.now()}`,
            sourceId: s.id,
            kind: s.kind,
            displayName: s.name,
            detail: s.detail,
            phase: "capturing",
            fault: null,
            gainDb: 0,
            muted: false,
            colourSlot: colour,
            restored: false,
          });
        }
        break;
      }
      case "remove-source":
        this.current.inputs = this.current.inputs.filter(
          (s) => s.stripId !== c.stripId,
        );
        break;
      case "replace-source": {
        const s = input(c.stripId);
        const next = this.current.sources.find((s) => s.id === c.sourceId);
        if (s && next)
          Object.assign(s, {
            sourceId: next.id,
            kind: next.kind,
            displayName: next.name,
            detail: next.detail,
            phase: "capturing",
            fault: null,
            restored: false,
          });
        break;
      }
      case "start-capture": {
        const s = input(c.stripId);
        if (s) {
          s.phase = "capturing";
          s.restored = false;
        }
        break;
      }
      case "stop-capture": {
        const s = input(c.stripId);
        if (s) s.phase = "stopped";
        break;
      }
      case "set-source-muted": {
        const s = input(c.stripId);
        if (s) s.muted = c.muted;
        break;
      }
      case "set-source-gain": {
        const s = input(c.stripId);
        if (s) s.gainDb = c.gainDb;
        break;
      }
      case "add-destination":
        if (this.current.destinations.length >= 4)
          throw {
            code: "invalid-selection",
            message: "Four destinations are supported. Remove one first.",
          };
        if (
          this.current.serverReservations.some((r) => r.serverId === c.serverId)
        )
          throw {
            code: "invalid-selection",
            message:
              "Another destination reserves this server. Choose a different server.",
          };
        this.current.destinations.push({
          destinationId: `fixture-output:${++this.nextDestination}`,
          kind: "discord",
          deviceId: null,
          displayName: null,
          sessionId: 1,
          serverId: c.serverId,
          channelId: c.channelId,
          connection: { phase: "disconnected" },
          volumeDb: 0,
          muted: false,
        });
        break;
      case "remove-destination":
        this.current.destinations = this.current.destinations.filter(
          (d) => d.destinationId !== c.destinationId,
        );
        // Removal completes synchronously only in this owned fixture.
        this.current.serverReservations =
          this.current.serverReservations.filter(
            (r) => r.destinationId !== c.destinationId,
          );
        break;
      case "change-destination": {
        const d = dest(c.destinationId);
        if (d) {
          if (d.serverId === c.serverId && d.channelId === c.channelId) break;
          if (
            this.current.serverReservations.some(
              (r) =>
                r.destinationId === d.destinationId &&
                (r.phase === "leaving" || r.phase === "blocked"),
            )
          )
            throw {
              code: "busy",
              message: "Room cleanup has not finished. Retry disconnect first.",
            };
          if (
            this.current.serverReservations.some(
              (r) =>
                r.serverId === c.serverId &&
                r.destinationId !== d.destinationId,
            )
          )
            throw {
              code: "invalid-selection",
              message:
                "Another destination reserves this server. Choose a different server.",
            };
          d.serverId = c.serverId;
          d.channelId = c.channelId;
          if (d.connection.phase === "connected") d.sessionId++;
        }
        break;
      }
      case "connect-destination": {
        const d = dest(c.destinationId);
        if (d) {
          const sharedReady = this.current.gateway.phase === "connected";
          d.sessionId++;
          d.connection = this.holdConnect
            ? {
                phase: "connecting",
                stage:
                  d.kind === "playback"
                    ? "capture"
                    : sharedReady
                      ? "join"
                      : "gateway",
              }
            : { phase: "connected" };
          if (d.kind === "discord" && !sharedReady)
            this.current.gateway = {
              phase: this.holdConnect ? "connecting" : "connected",
              generation: ["connecting", "recovering"].includes(
                this.current.gateway.phase,
              )
                ? this.current.gateway.generation
                : this.current.gateway.generation + 1,
              message: null,
            };
        }
        break;
      }
      case "disconnect-destination": {
        const d = dest(c.destinationId);
        if (d) {
          d.connection = this.holdDisconnect
            ? {
                phase: "cleanup-incomplete",
                message: "Audio cleanup has not finished.",
              }
            : { phase: "disconnected" };
          // A successful fixture Stop is an explicit observed cleanup receipt.
          if (!this.holdDisconnect)
            this.current.serverReservations =
              this.current.serverReservations.filter(
                (r) => r.destinationId !== d.destinationId,
              );
          const owned =
            this.current.destinations.some(
              (peer) =>
                peer.kind === "discord" &&
                !["disconnected", "error"].includes(peer.connection.phase),
            ) ||
            this.current.serverReservations.some((r) => r.phase !== "selected");
          if (d.kind === "discord" && !owned)
            this.current.gateway.phase = "disconnected";
        }
        break;
      }
      case "set-destination-volume": {
        const d = dest(c.destinationId);
        if (d) d.volumeDb = c.volumeDb;
        break;
      }
      case "set-destination-muted": {
        const d = dest(c.destinationId);
        if (d) d.muted = c.muted;
        break;
      }
      case "add-playback-destination": {
        if (
          this.current.destinations.length >= 4 ||
          this.current.destinations.filter((d) => d.kind === "playback")
            .length >= 3
        )
          throw {
            code: "invalid-selection",
            message: "Output capacity is full. Remove one first.",
          };
        const device = this.current.playbackDevices.find(
          (d) => d.id === c.deviceId,
        );
        if (device)
          this.current.destinations.push({
            destinationId: `fixture-output:${++this.nextDestination}`,
            sessionId: 0,
            kind: "playback",
            deviceId: device.id,
            displayName: device.name,
            serverId: "",
            channelId: "",
            connection: { phase: "disconnected" },
            volumeDb: 0,
            muted: false,
          });
        break;
      }
      case "change-playback-destination": {
        const d = dest(c.destinationId);
        const device = this.current.playbackDevices.find(
          (d) => d.id === c.deviceId,
        );
        if (d && device) {
          d.deviceId = device.id;
          d.displayName = device.name;
          if (d.connection.phase === "connected") d.sessionId++;
        }
        break;
      }
      case "refresh-playback-devices":
        this.current.playbackDiscovery = { phase: "ready" };
        break;
      case "set-settings":
        this.current.settings = c.settings;
        break;
      case "retry-preferences":
        break;
      case "refresh-sources":
        this.current.sourceDiscovery = { phase: "ready" };
        break;
      case "refresh-catalog":
        this.current.discovery = { phase: "ready" };
        break;
      case "recheck-configuration":
        this.current.bot = { ...this.current.bot, status: "ready" };
        break;
    }
    if (
      [
        "set-settings",
        "set-source-gain",
        "set-source-muted",
        "set-destination-volume",
        "set-destination-muted",
        "retry-preferences",
      ].includes(c.type)
    ) {
      if (this.failPreferences) {
        this.current.preferencesWarning = {
          code: "configuration",
          message: "Preferences could not be saved.",
        };
      } else {
        delete this.current.preferencesWarning;
      }
    }
    this.emit({
      inputs: this.current.inputs,
      destinations: this.current.destinations,
      settings: this.current.settings,
    });
  };
}
