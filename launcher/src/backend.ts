// Manages the Rust backend as a child process (opencode model): the `teams`
// command owns the server lifecycle — spawn it, wait until it's listening, and
// stop it on exit. One command starts everything.

import { spawn, type Subprocess } from "bun";
import { chmodSync, existsSync, mkdirSync, openSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import {
  assetId,
  cachedAssetIsCurrent,
  replaceFile,
  stampCachedAsset,
} from "./embedded-cache";

/// Where a normal, send-capable backend listens.
const DEFAULT_PORT = 19420;
/// Where a READ-ONLY backend listens instead, so it never takes the port the
/// user's own backend wants.
const READ_ONLY_PORT = 19430;

const HOST = "127.0.0.1";

/// The port the backend will bind, resolved exactly as the backend itself resolves
/// it (`resolve_port` in src/bin/server.rs): an explicit `TEAMS_LITE_PORT` wins,
/// else read-only mode moves aside to 19430. Two processes read the same
/// environment, so a rule duplicated here is a rule that drifts — and a launcher
/// that waited on 19420 while the child bound 19430 would time out on every
/// read-only start.
export function backendPort(): number {
  const configured = Number(process.env.TEAMS_LITE_PORT);
  if (Number.isInteger(configured) && configured > 0) return configured;
  return process.env.TEAMS_LITE_READ_ONLY === "1" ? READ_ONLY_PORT : DEFAULT_PORT;
}

/// The WebSocket URL of that backend.
export function backendUrl(): string {
  return `ws://${HOST}:${backendPort()}`;
}

/// Where the backend we spawn writes its output — one file PER PORT.
///
/// Two send-capable installs share this machine (AGENTS.md § Running the released build
/// beside the staged one), and they used to share one log path as well: each writer kept its
/// own offset into a truncated file, so the two runs' output was spliced together with lines
/// cut mid-word. The port is what already tells the two backends apart everywhere else, so it
/// is what names the file. The DEFAULT port keeps the historical name, because that is the
/// path every doc, every error message and every reader's muscle memory already holds.
export function backendLogPath(): string {
  const port = backendPort();
  return port === DEFAULT_PORT
    ? "/tmp/teams-lite-server.log"
    : `/tmp/teams-lite-server-${port}.log`;
}

/// `TEAMS_LITE_BACKEND_LOG=-`: the backend writes to THIS process's own stdout and stderr
/// instead of a file. The container image sets it (packaging/docker/entrypoint.sh), because
/// there the file is inside a container nobody reads and `docker logs` — the one place a
/// reader of an image looks — would hold the launcher's three lines and none of the backend's:
/// which bus it found, why sign-in failed, what it refused. Any other value, or none, keeps the
/// file, which is right wherever a person runs `teams` in a terminal they are also using.
export const BACKEND_LOG_ENV = "TEAMS_LITE_BACKEND_LOG";

/// Does the backend write where this process writes?
export function backendLogsInherited(): boolean {
  return process.env[BACKEND_LOG_ENV]?.trim() === "-";
}

/// Where a reader finds what the backend said — named in every message that sends them there.
export function backendLogLocation(): string {
  return backendLogsInherited() ? "the backend's output above" : backendLogPath();
}

/// Are we running as a `bun build --compile` standalone binary? In that mode the
/// backend is embedded inside this executable (see embedded-backend.ts) rather
/// than sitting next to a source tree. Bun.embeddedFiles is populated only in a
/// compiled binary; under `bun run` it is empty.
export function isCompiledBinary(): boolean {
  const embedded = (globalThis as unknown as { Bun?: { embeddedFiles?: unknown[] } }).Bun
    ?.embeddedFiles;
  return Array.isArray(embedded) && embedded.length > 0;
}

/// The repo root, for a source checkout: launcher/src -> two levels up.
export function repoRoot(): string {
  return join(import.meta.dir, "..", "..");
}

/// Extract the embedded backend to a stable cache path and return it. The copy already
/// there is kept only when it IS the asset this binary carries — same bytes, not merely
/// the same byte count (see embedded-cache.ts, which says what that cost). An upgrade
/// therefore refreshes it, and an ordinary launch pays one hash of bytes it had to read
/// anyway.
async function extractEmbeddedBackend(): Promise<string> {
  const { default: bunfsPath } = await import("./embedded-backend");
  const bytes = new Uint8Array(await Bun.file(bunfsPath).arrayBuffer());

  const dir = join(homedir(), ".cache", "teams-lite");
  mkdirSync(dir, { recursive: true });
  const dest = join(dir, "server");
  const stamp = join(dir, ".server-id");
  const id = assetId(bytes);

  if (!existsSync(dest) || !cachedAssetIsCurrent(stamp, id)) {
    replaceFile(dest, bytes, 0o755);
    stampCachedAsset(stamp, id);
  }
  chmodSync(dest, 0o755);
  return dest;
}

/// Locate the backend binary. Compiled binary → extract the embedded copy.
/// Dev (`bun run`) → prefer release, fall back to debug in the source tree.
async function backendBinary(): Promise<string> {
  if (isCompiledBinary()) {
    return extractEmbeddedBackend();
  }

  const root = repoRoot();
  const release = join(root, "target", "release", "server");
  const debug = join(root, "target", "debug", "server");
  if (existsSync(release)) return release;
  if (existsSync(debug)) return debug;
  throw new Error(
    "backend binary not found — build it with: cargo build --release --bin server",
  );
}

/// Is something already listening on the backend port? (lets us attach to an
/// already-running server instead of spawning a second one.)
async function portOpen(): Promise<boolean> {
  try {
    const sock = await Bun.connect({
      hostname: HOST,
      port: backendPort(),
      socket: { data() {}, open(s) { s.end(); } },
    });
    sock.end();
    return true;
  } catch {
    return false;
  }
}

/// Environment variable the backend reads to learn which binary IT can replace with a
/// new release (`LAUNCHER_BIN_ENV` in src/update.rs). Kept in step with the Rust side by
/// name; the value is ours to state, because we are the only process that knows it.
export const LAUNCHER_BIN_ENV = "TEAMS_LITE_LAUNCHER_BIN";

/// Environment variable that PINS the backend's write token, so the parent hands the
/// same value to the backend and to the frontend it serves (`WRITE_TOKEN_ENV` in
/// src/bin/server.rs).
export const WRITE_TOKEN_ENV = "TEAMS_LITE_WRITE_TOKEN";

/// A write token of our own, for the backend we are about to spawn.
///
/// Two 128-bit UUIDs, hex, mirroring `mint_write_token` on the Rust side. It is minted
/// HERE rather than left to the backend for one reason: a backend that mints its own
/// PUBLISHES it, to one file per machine — so a second instance would overwrite the
/// token of the first, and the first one's frontend would then be handed a token its own
/// backend refuses. Reads would keep working while every send came back refused, which
/// on the always-on service means the user's phone quietly losing the ability to answer
/// anybody. A pinned token is never published (see `write_token` in src/bin/server.rs),
/// so an instance started by `teams` costs the service nothing.
export function mintWriteToken(): string {
  return `${crypto.randomUUID()}${crypto.randomUUID()}`.replace(/-/g, "");
}

/// What we add to the backend's environment, and why each entry is there.
///
/// `TEAMS_LITE_LAUNCHER_BIN` is what makes an in-app update possible at all: this
/// binary IS the release asset, so the backend can download a newer one and rename it
/// over this path. It is set only for a COMPILED binary — under `bun run` the path is
/// bun itself, and swapping that is not an update of teams-lite — which is also what
/// makes the variable the backend's proof that a launcher is there to restart the app
/// afterwards (see launcher/src/update.ts).
///
/// `TEAMS_LITE_WRITE_TOKEN` pins the write lock's token (see `mintWriteToken`), and the
/// caller hands the same value to the web server it runs in-process.
///
/// `TEAMS_LITE_LAUNCHER` says a launcher OWNS this backend and can re-spawn it, which is
/// what lets Settings › This app offer a restart (`restart` in the Rust crate, over the
/// `backend_restart` event). It is set for every spawn, compiled or not — unlike
/// `TEAMS_LITE_LAUNCHER_BIN`, which names a binary an update may replace and so exists only
/// in a compiled run. A backend the user started by hand has neither, and the app then says
/// so instead of exiting into nothing.
///
/// `TEAMS_NO_IDLE_EXIT` (dev only) keeps a spawned backend up across the frontend
/// reloads a hot-reloading session produces.
export function backendEnv(keepAlive: boolean, writeToken?: string): Record<string, string> {
  const env: Record<string, string> = {};
  if (isCompiledBinary()) env[LAUNCHER_BIN_ENV] = process.execPath;
  env[LAUNCHER_ENV] = "1";
  if (writeToken) env[WRITE_TOKEN_ENV] = writeToken;
  if (keepAlive) env.TEAMS_NO_IDLE_EXIT = "1";
  return env;
}

/// Environment variable that tells the backend a launcher can restart it (`LAUNCHER_ENV`
/// in src/restart.rs). Kept in step with the Rust side by name, like the two above.
export const LAUNCHER_ENV = "TEAMS_LITE_LAUNCHER";

/// What a backend we merely ATTACHED to answers when asked to restart. It is another
/// launcher's child, or a systemd unit's process: killing it would take down an app we did
/// not start, and the process that owns it is the one that can put it back.
export const ATTACHED_BACKEND_CANNOT_RESTART =
  "this launcher attached to a backend it did not start, so it cannot restart it";

export type BackendHandle = {
  /// Stop the backend we spawned GRACEFULLY, resolving once it is really gone — not merely
  /// signalled (see {@link stopGracefully}).
  ///
  /// Graceful, because a stop is no longer instant on the backend's side: on SIGTERM it leaves
  /// the call it holds and hands its calling registration back, and a KILL would abandon the
  /// call — a participant gone silent for everybody else, and Teams offering the user's calls
  /// to an endpoint that is gone for up to an hour. And AWAITED, because an in-app update needs
  /// the difference: a signalled process still holds port 19420 for a moment, and the new
  /// build's first act is to ask whether something is already listening there — a yes makes it
  /// ATTACH to the backend we just stopped, and the app comes back on the new UI with a dead
  /// backend behind it. Resolves immediately when we only attached to somebody else's backend,
  /// which is not ours to stop.
  stop: () => Promise<void>;
  /// The write token we pinned for the backend we SPAWNED, which the caller must hand to
  /// its own frontend (see `mintWriteToken`). Null when we merely attached to a backend
  /// somebody else started: that one published its own token, and the frontend reads it
  /// from the file the way it always did.
  writeToken: string | null;
  /// Stop the backend we spawned and start it again, resolving once it is listening.
  ///
  /// What Settings › This app asks for: the backend goes, our web server does NOT, so the
  /// page stays served and only its socket blinks. The new child keeps the SAME pinned
  /// write token — a fresh one would be refused by the page's copy until somebody reloaded,
  /// which is the exact failure the pinned token exists to prevent.
  ///
  /// Rejects for a backend we attached to (`ATTACHED_BACKEND_CANNOT_RESTART`), and for a
  /// new child that never binds — the caller says so rather than leaving the user with a
  /// page that quietly has nothing behind it.
  restart: () => Promise<void>;
  /// Whether this launcher is going away for good — a stop signal has reached it. A restart or
  /// an update whose own graceful stop such a signal landed in must spawn nothing after it: the
  /// stop is seconds long now (the backend leaves its call), and a child spawned behind the
  /// signal is one nothing will ever stop.
  leaving: () => boolean;
};

/// Ensure the backend is running. If a server is already up, attach to it and
/// don't manage its lifecycle. Otherwise spawn one and return a stop() handle.
///
/// `keepAlive` (dev use) starts the spawned backend with `TEAMS_NO_IDLE_EXIT`, so
/// it survives frontend disconnects and only stops when we kill it — handy when
/// the browser tab is closed/reloaded during development. It has no effect when
/// we merely attach to a backend someone else already started.
export async function ensureBackend(opts: { keepAlive?: boolean } = {}): Promise<BackendHandle> {
  if (await portOpen()) {
    // Someone else owns it: neither ours to stop, nor ours to wait for, and its token is
    // its own — published to the file our frontend already reads.
    return {
      stop: () => Promise.resolve(),
      writeToken: null,
      restart: () => Promise.reject(new Error(ATTACHED_BACKEND_CANNOT_RESTART)),
      leaving: () => false,
    };
  }

  const bin = await backendBinary();
  const writeToken = mintWriteToken();
  // `let`, because a restart replaces it — and `stop` below reads it through this binding
  // rather than capturing one child, so the exit handlers registered ONCE always stop
  // whichever backend is current.
  let proc = await startBackend(bin, opts.keepAlive === true, writeToken);
  /// A replacement child from the moment it is SPAWNED until it is listening and becomes `proc`.
  /// The wait for its port takes up to a minute, and a launcher stopped inside it must still reach
  /// the child it just started — the exit hooks look at this as well as at `proc`.
  let pending: Subprocess | null = null;
  /// WHICH child is the current one. A deliberate stop bumps it BEFORE it kills, so the
  /// dying child's own exit reads as superseded and the supervisor stands down — see
  /// {@link superviseBackend} for why that ordering is the whole of it.
  let generation = 0;
  /// Set once this launcher is going away for good — by a stop signal, through the exit hooks.
  /// Read by the supervisor, by a restart and by an in-app update, none of which may spawn
  /// anything after it.
  let gone = false;
  const stop = async (): Promise<void> => {
    // BEFORE the signal, so the dying child's own exit reads as superseded and the supervisor
    // does not race a respawn of its own.
    generation += 1;
    const child = proc;
    await stopGracefully({
      terminate: () => child.kill("SIGTERM"),
      kill: () => child.kill(9),
      exited: () => child.exited.then(() => {}),
      wait: (ms) => Bun.sleep(ms),
      log: (message) => console.error(message),
    });
  };
  stopChildOnExit({
    leaving: () => {
      gone = true;
    },
    stop,
    kill: () => {
      for (const child of [proc, pending]) {
        try {
          child?.kill(9);
        } catch {}
      }
    },
    alive: () =>
      [proc, pending].some((child) => child !== null && child.exitCode === null && child.signalCode === null),
  });
  /// Spawn a replacement and re-arm the watch over it. One place, so the first child and
  /// every one after it are supervised identically.
  const spawnSupervised = async (): Promise<void> => {
    if (gone) return;
    const child = await startBackend(bin, opts.keepAlive === true, writeToken, (spawned) => {
      pending = spawned;
    });
    pending = null;
    if (gone) {
      // A stop arrived while this child was coming up: it is not ours to keep.
      try {
        child.kill("SIGTERM");
      } catch {}
      return;
    }
    proc = child;
    generation += 1;
    watch(proc, generation);
  };
  const watch = (child: Subprocess, mine: number) => {
    void superviseBackend({
      exited: () => child.exited.then(() => {}),
      gone: () => gone,
      current: () => mine === generation,
      start: spawnSupervised,
      log: (message) => console.error(message),
      wait: (ms) => Bun.sleep(ms),
      logPath: backendLogLocation(),
    });
  };
  watch(proc, generation);
  return {
    stop,
    writeToken,
    // ONE AT A TIME. A stop takes seconds now (the backend leaves its call), so a second press —
    // the phone's, while the laptop's restart is still stopping — would otherwise run a second
    // stop-and-spawn beside the first: two children racing for one port, and the one that bound
    // first held by neither `proc` nor `pending`, so nothing could ever stop it.
    restart: oneAtATime(async () => {
      // `stop` bumps the generation first, so the child it stops does not read its own
      // death as a crash and race this replacement — and it resolves once that child is
      // reaped, because a signalled process still holds its port for a moment and the
      // replacement's own bind would fail on it. A stop signal that landed meanwhile asked for
      // the app to go, and `spawnSupervised` spawns nothing after it.
      await stop();
      await spawnSupervised();
    }),
    leaving: () => gone,
  };
}

/// How long to wait before trying again when the backend will not START.
///
/// Only reached when `startBackend` THREW, which it does after waiting 60 s for the port —
/// so this is the pause between minute-long attempts rather than a busy loop's throttle.
const RESTART_BACKOFF_MS = 5_000;

/**
 * Why the backend is SUPERVISED, and what the generation counter is for.
 *
 * **This is a real outage, not a hypothetical.** `launch()` spawned the backend as a child
 * and never looked at it again: waiting for its exit was only ever done by a restart
 * or an update, both of which ask for the exit. So a backend that died for any other reason —
 * a crash, an OOM kill, an idle exit whose keepalive had dropped — left this process serving
 * the app against a dead socket, for ever. Measured on the always-on EC2 instance: the web
 * front had been up for **11 hours 40 minutes** with no backend behind it, so every page said
 * "Backend lost" and nothing on the machine was going to change that. The app cannot bring
 * itself back — the backend is our child and only we can spawn another — so nobody could.
 *
 * The GENERATION is what keeps that apart from an exit somebody ASKED for, and the ordering is
 * the whole of it: `stop()` bumps the counter and then signals, so by the time the dying child's
 * `exited` resolves its own generation is stale and the supervisor stands down. Without that,
 * a Settings restart and an in-app update would each race a respawn of their own — two
 * backends fighting for one port, which is worse than the bug being fixed.
 *
 * `gone` covers the one exit that must never be followed by a respawn: this launcher itself
 * going away, which is the update's own last step.
 *
 * An ATTACHED backend gets none of this. It is somebody else's child, we cannot wait for it
 * and we must not start a second one on its port — `ensureBackend`'s attach branch returns a
 * handle whose `stop` resolves at once, and a supervisor over that would respawn in a
 * tight loop against a backend that is perfectly healthy.
 */
export type SupervisorDeps = {
  /** Resolves when the child being watched has really gone. */
  exited: () => Promise<void>;
  /** Whether this launcher is going away for good. */
  gone: () => boolean;
  /** Whether the child that just exited was still the CURRENT one — false once a restart
   *  or an update has bumped past it. */
  current: () => boolean;
  /** Start a replacement, and re-arm the watch over it. Throws when it will not come up. */
  start: () => Promise<void>;
  log: (message: string) => void;
  wait: (ms: number) => Promise<void>;
  /** Where the backend's own output went, so the line that says it died can point at it. */
  logPath: string;
};

/**
 * Watch one backend child and bring it back if it dies on its own.
 *
 * Injected for the reason {@link RelaunchDeps} is: the ORDER and the refusals are the whole of
 * the behaviour, and a test that spawns real processes to check them would be slower and would
 * pin how it is spelled rather than what it does.
 */
export async function superviseBackend(deps: SupervisorDeps): Promise<void> {
  await deps.exited();
  for (;;) {
    // Both refusals, re-read on every pass: a stop can land while a start is in flight.
    if (deps.gone() || !deps.current()) return;
    deps.log(
      `[backend] the backend exited on its own — starting it again. See ${deps.logPath}`,
    );
    try {
      await deps.start();
      deps.log("[backend] back up");
      return;
    } catch (e) {
      // `startBackend` already waits a minute for the port, so a failing attempt is not a
      // tight loop — but it THROWS, and an unhandled rejection here would take the web
      // server down with it. Said and retried: giving up is what left the app serving pages
      // against nothing for eleven hours.
      deps.log(`[backend] could not start it: ${e instanceof Error ? e.message : String(e)}`);
      await deps.wait(RESTART_BACKOFF_MS);
    }
  }
}

/// Spawn one backend and resolve once it is listening.
///
/// Separate from `ensureBackend` because a restart does exactly this again: the wait for the
/// port is the whole of "is it up", and a second copy of it would be a second answer to that
/// question (the broker handshake can take seconds, so a shorter wait somewhere would report
/// a healthy start as a failure).
async function startBackend(
  bin: string,
  keepAlive: boolean,
  writeToken: string,
  onSpawn: (child: Subprocess) => void = () => {},
): Promise<Subprocess> {
  // APPENDED, and one file per PORT.
  //
  // Both halves were got wrong and both cost a diagnosis. A `BunFile` handed to `spawn`
  // TRUNCATES and each writer keeps its own offset, so two backends on one machine — a
  // `teams` run beside the always-on service, which is the arrangement AGENTS.md § Running
  // the released build describes — spliced their output into one file: whole lines from one
  // run overwritten mid-word by the other, and the same startup banner appearing at two
  // offsets as though it had happened twice. Reading a real outage out of that took an hour.
  // Per port, so the two never share; appended, so a RESTART does not erase the reason the
  // last backend died, which is the one thing worth having after one.
  const log = backendLogsInherited() ? "inherit" : openSync(backendLogPath(), "a");
  const proc: Subprocess = spawn([bin], {
    stdout: log,
    stderr: log,
    stdin: "ignore",
    env: { ...process.env, ...backendEnv(keepAlive, writeToken) },
  });
  onSpawn(proc);

  // wait for it to bind (auth broker handshake can take a few seconds)
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) {
      throw new Error(
        `backend exited (code ${proc.exitCode}). See ${backendLogLocation()}`,
      );
    }
    if (await portOpen()) return proc;
    await Bun.sleep(300);
  }
  try { proc.kill(9); } catch {}
  throw new Error(`backend still not listening after 60s. See ${backendLogLocation()}`);
}

