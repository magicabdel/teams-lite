// A stop LETS THE BACKEND LEAVE ITS CALL, and it did not.
//
// The launcher used to stop its backend with `kill(9)` and nothing else. That was safe while a
// stop had nothing to do, and it has: on SIGTERM the backend hangs up the call the user is in
// and hands its calling registration back (`leave_calls_for_exit` in src/bin/server.rs). A KILL
// skips both — the people on the call are left with a participant gone silent, and Teams goes
// on offering the user's calls to an endpoint that is gone. What is pinned here:
//
//   * the graceful stop's order — TERM, wait up to the grace, KILL only after it, resolve once
//     the child is gone either way;
//   * the launcher's own signals — the first stops gracefully and exits, a duplicate within a
//     second is ignored (a unit's control-group TERM and a terminal's process-group Ctrl+C both
//     deliver one), and a later one is a person asking twice, and KILLs at once.
//
// The budget this grace belongs to is pinned across the process boundary by
// `the_stop_budgets_close` in src/bin/server.rs, which reads this module's constants.
import { describe, expect, test } from "bun:test";
import {
  BACKEND_STOP_GRACE_MS,
  REPEAT_SIGNAL_MS,
  exitSignalHandler,
  oneAtATime,
  stopGracefully,
  type ExitSignalDeps,
  type GracefulStopDeps,
} from "./backend";

/** A child that records what it was sent, and exits when `exitOn` arrives (or never). */
function child(exitOn: "SIGTERM" | "SIGKILL" | "never") {
  const sent: string[] = [];
  let exit!: () => void;
  const gone = new Promise<void>((resolve) => {
    exit = resolve;
  });
  // The timer is ours, so the grace can be passed by hand rather than by waiting seven seconds.
  let elapse!: () => void;
  const waited: number[] = [];
  const deps: GracefulStopDeps = {
    terminate: () => {
      sent.push("SIGTERM");
      if (exitOn === "SIGTERM") exit();
    },
    kill: () => {
      sent.push("SIGKILL");
      if (exitOn !== "never") exit();
    },
    exited: () => gone,
    wait: (ms) =>
      new Promise((resolve) => {
        waited.push(ms);
        elapse = resolve;
      }),
    log: () => {},
  };
  return { deps, sent, waited, elapse: () => elapse(), reap: () => exit() };
}

describe("stopGracefully", () => {
  test("asks with SIGTERM and never KILLs a backend that leaves in time", async () => {
    const { deps, sent, waited } = child("SIGTERM");
    await stopGracefully(deps);
    expect(sent).toEqual(["SIGTERM"]);
    expect(waited).toEqual([BACKEND_STOP_GRACE_MS]);
  });

  test("KILLs only once the grace is over, and resolves once the child is gone", async () => {
    const { deps, sent, elapse } = child("SIGKILL");
    let resolved = false;
    const stopping = stopGracefully(deps).then(() => {
      resolved = true;
    });
    await Bun.sleep(0);
    expect(sent).toEqual(["SIGTERM"]);
    expect(resolved).toBe(false);
    elapse();
    await stopping;
    expect(sent).toEqual(["SIGTERM", "SIGKILL"]);
  });

  // The KILL is sent at once, and the stop still waits for the kernel to REAP the child: a
  // relaunch's next step binds the backend's port, and a child that is signalled but not yet
  // gone still holds it.
  test("after the KILL it still waits for the child to be reaped", async () => {
    const { deps, sent, elapse, reap } = child("never");
    let resolved = false;
    const stopping = stopGracefully(deps).then(() => {
      resolved = true;
    });
    await Bun.sleep(0);
    elapse();
    await Bun.sleep(0);
    expect(sent).toEqual(["SIGTERM", "SIGKILL"]);
    expect(resolved).toBe(false);
    reap();
    await stopping;
    expect(resolved).toBe(true);
  });

  test("the grace is longer than the backend's own stop, and a repeat window is shorter", () => {
    // The backend leaves within 5 s plus a quarter-second flush (SHUTDOWN_BUDGET +
    // RESTART_ANSWER_GRACE): a KILL inside that cuts its hangup short.
    expect(BACKEND_STOP_GRACE_MS).toBeGreaterThan(5_250);
    expect(REPEAT_SIGNAL_MS).toBeLessThan(BACKEND_STOP_GRACE_MS);
  });
});

/** A signal handler on a clock of our own, recording what it did. */
function signals() {
  const did: string[] = [];
  let now = 0;
  let finish!: () => void;
  const stopped = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const deps: ExitSignalDeps = {
    now: () => now,
    leaving: () => did.push("leaving"),
    stop: () => {
      did.push("stop");
      return stopped;
    },
    kill: () => did.push("kill"),
    exit: () => did.push("exit"),
    log: () => {},
  };
  return {
    handle: exitSignalHandler(deps),
    did,
    advance: (ms: number) => {
      now += ms;
    },
    finish: () => finish(),
  };
}

describe("exitSignalHandler", () => {
  test("the first signal stops the backend gracefully, and exits once it is gone", async () => {
    const { handle, did, finish } = signals();
    handle("SIGTERM");
    // `leaving` FIRST, so no supervisor respawns the backend this stop is taking down.
    expect(did).toEqual(["leaving", "stop"]);
    finish();
    await Bun.sleep(0);
    expect(did).toEqual(["leaving", "stop", "exit"]);
  });

  test("a duplicate within the repeat window is the same stop, and is ignored", async () => {
    const { handle, did, advance } = signals();
    handle("SIGTERM");
    advance(REPEAT_SIGNAL_MS - 1);
    handle("SIGINT");
    handle("SIGHUP");
    expect(did).toEqual(["leaving", "stop"]);
  });

  test("a later signal is a person asking twice, and KILLs at once", () => {
    const { handle, did, advance } = signals();
    handle("SIGINT");
    advance(REPEAT_SIGNAL_MS);
    handle("SIGINT");
    expect(did).toEqual(["leaving", "stop", "kill", "exit"]);
  });
});

describe("oneAtATime", () => {
  // Two restarts pressed a second apart — a laptop and a phone — while the first is still stopping
  // a backend that is leaving its call: the second must share the first, or two children race for
  // one port and the one that loses the race is the one nothing can stop.
  test("a second call while one is in flight shares it, and a later one runs again", async () => {
    let runs = 0;
    let finish!: () => void;
    const restart = oneAtATime(
      () =>
        new Promise<void>((resolve) => {
          runs += 1;
          finish = resolve;
        }),
    );
    const first = restart();
    const second = restart();
    expect(runs).toBe(1);
    finish();
    await Promise.all([first, second]);
    const third = restart();
    expect(runs).toBe(2);
    finish();
    await third;
  });
});

// The wrapper above only helps where it is USED: the handle's restart must go through it, or two
// presses still race two stop-and-spawns for one port.
test("the backend handle's restart is one at a time", async () => {
  const source = await Bun.file(new URL("./backend.ts", import.meta.url)).text();
  expect(source).toContain("    restart: oneAtATime(async () => {\n");
});
