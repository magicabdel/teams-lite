// The reading half of `bun run join-live -- --pair --toggle-mute`, tested with no meeting: what
// the receiver makes of the roster's `endpointState`, and the one line the run ends on — judged
// against the moments of the presses.
//
// The run itself needs both of the user's installs and the pinned meeting, which no test may
// have — so what a test CAN hold is that the reading is right: a moved mute is SEEN, a mute that
// never came back is HALF, and nothing moving is NOT SEEN, never a pass.

import { describe, expect, it } from "vitest";
import { EndpointMuteLog, muteRoundTrip } from "./mute-digest";

/** One roster participant — the user on two endpoints, the way the tenant names them. */
function me(sender: boolean | null, receiver: boolean | null = false): Record<string, unknown> {
  const state = (muted: boolean | null) =>
    muted === null ? { participantId: "p" } : { endpointState: { state: { isMuted: muted } } };
  return {
    details: { displayName: "Me" },
    endpoints: { "receiver-endpoint": state(receiver), "sender-endpoint": state(sender) },
  };
}

describe("EndpointMuteLog", () => {
  it("folds each endpoint's isMuted into the values it moved through, and names no id", () => {
    const log = new EndpointMuteLog();
    for (const sender of [false, false, true, true, false]) log.note(me(sender));
    expect(log.list()).toEqual([
      { endpoint: "endpoint 1", isMuted: [false] },
      { endpoint: "endpoint 2", isMuted: [false, true, false] },
    ]);
    expect(JSON.stringify(log.list())).not.toContain("sender-endpoint");
  });

  /** An endpoint that never stated a mute is a different answer from one that said `false`. */
  it("lists an endpoint that never stated a mute with no values", () => {
    const log = new EndpointMuteLog();
    log.note(me(null, null));
    expect(log.list()).toEqual([
      { endpoint: "endpoint 1", isMuted: [] },
      { endpoint: "endpoint 2", isMuted: [] },
    ]);
  });

  it("reads nothing out of a participant with no endpoint object", () => {
    const log = new EndpointMuteLog();
    log.note({ details: {} });
    log.note({ endpoints: [] });
    log.note({ endpoints: "x" });
    expect(log.list()).toEqual([]);
  });
});

describe("muteRoundTrip", () => {
  const e = (endpoint: string, isMuted: boolean[]) => ({ endpoint, isMuted });
  const both = { muted: true, unmuted: true };

  it("is SEEN when an endpoint went muted between the presses and back after them", () => {
    expect(
      muteRoundTrip(
        {
          beforeMute: [e("endpoint 1", [false]), e("endpoint 2", [false])],
          afterMute: [e("endpoint 1", [false]), e("endpoint 2", [false, true])],
          afterUnmute: [e("endpoint 1", [false]), e("endpoint 2", [false, true, false])],
        },
        both,
      ),
    ).toBe("SEEN — endpoint 2's isMuted went false → true → false, in step with the presses");
  });

  /** The pinned meeting may hold a colleague: one who joined muted and unmuted on their own folds
   *  to "true → false" over the run, and that must never pass for the sender's publish. */
  it("never reads a colleague's own unmute, at the wrong moment, as the round trip", () => {
    const colleague = {
      beforeMute: [e("endpoint 1", [true])],
      afterMute: [e("endpoint 1", [true])],
      afterUnmute: [e("endpoint 1", [true, false])],
    };
    expect(muteRoundTrip(colleague, both)).toBe(
      "NOT SEEN — no endpoint's isMuted moved with the presses; the sender's log says what it published",
    );
    // Muted and unmuted again all before the first press: nothing happened between the presses.
    const early = {
      beforeMute: [e("endpoint 1", [false, true, false])],
      afterMute: [e("endpoint 1", [false, true, false])],
      afterUnmute: [e("endpoint 1", [false, true, false])],
    };
    expect(muteRoundTrip(early, both)).toMatch(/^NOT SEEN/);
  });

  it("is HALF when the mute arrived and the unmute never did", () => {
    expect(
      muteRoundTrip(
        {
          beforeMute: [e("endpoint 2", [])],
          afterMute: [e("endpoint 2", [true])],
          afterUnmute: [e("endpoint 2", [true])],
        },
        both,
      ),
    ).toBe("HALF — endpoint 2 showed the mute and never showed the unmute");
  });

  /** A press the sender's stage never showed asked the service nothing, so nothing can pass. */
  it("is NOT SEEN whatever the roster says when a press never landed", () => {
    const moved = {
      beforeMute: [e("endpoint 2", [false])],
      afterMute: [e("endpoint 2", [false, true])],
      afterUnmute: [e("endpoint 2", [false, true, false])],
    };
    for (const pressed of [
      { muted: false, unmuted: true },
      { muted: true, unmuted: false },
    ]) {
      expect(muteRoundTrip(moved, pressed)).toBe(
        "NOT SEEN — the sender's stage never showed the press, so the service was asked nothing",
      );
    }
    expect(muteRoundTrip({ beforeMute: [], afterMute: [], afterUnmute: [] }, both)).toMatch(/^NOT SEEN/);
  });
});