/// How long the launcher waits for its backend to EXIT after a SIGTERM, before it KILLs it.
///
/// The backend spends that time leaving the call it holds and handing its calling registration
/// back (`leave_calls_for_exit` in src/bin/server.rs), within a budget of its own — 5 s plus a
/// quarter-second flush. This is the next link of a chain that has to CLOSE: longer than the
/// backend's own stop, and shorter than every supervisor's — systemd's `TimeoutStopSec`, Docker's
/// default grace and the chart's. `the_stop_budgets_close` (src/bin/server.rs) reads this line.
export const BACKEND_STOP_GRACE_MS = 7_000;

/// A second stop signal this soon after the first is the SAME stop arriving twice, not a
/// person asking again. Two sources make that the rule rather than the exception: a unit's
/// `KillMode=control-group` TERMs this launcher and its backend together, and a terminal's
/// Ctrl+C reaches every process of the group. Acting on the duplicate would KILL the backend
/// in the middle of its hangup. A signal later than this is a human's second ask, and is obeyed.
export const REPEAT_SIGNAL_MS = 1_000;

/**
 * `run`, at most one at a time: a call made while one is in flight shares that one rather than
 * starting another, and a call after it has settled runs again.
 */
export function oneAtATime(run: () => Promise<void>): () => Promise<void> {
  let running: Promise<void> | null = null;
  return () =>
    (running ??= run().finally(() => {
      running = null;
    }));
}

