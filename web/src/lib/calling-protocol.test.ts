import { describe, expect, it } from "vitest";
import { UNKNOWN_CALL_STATUS, type ActiveCall, type CallStatus } from "./call";
import {
  CALLING_PROTOCOL_NOTE,
  MUTE_PROTOCOL_CHOICES,
  callingProtocolOffered,
  liveCallKeepsItsProtocol,
} from "./calling-protocol";

function call(overrides: Partial<ActiveCall> = {}): ActiveCall {
  return {
    id: "call-1",
    direction: "outgoing",
    kind: "meeting",
    phase: "connected",
    conversation_id: "19:meeting_x@thread.v2",
    peer: "Design Sync",
    peer_mri: "",
    others: [],
    other_mris: [],
    in_lobby: false,
    waiting_in_lobby: 0,
    publishing: [],
    sending: [],
    muted: false,
    connected_at_ms: 1,
    end_reason: null,
    can_accept: false,
    can_send_media: true,
    can_hangup: true,
    ...overrides,
  };
}

function status(overrides: Partial<CallStatus> = {}): CallStatus {
  return { enabled: true, ready: true, call: null, protocol: { mute: "legacy" }, ...overrides };
}

describe("callingProtocolOffered", () => {
  it("offers the row on a backend that calls and states its protocol", () => {
    expect(callingProtocolOffered(status())).toBe(true);
  });

  /** A read-only backend never calls, so a protocol for its calls is a choice about nothing; a
   *  backend too old to state one has no setting. Neither draws a row that would change
   *  nothing — and nothing is offered before the backend has spoken at all. */
  it("offers nothing where the choice would change nothing", () => {
    expect(callingProtocolOffered(status({ enabled: false }))).toBe(false);
    expect(callingProtocolOffered(status({ protocol: undefined }))).toBe(false);
    expect(callingProtocolOffered(UNKNOWN_CALL_STATUS)).toBe(false);
  });
});

describe("the mute row's choices", () => {
  it("offers this app's existing flow first, as the default, and names what each does", () => {
    expect(MUTE_PROTOCOL_CHOICES.map((choice) => choice.value)).toEqual(["legacy", "endpoint-state"]);
    // Not "Measured": the legacy mute publish has never been measured on the tenant either.
    expect(MUTE_PROTOCOL_CHOICES[0]!.label).toBe("As before (default)");
    expect(MUTE_PROTOCOL_CHOICES[1]!.label).toBe("Publish it the way Teams does");
    for (const choice of MUTE_PROTOCOL_CHOICES) expect(choice.detail.length).toBeGreaterThan(0);
  });

  /** The microphone never depends on the publish, and the option that changes the publish must
   *  say so: a reader choosing "the way Teams does" is not choosing whether they can be heard. */
  it("says the microphone mutes the same either way", () => {
    expect(MUTE_PROTOCOL_CHOICES[1]!.detail).toContain("Your microphone mutes the same either way.");
  });

  it("says where the choice applies, and that it is this backend's alone", () => {
    expect(CALLING_PROTOCOL_NOTE).toContain("from now on");
    expect(CALLING_PROTOCOL_NOTE).toContain("Each teams-lite install on this machine keeps its own.");
  });
});

describe("liveCallKeepsItsProtocol", () => {
  /** A call keeps the protocol it started with, so the one moment the "from the next call" rule
   *  would surprise the reader — they change it during a call — is the one it is said. */
  it("says a live call keeps the way it started with when that differs", () => {
    const live = call({ protocol: { mute: "legacy" } });
    expect(liveCallKeepsItsProtocol(status({ call: live, protocol: { mute: "endpoint-state" } }))).toBe(
      "The call going now keeps the way it started with until it ends.",
    );
  });

  it("says nothing when they agree, with no live call, or with a call that states nothing", () => {
    const live = call({ protocol: { mute: "legacy" } });
    expect(liveCallKeepsItsProtocol(status({ call: live }))).toBeNull();
    expect(liveCallKeepsItsProtocol(status())).toBeNull();
    expect(
      liveCallKeepsItsProtocol(
        status({ call: call({ phase: "ended", protocol: { mute: "legacy" } }), protocol: { mute: "endpoint-state" } }),
      ),
    ).toBeNull();
    expect(liveCallKeepsItsProtocol(status({ call: call(), protocol: { mute: "endpoint-state" } }))).toBeNull();
  });
});
