import { describe, it, expect } from "vitest";
import {
  callFailureMessage,
  captureDroppedMessage,
  captureRefusedMessage,
  renegotiationRefusedMessage,
} from "./call-failure";
import { CaptureUnavailableError, MicrophoneUnavailableError, type SendKind } from "./call-media";

// What the user is told when a call, a join or a capture did not happen. The rule is the
// one ./send-failure.test.ts pins for the other outward action: every sentence says what
// did not happen, and none of them hands the user a word written for whoever holds the
// socket.

describe("callFailureMessage", () => {
  it("always answers one non-empty sentence", () => {
    for (const raw of ["", "boom", "not connected", "call_place: no such call"]) {
      const message = callFailureMessage(new Error(raw));
      expect(message.length).toBeGreaterThan(0);
    }
  });

  it("still says something when the failure carried no words", () => {
    // The service really does answer 400 with an empty body, and a silent notice would
    // leave the user's click looking like it worked.
    expect(callFailureMessage(new Error(""))).toMatch(/nothing said why/);
    expect(callFailureMessage(new Error("call_join: "))).toMatch(/nothing said why/);
  });

  it("explains a refused microphone rather than reporting it", () => {
    // The common failure, and not a bug: the browser asks once and the answer sticks.
    const message = callFailureMessage(new MicrophoneUnavailableError(new Error("NotAllowedError")));
    expect(message).toMatch(/microphone/i);
    expect(message).toMatch(/Allow it for this site/);
  });

  it("blames the ADDRESS rather than a permission when the origin is insecure", () => {
    // Measured on Brave over NetBird, at http://100.x.y.z:19440: there is no
    // `navigator.mediaDevices` outside a secure context, so the open fails with a TypeError
    // and the app said "Allow it for this site" — permissions that were never the cause and
    // could never be the fix. Both halves are asserted, because the sentence has to name the
    // one thing the reader can change AND stop naming the one they cannot.
    const message = callFailureMessage(
      new MicrophoneUnavailableError(new Error("mediaDevices is undefined"), true),
    );
    expect(message).toMatch(/http:\/\//);
    expect(message).toMatch(/https/);
    expect(message).not.toMatch(/Allow it for this site/);

    // The camera and the screen lose the same capability at the same address, and each
    // names its own — "open a camera" and "capture a screen" are not one sentence.
    expect(
      callFailureMessage(new CaptureUnavailableError("camera", new Error("undefined"), true)),
    ).toMatch(/camera/);
    expect(
      callFailureMessage(new CaptureUnavailableError("screen", new Error("undefined"), true)),
    ).toMatch(/screen/);

    // And a real refusal on a secure page is untouched: the flag is the whole difference.
    expect(
      callFailureMessage(new CaptureUnavailableError("camera", new Error("NotAllowedError"))),
    ).not.toMatch(/http:\/\//);
  });

  it("turns the socket's own words into what they cost the call", () => {
    // This IS the sentence that used to float over the chat list: two words, naming
    // neither what did not happen nor what to do.
    for (const raw of ["not connected", "connection closed", "closed"]) {
      expect(callFailureMessage(new Error(raw))).toMatch(/backend is not reachable/);
    }
  });

  it("says a timeout is worth retrying", () => {
    expect(callFailureMessage(new Error("timeout: call_prepare"))).toMatch(/did not answer/);
  });

  it("names the reload a dead write token needs, and never the token", () => {
    const raw = "call_place: this method needs the write token";
    const message = callFailureMessage(new Error(raw));
    expect(message).toMatch(/Reload the app/);
    expect(message).not.toMatch(/token/i);
  });

  it("says a read-only backend refuses, without naming its variable", () => {
    const message = callFailureMessage(new Error("refused: TEAMS_LITE_READ_ONLY=1 is set"));
    expect(message).toMatch(/read-only/);
    expect(message).not.toMatch(/TEAMS_LITE_READ_ONLY/);
  });

  it("points a broken sign-in at itself", () => {
    expect(callFailureMessage(new Error("could not acquire a chat token: keyring"))).toMatch(
      /Sign-in is broken/,
    );
  });

  it("drops the RPC name off a backend refusal and keeps the rest word for word", () => {
    expect(callFailureMessage(new Error("call_prepare: that call is not ringing"))).toBe(
      "that call is not ringing",
    );
    expect(callFailureMessage(new Error("call_offer_media: the service refused it"))).toBe(
      "the service refused it",
    );
  });

  /** The backend reached, and ITS calling registration not up yet. The sentence says what it
   *  costs and that the machine mends it on its own — and an older backend's wording, which
   *  went on to send the user to a switch that no longer exists, is told the same thing. */
  it("says calls connect on their own when the calling registration is not up", () => {
    const sentence =
      "Calls are not connected on this machine yet. They connect on their own — try again in a moment.";
    for (const raw of [
      // Today's backend, word for word (the dash is an em dash).
      "call_prepare: calling is not connected yet — this machine registers for calls on its own once it can; try again in a moment",
      // The same, without the RPC's name in front.
      "calling is not connected yet — this machine registers for calls on its own once it can; try again in a moment",
      // The bare phrase an older mock and an older backend opened with.
      "call_prepare: calling is not connected yet",
      // An older backend's tail. Its literal sentence is deliberately not spelled here: the
      // crate scans web/src for it, because that advice must not survive anywhere — and what
      // this asserts is that any tail after the shared phrase is replaced, not that one is.
      "call_prepare: calling is not connected yet — open Settings, then try again",
    ]) {
      const message = callFailureMessage(new Error(raw));
      expect(message).toBe(sentence);
      expect(message).not.toMatch(/Settings|call_prepare/);
    }
  });

  /** The SOCKET's `not connected` is a different failure — the page could not reach its
   *  backend at all — and it keeps its own sentence rather than being read as a calling
   *  registration that is down. */
  it("never reads the socket's own not-connected as the calling registration", () => {
    for (const raw of ["not connected", "connection closed", "closed"]) {
      expect(callFailureMessage(new Error(raw))).toBe(
        "The backend is not reachable, so nothing can be called from here.",
      );
    }
  });

  it("leaves a colon that is not an RPC name alone", () => {
    // A sentence may carry a colon of its own, and eating half of it would change what it
    // said. Only a leading snake_case word counts.
    for (const raw of [
      "the meeting starts at 10:30",
      "https://teams.microsoft.com refused it",
      "Camera: refused",
    ]) {
      expect(callFailureMessage(new Error(raw))).toBe(raw);
    }
  });

  it("keeps a failure nobody wrote a sentence for", () => {
    // An unrecognised failure is exactly the one the user has to be able to report.
    expect(callFailureMessage(new Error("ICE gathering produced no candidates"))).toBe(
      "ICE gathering produced no candidates",
    );
  });

  it("reads an opaque Event rather than stringifying it", () => {
    // A browser WebSocket failure arrives as an Event, which stringifies to "[object Event]".
    expect(callFailureMessage(new Event("error"))).toBe("connection error");
    expect(callFailureMessage({})).toBe("unknown error");
  });
});

describe("captureDroppedMessage", () => {
  it("names the capture and the one action left", () => {
    // Not a failure of anything the user did: the meeting accepted the section and then
    // dropped it, so the picture stops with no click behind it. A camera that switches
    // itself off in silence reads as this app losing their input.
    expect(captureDroppedMessage("camera")).toMatch(/dropped your camera/);
    expect(captureDroppedMessage("camera")).toMatch(/Turn it on again/);
    expect(captureDroppedMessage("screen")).toMatch(/screen share/);
    expect(captureDroppedMessage("screen")).toMatch(/Share it again/);
  });

  it("hands the user no word written for whoever holds the socket", () => {
    // The rule this whole module exists for. "The transceiver is stopped" is what the
    // browser says, and it reached a real user as the outcome of switching a camera off.
    for (const kind of ["camera", "screen"] as const) {
      expect(captureDroppedMessage(kind)).not.toMatch(/transceiver|sdp|section|m-line/i);
    }
  });
});

// A capture the meeting never accepted, which is what a screen share really met on this
// tenant. It is NOT the drop above and the difference is the advice: this app gave the drop's
// advice for a refusal, so the user shared again and met the same refusal in the same second.

describe("captureRefusedMessage", () => {
  it("says nothing was shown, and never to try the same thing again", () => {
    for (const kind of ["camera", "screen"] as const) {
      expect(captureRefusedMessage(kind)).toMatch(/would not accept/);
      expect(captureRefusedMessage(kind)).toMatch(/nothing was shown/);
      // The drop's own advice, which is the wrong advice here.
      expect(captureRefusedMessage(kind)).not.toMatch(/again\./);
    }
  });

  it("names the one client that can still do it", () => {
    // Sending is not verified against a real tenant, so the honest thing left to name is
    // real Teams — which is what § Joining a meeting already says about a shared screen.
    expect(captureRefusedMessage("screen")).toMatch(/in Teams/);
    expect(captureRefusedMessage("camera")).toMatch(/in Teams/);
  });

  it("is not the sentence a DROP gets", () => {
    // Two endings, two sentences. One text for both is how the wrong advice happened.
    for (const kind of ["camera", "screen"] as const) {
      expect(captureRefusedMessage(kind)).not.toBe(captureDroppedMessage(kind));
    }
  });
});

// The third way a capture ends with no click behind it, after a refusal and a drop: the
// meeting ANSWERED and the browser could not read the answer. It is the one that used to cost
// the whole call — a user shared their screen and lost the person they were talking to — so
// the sentence has one job beyond naming what stopped.

describe("renegotiationRefusedMessage", () => {
  it("says the call is still there, which is the half nothing else says", () => {
    // Everything the user can SEE at this moment says the opposite: the share stopped and an
    // error arrived, a second after they pressed share.
    expect(renegotiationRefusedMessage(["screen"])).toMatch(/still in the call/);
    expect(renegotiationRefusedMessage([])).toMatch(/still in the call/);
  });

  it("names what stopped, and both when both did", () => {
    expect(renegotiationRefusedMessage(["screen"])).toMatch(/your screen share stopped/);
    expect(renegotiationRefusedMessage(["camera"])).toMatch(/your camera stopped/);
    // One offer carries a camera and a screen, so both going off at once is a real state.
    const both = renegotiationRefusedMessage(["camera", "screen"]);
    expect(both).toMatch(/your camera and your screen share stopped/);
  });

  it("still answers one sentence when nothing of the user's was in the offer", () => {
    // A renegotiation of ours can carry nothing but the sections the far side asked for.
    // There is no capture to name and something still has to be said.
    const message = renegotiationRefusedMessage([]);
    expect(message.length).toBeGreaterThan(0);
    expect(message).not.toMatch(/your camera|your screen/);
  });

  it("hands the user no word written for whoever reads a console", () => {
    for (const released of [[], ["camera"], ["screen"], ["camera", "screen"]] as SendKind[][]) {
      expect(renegotiationRefusedMessage(released)).not.toMatch(
        /SessionDescription|transceiver|sdp|m-line|rollback/i,
      );
    }
  });
});