/** What one graceful stop needs, injected so the sequence is pinned by a test with no process. */
export type GracefulStopDeps = {
  /** Ask the child to stop (SIGTERM). */
  terminate: () => void;
  /** Make it stop (SIGKILL). */
  kill: () => void;
  /** Resolves once the child has really gone. */
  exited: () => Promise<void>;
  wait: (ms: number) => Promise<void>;
  log: (message: string) => void;
  /** Defaults to {@link BACKEND_STOP_GRACE_MS}. */
  graceMs?: number;
};

/**
 * Stop a backend the way it can leave its call: SIGTERM, wait up to the grace for it to exit,
 * and only then KILL it — and resolve once it is gone either way.
 *
 * It used to be `kill(9)` and nothing else, which was safe only while a stop had nothing to do.
 * It has: a backend on SIGTERM hangs up the call the user is in and hands the calling
 * registration back, and a KILL skips both — the people on the call are left with a participant
 * gone silent, and Teams goes on offering the user's calls to an endpoint that is gone.
 */
export async function stopGracefully(deps: GracefulStopDeps): Promise<void> {
  const grace = deps.graceMs ?? BACKEND_STOP_GRACE_MS;
  const exited = deps.exited().then(() => true as const);
  try {
    deps.terminate();
  } catch {}
  const inTime = await Promise.race([exited, deps.wait(grace).then(() => false as const)]);
  if (inTime) return;
  deps.log(`[backend] the backend did not stop within ${grace / 1000} s of SIGTERM — killing it`);
  try {
    deps.kill();
  } catch {}
  await exited;
}

/** What a stop signal reaching THIS launcher does, injected for the same reason. */
export type ExitSignalDeps = {
  now: () => number;
  /** This launcher is going away for good: no supervisor may respawn behind it. */
  leaving: () => void;
  /** The graceful stop of the current backend ({@link stopGracefully}). */
  stop: () => Promise<void>;
  /** KILL the current backend at once. */
  kill: () => void;
  exit: () => void;
  log: (message: string) => void;
};

/**
 * The handler for SIGINT, SIGTERM and SIGHUP on the launcher.
 *
 * The FIRST signal stops the backend gracefully and then exits; a repeat within
 * {@link REPEAT_SIGNAL_MS} is the same stop delivered twice and is ignored; a later one is a
 * person asking twice — a second Ctrl+C — and KILLs the backend at once, because a stop the user
 * wants faster than the backend's own budget is theirs to have.
 */
export function exitSignalHandler(deps: ExitSignalDeps): (signal: string) => void {
  let firstAt: number | null = null;
  return (signal) => {
    const now = deps.now();
    if (firstAt === null) {
      firstAt = now;
      deps.leaving();
      deps.log(`[backend] ${signal} — stopping the backend; it leaves any call first`);
      void deps
        .stop()
        .catch((e) => deps.log(`[backend] the backend did not stop cleanly: ${String(e)}`))
        .finally(() => deps.exit());
      return;
    }
    if (now - firstAt < REPEAT_SIGNAL_MS) return;
    deps.log(`[backend] ${signal} again — killing the backend now`);
    deps.kill();
    deps.exit();
  };
}

/// Stop the backend whenever this process goes away, for any reason.
///
/// Two hooks are needed, not one. A signal terminates the process WITHOUT running exit
/// handlers — and Ctrl+C on the launcher is the ordinary way to stop it — so the signals stop
/// the backend gracefully ({@link exitSignalHandler}). "exit" covers a normal return and an
/// uncaught error, and it is SYNCHRONOUS, so it can only KILL: it is the fallback for a backend
/// still alive when this process ends some other way. Without either a `--dev` backend, which
/// we start with idle-exit disabled, would survive as an orphan holding the port and the user's
/// account, and only a manual kill would clear it.
function stopChildOnExit(deps: {
  leaving: () => void;
  stop: () => Promise<void>;
  kill: () => void;
  alive: () => boolean;
}): void {
  process.on("exit", () => {
    if (deps.alive()) deps.kill();
  });
  const onSignal = exitSignalHandler({
    now: () => Date.now(),
    leaving: deps.leaving,
    stop: deps.stop,
    kill: deps.kill,
    exit: () => process.exit(0),
    log: (message) => console.error(message),
  });
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"] as const) {
    process.on(signal, () => onSignal(signal));
  }
}
