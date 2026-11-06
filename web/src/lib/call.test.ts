import { describe, expect, it } from "vitest";
import {
  UNKNOWN_CALL_STATUS,
  callDurationLabel,
  CALL_END_ANSWERED_ELSEWHERE,
  CALL_END_CALLER_CANCELLED,
  CALL_END_FORWARDED,
  CALL_END_LEFT_RINGING,
  CALL_END_UNANSWERED,
  CALL_END_UNREACHABLE,
  CALL_END_VOICEMAIL,
  callEndLabel,
  callIsStillRinging,
  callPhaseLabel,
  callNamesAConversation,
  callPresenceLabel,
  callUnavailableReason,
  canJoinMeeting,
  canPlaceCall,
  conversationCallAction,
  conversationIsCallable,
  callShowsMuted,
  callStillHoldsMedia,
  isLive,
  lostCallMessage,
  UNMUTE_REFUSED_HERE,
  followMuteAnswer,
  followMuteFrame,
  microphoneShouldBeOpen,
  muteToResend,
  pendingMuteAfterFrame,
  unmuteRefusal,
  type PendingMute,
  isMeeting,
  isMeetingJoinLink,
  meetingAddressOf,
  meetingAddressOfThread,
  meetingUnavailableReason,
  type ActiveCall,
  type CallPhase,
  type CallRegistrationState,
  type CallStatus,
} from "./call";
import type { Conversation } from "./protocol";

function call(overrides: Partial<ActiveCall> = {}): ActiveCall {
  return {
    id: "call-1",
    direction: "incoming",
    kind: "call",
    phase: "ringing",
    conversation_id: "19:thread@thread.v2",
    peer: "Riley Carter",
    peer_mri: "8:orgid:riley",
    others: [],
    other_mris: [],
    in_lobby: false,
    waiting_in_lobby: 0,
    publishing: [],
    sending: [],
    muted: false,
    connected_at_ms: null,
    end_reason: null,
    can_accept: true,
    can_send_media: false,
    can_hangup: true,
    ...overrides,
  };
}

function status(overrides: Partial<CallStatus> = {}): CallStatus {
  return { enabled: true, ready: true, call: null, ...overrides };
}

describe("the unknown state", () => {
  /** Before the backend has spoken, nothing is known to be registered, so the client's own
   *  default says so. A hopeful `enabled` would tell the user their calls ring here while
   *  nothing is registered — the same rule the agent's mode switch follows. */
  it("claims nothing before the backend answers", () => {
    expect(UNKNOWN_CALL_STATUS.enabled).toBe(false);
    expect(UNKNOWN_CALL_STATUS.ready).toBe(false);
    expect(UNKNOWN_CALL_STATUS.call).toBeNull();
    expect(canPlaceCall(UNKNOWN_CALL_STATUS)).toBe(false);
  });
});

describe("why the registration is not up", () => {
  /** One sentence per state, each saying what is really going on — and, for the two the
   *  machine mends on its own, that it does. `ready` is what decides whether a call can
   *  start; the registration only decides which sentence explains a `false`. */
  const notReady = (state: CallRegistrationState, registrar_status: number | null = null) =>
    status({ ready: false, registration: { state, registrar_status } });

  it("says a sign-in outage mends itself", () => {
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      expect(reason(notReady("waiting_for_sign_in"))).toBe(
        "Calls connect on their own once sign-in works again.",
      );
    }
  });

  it("says an account with no calling service cannot take calls at all", () => {
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      expect(reason(notReady("unavailable"))).toBe(
        "This account's Teams directory names no calling service, so this machine cannot take calls.",
      );
    }
  });

  /** "starting" and "connecting" are one fact to the user: a connection on its way up. The
   *  sentence promises no number of seconds, because a network can be down for hours. */
  it("says a connection on its way up, and promises no time", () => {
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      for (const state of ["starting", "connecting"] as const) {
        const sentence = reason(notReady(state));
        expect(sentence).toBe("Connecting for calls…");
        // The ellipsis is the single character, not three full stops.
        expect(sentence.endsWith("…")).toBe(true);
        expect(sentence).not.toMatch(/second|minute|\d/i);
      }
    }
  });

  /** A registration that is up while `ready` is false is a connection the backend has not
   *  finished reporting — today's sentence. `refused` changes nothing yet (it comes with
   *  `ready: true` in the ordinary case), so it reads the same. */
  it("keeps today's sentence for a registration that is up but not ready", () => {
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      expect(reason(notReady("registered", 200))).toBe(
        "This machine is not registered for calls yet.",
      );
      expect(reason(notReady("refused", 403))).toBe(
        "This machine is not registered for calls yet.",
      );
    }
  });

  /** A backend too old to send the field, and one that sends `null`, are both today's
   *  backend: the sentence it always had. */
  it("keeps today's sentence when the backend says nothing about it", () => {
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      expect(reason(status({ ready: false }))).toBe("This machine is not registered for calls yet.");
      expect(reason(status({ ready: false, registration: null }))).toBe(
        "This machine is not registered for calls yet.",
      );
    }
  });

  /** A word a NEWER backend invents must not throw and must not be guessed at: it reads as
   *  the narrowest true thing, which is that this machine is not registered yet. So must a
   *  field whose shape is not the one promised at all. */
  it("reads a word it does not know as not registered yet, and never throws", () => {
    const strange = [
      { state: "rebooting", registrar_status: null },
      { state: "", registrar_status: null },
      { state: 7, registrar_status: null },
      "registered",
      42,
      {},
    ];
    for (const reason of [callUnavailableReason, meetingUnavailableReason]) {
      for (const registration of strange) {
        const odd = { ready: false, registration } as unknown as Partial<CallStatus>;
        expect(reason(status(odd))).toBe("This machine is not registered for calls yet.");
      }
    }
  });

  /** A read-only window has no registration to explain, so its own sentence wins whatever
   *  the registration says; and a ready machine in a call talks about the call, not the
   *  registration. The order is what the user can act on. */
  it("puts the window first and the live call last", () => {
    const off = {
      enabled: false,
      ready: false,
      registration: { state: "waiting_for_sign_in", registrar_status: null },
    } as const;
    expect(callUnavailableReason(status(off))).toBe(
      "This window cannot take calls, so it cannot place one.",
    );
    expect(meetingUnavailableReason(status(off))).toBe(
      "This window cannot take calls, so it cannot join a meeting.",
    );
    // Not ready AND in a call is a real state — a dropped calling socket does not end a live
    // call — and the registration is said first: it is the one that has to come back before
    // a second call could start at all.
    expect(
      callUnavailableReason(
        status({ ...notReady("connecting"), call: call({ phase: "connected" }) }),
      ),
    ).toBe("Connecting for calls…");
    // Ready, registered, and in a call: one call at a time, whatever the registration says.
    const busy = status({
      registration: { state: "registered", registrar_status: 200 },
      call: call({ phase: "connected" }),
    });
    expect(callUnavailableReason(busy)).toBe("This app holds one call at a time.");
    expect(meetingUnavailableReason(busy)).toBe("This app holds one call at a time.");
    // Ready and REFUSED changes nothing about a call that can be placed: it is informational.
    const refused = status({ registration: { state: "refused", registrar_status: 403 } });
    expect(callUnavailableReason(refused)).toBe("");
    expect(canPlaceCall(refused)).toBe(true);
  });

  /** No state sends the user to a switch, because there is none. */
  it("never names Settings", () => {
    const states: CallRegistrationState[] = [
      "starting",
      "waiting_for_sign_in",
      "unavailable",
      "connecting",
      "registered",
      "refused",
    ];
    for (const state of states) {
      expect(callUnavailableReason(notReady(state))).not.toMatch(/Settings|turn it on/i);
      expect(meetingUnavailableReason(notReady(state))).not.toMatch(/Settings|turn it on/i);
    }
  });
});

describe("whether a call is live", () => {
  it("counts every phase but the last", () => {
    const phases: CallPhase[] = ["ringing", "dialing", "connecting", "connected"];
    for (const phase of phases) expect(isLive(call({ phase }))).toBe(true);
    expect(isLive(call({ phase: "ended" }))).toBe(false);
    expect(isLive(null)).toBe(false);
  });

});

describe("whether a page's microphone still belongs to a call", () => {
  /** The microphone is the thing a mistake here leaves switched on — or takes away from an
   *  answer that is about to connect — so it is keyed on the CALL and never on the phase. */
  it("keeps it in every live phase of the call it was opened for, ringing included", () => {
    // Ringing is the answer still travelling: the backend's phase moves when the attach
    // answers, and a frame in that moment must not release the microphone under it.
    const phases: CallPhase[] = ["ringing", "dialing", "connecting", "connected"];
    for (const phase of phases) {
      expect(callStillHoldsMedia(call({ id: "call-1", phase }), "call-1")).toBe(true);
    }
  });

  it("releases it when the call is over, gone, or another call", () => {
    expect(callStillHoldsMedia(call({ id: "call-1", phase: "ended" }), "call-1")).toBe(false);
    // A backend that restarted holds no call at all.
    expect(callStillHoldsMedia(null, "call-1")).toBe(false);
    // ...or a different one, already ringing again.
    expect(callStillHoldsMedia(call({ id: "call-2", phase: "connected" }), "call-1")).toBe(false);
  });

  it("holds nothing when the page holds nothing", () => {
    expect(callStillHoldsMedia(call({ id: "call-1", phase: "connected" }), null)).toBe(false);
    expect(callStillHoldsMedia(null, null)).toBe(false);
  });

  /** What a failed Answer may promise: that call, still ringing, still answerable. */
  it("says a call is still ringing only for that call, ringing, and answerable", () => {
    expect(callIsStillRinging(call({ id: "call-1" }), "call-1")).toBe(true);
    expect(callIsStillRinging(call({ id: "call-1", can_accept: false }), "call-1")).toBe(false);
    expect(callIsStillRinging(call({ id: "call-1", phase: "connected" }), "call-1")).toBe(false);
    expect(callIsStillRinging(call({ id: "call-1", phase: "ended" }), "call-1")).toBe(false);
    expect(callIsStillRinging(call({ id: "call-2" }), "call-1")).toBe(false);
    expect(callIsStillRinging(null, "call-1")).toBe(false);
  });

  /** A page that reconnects holding a microphone for a call the backend no longer has. */
  it("blames a restart only when the backend's start moment moved", () => {
    expect(lostCallMessage(1_000, 2_000)).toBe("teams-lite restarted, so you left the call.");
    expect(lostCallMessage(1_000, 1_000)).toBe("The call ended while this page was disconnected.");
    // A backend too old to say reads as the second sentence, which is true either way.
    expect(lostCallMessage(null, 2_000)).toBe("The call ended while this page was disconnected.");
    expect(lostCallMessage(1_000, undefined)).toBe(
      "The call ended while this page was disconnected.",
    );
    expect(lostCallMessage(undefined, null)).toBe(
      "The call ended while this page was disconnected.",
    );
  });
});

describe("whether a call can be placed", () => {
  it("needs the setting, the connection, and a free machine", () => {
    expect(canPlaceCall(status())).toBe(true);
    expect(canPlaceCall(status({ enabled: false }))).toBe(false);
    expect(canPlaceCall(status({ ready: false }))).toBe(false);
    expect(canPlaceCall(status({ call: call({ phase: "connected" }) }))).toBe(false);
    // A call that ended frees the machine again.
    expect(canPlaceCall(status({ call: call({ phase: "ended" }) }))).toBe(true);
  });

  /** A chat with somebody to ring is callable — one person, or a whole group at once,
   *  which is the same POST. Notes is the one chat with nobody in it. */
  it("offers a call wherever there is somebody to ring", () => {
    // The backend's own spellings (`ConversationKind`), not a guess: a mismatch here
    // hides the button in every chat.
    expect(conversationIsCallable("one_on_one")).toBe(true);
    expect(conversationIsCallable("group")).toBe(true);
    // A row the backend synced an id for and nothing else. Every one observed so far is a
    // meeting thread, which the action below sends to Join — and a call is a better
    // fallback than no control for one that is not.
    expect(conversationIsCallable("unknown")).toBe(true);
    for (const kind of ["notes", undefined] as const) {
      expect(conversationIsCallable(kind)).toBe(false);
    }
  });

  it("says why it cannot, in the order the user can act on", () => {
    // A window that does not call at all is a read-only backend or the second install, and
    // neither is something the user turns on: the sentence says what this window IS, and
    // never sends them to a switch that no longer exists.
    expect(callUnavailableReason(status({ enabled: false }))).toMatch(/cannot take calls/);
    expect(callUnavailableReason(status({ enabled: false }))).not.toMatch(/Settings/);
    expect(callUnavailableReason(status({ ready: false }))).toMatch(/not registered/);
    expect(callUnavailableReason(status({ call: call() }))).toMatch(/one call at a time/);
    expect(callUnavailableReason(status())).toBe("");
  });
});

describe("what a conversation's header offers", () => {
  function conversation(overrides: Partial<Conversation> = {}): Conversation {
    return {
      id: "19:21d2695ae8ff4e25ace9c662e5c326cb@thread.v2",
      name: "Design crew",
      last_message_time: 0,
      kind: "group",
      last_message_preview: "",
      last_message_sender: "",
      last_message_from_me: false,
      is_read: true,
      is_muted: false,
      is_pinned: false,
      is_hidden: false,
      thread_type: "chat",
      draft: "",
      ...overrides,
    };
  }

  /** The whole point of the pair: a meeting in the chat list is JOINED, without going to
   *  the calendar for its link, and every other chat is CALLED. */
  it("joins a meeting thread and calls every other chat", () => {
    const meetingChat = conversation({
      id: "19:meeting_YWI2Y2E5MDIt@thread.v2",
      thread_type: "meeting",
      name: "Daily",
    });
    expect(conversationCallAction(meetingChat)).toBe("join");
    expect(meetingAddressOf(meetingChat)).toEqual({
      kind: "thread",
      thread: "19:meeting_YWI2Y2E5MDIt@thread.v2",
    });

    expect(conversationCallAction(conversation())).toBe("call");
    expect(conversationCallAction(conversation({ kind: "one_on_one" }))).toBe("call");
    expect(conversationCallAction(conversation({ kind: "notes", id: "48:notes" }))).toBe("none");
    expect(conversationCallAction(undefined)).toBe("none");
  });

  /** The ADDRESS decides, never CSA's flag about the thread's origin: a row flagged
   *  `meeting` whose id names no meeting has nothing to join, and it must not be left with
   *  no control at all. */
  it("falls back to a call when the flag says meeting but no address does", () => {
    const flagged = conversation({ thread_type: "meeting" });
    expect(meetingAddressOf(flagged)).toBeNull();
    expect(conversationCallAction(flagged)).toBe("call");
  });

  /** A 1:1 has one person to name, so it is never a meeting address whatever else is
   *  true of it. */
  it("never reads a chat as a meeting on anything but its id", () => {
    expect(meetingAddressOf(conversation({ kind: "one_on_one" }))).toBeNull();
    expect(meetingAddressOf(undefined)).toBeNull();
  });

  /** The incoming-call card holds only the id the awareness event named, and it joins the
   *  meeting from there — so the rule reads a bare thread id under the same spelling. */
  it("reads the same rule from a bare thread id", () => {
    expect(meetingAddressOfThread("19:meeting_YWI2Y2E5MDIt@thread.v2")).toEqual({
      kind: "thread",
      thread: "19:meeting_YWI2Y2E5MDIt@thread.v2",
    });
    expect(meetingAddressOfThread("19:group-chat@thread.v2")).toBeNull();
    expect(meetingAddressOfThread("19:abc@thread.skype")).toBeNull();
    expect(meetingAddressOfThread(undefined)).toBeNull();
    expect(meetingAddressOfThread(null)).toBeNull();
  });
});

describe("what the bar says", () => {
  it("names each phase once", () => {
    expect(callPhaseLabel(call({ phase: "ringing" }))).toBe("Incoming call");
    expect(callPhaseLabel(call({ phase: "dialing" }))).toBe("Calling…");
    expect(callPhaseLabel(call({ phase: "connecting" }))).toBe("Connecting…");
    expect(callPhaseLabel(call({ phase: "connected" }))).toBe("In a call");
    expect(callPhaseLabel(call({ phase: "ended" }))).toBe("Call ended");
  });

  /** A GROUP call rang several phones, so once it is up "who is in it" is a fact that
   *  changes — one person may pick up while two never do — and only the roster knows. Until
   *  then it says what a 1:1 says: the bar names the conversation beside it. */
  it("answers a group call from its roster once it is up", () => {
    const group = (overrides: Partial<ActiveCall> = {}) =>
      call({ kind: "group", peer: "Design crew", peer_mri: "", ...overrides });
    expect(callPhaseLabel(group({ phase: "dialing" }))).toBe("Calling…");
    expect(callPhaseLabel(group({ phase: "connected" }))).toBe("In the call");
    expect(callPhaseLabel(group({ phase: "connected", others: ["Ava", "Liam"] }))).toBe(
      "With Ava and Liam",
    );
    // It is not a meeting: there is no lobby, and leaving it is hanging up.
    expect(isMeeting(group())).toBe(false);
    // But it names a CONVERSATION rather than a person, so the bar draws the group mark
    // instead of a face seeded from an empty mri.
    expect(callNamesAConversation(group())).toBe(true);
    expect(callNamesAConversation(call())).toBe(false);
    expect(callNamesAConversation(null)).toBe(false);
  });

  it("counts the duration from the backend's own clock", () => {
    const started = 1_700_000_000_000;
    const live = call({ phase: "connected", connected_at_ms: started });
    expect(callDurationLabel(live, started)).toBe("0:00");
    expect(callDurationLabel(live, started + 7_000)).toBe("0:07");
    expect(callDurationLabel(live, started + 65_000)).toBe("1:05");
    expect(callDurationLabel(live, started + 3_723_000)).toBe("1:02:03");
    // A clock that disagrees with the backend must not count backwards.
    expect(callDurationLabel(live, started - 5_000)).toBe("0:00");
    // Nothing to state before audio started.
    expect(callDurationLabel(call(), started)).toBe("");
    expect(callDurationLabel(null, started)).toBe("");
  });

  /** An ending the user caused needs no explanation: they were there. Every other one
   *  gets a sentence, because the alternative is a call that vanishes with no reason. */
  it("explains an ending the user did not cause, and stays quiet about the rest", () => {
    expect(callEndLabel(call({ end_reason: "CallEndReasonHangup" }))).toBe("");
    expect(callEndLabel(call({ end_reason: "CallEndReasonDeclined" }))).toBe("");
    // A ring the user let go without declining: they ended it themselves.
    expect(CALL_END_LEFT_RINGING).toBe("CallEndReasonLeftRinging");
    expect(callEndLabel(call({ end_reason: CALL_END_LEFT_RINGING }))).toBe("");
    expect(callEndLabel(call({ end_reason: null }))).toBe("");
    expect(callEndLabel(null)).toBe("");
    expect(callEndLabel(call({ end_reason: "CallEndReasonPlaceFailed" }))).toMatch(/not be placed/);
    expect(callEndLabel(call({ end_reason: "CallEndReasonAcceptFailed" }))).toMatch(
      /not be answered/,
    );
    expect(callEndLabel(call({ end_reason: "CallEndReasonReconnected" }))).toMatch(/connection/);
    expect(callEndLabel(call({ end_reason: "CallEndReasonCallingTurnedOff" }))).toMatch(
      /stopped taking calls/,
    );
    // A code we have never seen still says something rather than nothing.
    expect(callEndLabel(call({ end_reason: "code 486" }))).toBe("The call ended.");
  });

  /** A call that rang NOTHING — nobody's client is signed in. Measured against the tenant:
   *  the service invites nobody and ends the conversation two seconds later, which is
   *  indistinguishable from this app dropping the call unless the sentence says otherwise. */
  it("names the person a call could not reach, and the cause", () => {
    const ended = callEndLabel(
      call({ end_reason: CALL_END_UNREACHABLE, peer: "Gabriel CRETI", kind: "call" }),
    );
    expect(ended).toMatch(/Gabriel CRETI could not be reached/);
    expect(ended).toMatch(/no device of theirs is signed in/);
    // Never the service's own words: a sub-code and `addParticipantFailure` are written for
    // whoever holds the socket.
    expect(ended).not.toMatch(/endpoint|subCode|addParticipant/i);
    // A group and a meeting have no one person to name, so they say it of everybody.
    expect(callEndLabel(call({ end_reason: CALL_END_UNREACHABLE, kind: "group" }))).toMatch(
      /Nobody could be reached/,
    );
  });
});

describe("a ring that ended elsewhere, and one nobody answered", () => {
  const ring = (overrides: Partial<ActiveCall> = {}) =>
    call({ direction: "incoming", kind: "call", peer: "Ana Lima", phase: "ended", ...overrides });

  /** Answered on another device is the one ending that says WHO, and "you" is the commonest
   *  answer: the user picked up on their phone. Anybody else is named; a frame that named
   *  nobody says only what happened. */
  it("says who answered a ring this machine was offered", () => {
    expect(
      callEndLabel(
        ring({ end_reason: CALL_END_ANSWERED_ELSEWHERE, ended_elsewhere: { by_self: true, name: "Me" } }),
      ),
    ).toBe("You answered on another device.");
    expect(
      callEndLabel(
        ring({
          end_reason: CALL_END_ANSWERED_ELSEWHERE,
          ended_elsewhere: { by_self: false, name: "Dee Legate" },
        }),
      ),
    ).toBe("Dee Legate answered this call.");
    expect(
      callEndLabel(
        ring({ end_reason: CALL_END_ANSWERED_ELSEWHERE, ended_elsewhere: { by_self: false, name: "" } }),
      ),
    ).toBe("Somebody else answered this call.");
    // A backend too old to say who, or a frame that named nobody.
    expect(callEndLabel(ring({ end_reason: CALL_END_ANSWERED_ELSEWHERE }))).toBe(
      "The call was answered elsewhere.",
    );
    expect(callEndLabel(ring({ end_reason: CALL_END_ANSWERED_ELSEWHERE, ended_elsewhere: null }))).toBe(
      "The call was answered elsewhere.",
    );
  });

  it("names a forwarded ring, one that went to voicemail, and one the caller gave up on", () => {
    expect(callEndLabel(ring({ end_reason: CALL_END_FORWARDED }))).toBe("The call was forwarded.");
    expect(callEndLabel(ring({ end_reason: CALL_END_VOICEMAIL }))).toBe("The call went to voicemail.");
    expect(callEndLabel(ring({ end_reason: CALL_END_CALLER_CANCELLED }))).toBe(
      "Missed call from Ana Lima.",
    );
    expect(callEndLabel(ring({ end_reason: CALL_END_CALLER_CANCELLED, peer: "" }))).toBe("Missed call.");
  });

  /** WHICH side nobody was on is the whole sentence: a ring the user never took is a missed
   *  call, a call they placed that nobody took is "they did not answer". It used to be "The
   *  call ended." for both. */
  it("says which side nobody answered on", () => {
    expect(callEndLabel(ring({ end_reason: CALL_END_UNANSWERED }))).toBe("Missed call from Ana Lima.");
    expect(callEndLabel(ring({ end_reason: CALL_END_UNANSWERED, peer: "" }))).toBe("Missed call.");
    const placed = { end_reason: CALL_END_UNANSWERED, direction: "outgoing" as const };
    expect(callEndLabel(call({ ...placed, peer: "Ana Lima", kind: "call" }))).toBe(
      "Ana Lima did not answer.",
    );
    // A group has no one name to put there, and a person with no name has none either.
    expect(callEndLabel(call({ ...placed, peer: "Design crew", kind: "group" }))).toBe(
      "Nobody answered.",
    );
    expect(callEndLabel(call({ ...placed, peer: "", kind: "call" }))).toBe("Nobody answered.");
  });

  it("spells every name the backend states, and never the default sentence for one", () => {
    expect([
      CALL_END_ANSWERED_ELSEWHERE,
      CALL_END_FORWARDED,
      CALL_END_VOICEMAIL,
      CALL_END_CALLER_CANCELLED,
      CALL_END_UNANSWERED,
    ]).toEqual([
      "CallEndReasonAnsweredElsewhere",
      "CallEndReasonForwarded",
      "CallEndReasonForwardedToVoicemail",
      "CallEndReasonCallerCancelled",
      "CallEndReasonUnanswered",
    ]);
    for (const end_reason of [
      CALL_END_ANSWERED_ELSEWHERE,
      CALL_END_FORWARDED,
      CALL_END_VOICEMAIL,
      CALL_END_CALLER_CANCELLED,
      CALL_END_UNANSWERED,
    ]) {
      expect(callEndLabel(ring({ end_reason }))).not.toBe("The call ended.");
      expect(callEndLabel(ring({ end_reason }))).not.toBe("");
    }
  });
});

describe("joining a meeting", () => {
  const meeting = (overrides: Partial<ActiveCall> = {}) =>
    call({ kind: "meeting", direction: "outgoing", peer: "Quarterly planning", ...overrides });

  /** The check that decides whether a Join button is drawn at all. It is a small port of
   *  the Rust parse, so it has to agree with it on both answers. */
  it("recognises a Teams meeting link, and nothing else", () => {
    expect(
      isMeetingJoinLink(
        "https://teams.microsoft.com/l/meetup-join/19%3Ameeting_x%40thread.v2/0?context=%7B%7D",
      ),
    ).toBe(true);
    // A channel meeting is another real shape.
    expect(
      isMeetingJoinLink("https://teams.microsoft.com/l/meetup-join/19%3Aabc%40thread.tacv2/17194"),
    ).toBe(true);
    // And the SHORT shape Teams' newer meetings use — a code and a passcode, no thread.
    // It is the shape the user's own meetings have, and refusing it hid the button.
    expect(isMeetingJoinLink("https://teams.microsoft.com/meet/35017215452446?p=4QyEW")).toBe(true);
    expect(isMeetingJoinLink("https://teams.microsoft.com/meet/12345")).toBe(true);
    for (const url of [
      "",
      null,
      undefined,
      "https://teams.microsoft.com/l/channel/19%3Aabc%40thread.tacv2/General",
      "https://zoom.us/j/123",
      // The path is right, the thread is not one.
      "https://teams.microsoft.com/l/meetup-join/quarterly-planning",
      // And a short path with no code names no meeting.
      "https://teams.microsoft.com/meet/",
    ]) {
      expect(isMeetingJoinLink(url)).toBe(false);
    }
  });

  /** A meeting is many people by definition, so the one-to-one rule does not apply to
   *  it. One call at a time still does — there is one microphone. */
  it("needs calling on and a free machine, and no one-to-one rule", () => {
    expect(canJoinMeeting(status())).toBe(true);
    expect(canJoinMeeting(status({ enabled: false }))).toBe(false);
    expect(canJoinMeeting(status({ ready: false }))).toBe(false);
    expect(canJoinMeeting(status({ call: meeting({ phase: "connected" }) }))).toBe(false);
    expect(meetingUnavailableReason(status({ enabled: false }))).toMatch(/cannot take calls/);
    expect(meetingUnavailableReason(status({ enabled: false }))).not.toMatch(/Settings/);
    expect(meetingUnavailableReason(status({ call: meeting() }))).toMatch(/one call at a time/);
    expect(meetingUnavailableReason(status())).toBe("");
  });

  /** The lobby is its own state. "Connecting…" would hide the one thing the user has to
   *  know: nobody has let them in yet. */
  it("says the lobby, the join, and who is there", () => {
    expect(callPhaseLabel(meeting({ phase: "connecting" }))).toBe("Joining…");
    expect(callPhaseLabel(meeting({ phase: "connecting", in_lobby: true }))).toMatch(
      /Waiting to be let in/,
    );
    // Admitted, but the roster has not arrived: it says nothing about who is there
    // rather than "0 others".
    expect(callPhaseLabel(meeting({ phase: "connected" }))).toBe("In the meeting");
    expect(callPhaseLabel(meeting({ phase: "ended" }))).toBe("Meeting left");
  });

  /** One or two people are named, because that is what a glance wants. A crowd is
   *  counted, because six names do not fit and would not be read. */
  it("names a couple of people and counts a crowd", () => {
    expect(callPresenceLabel(meeting({ others: ["Ava"] }))).toBe("With Ava");
    expect(callPresenceLabel(meeting({ others: ["Ava", "Liam"] }))).toBe("With Ava and Liam");
    expect(callPresenceLabel(meeting({ others: ["Ava", "Liam", "Priya"] }))).toBe(
      "With 3 others",
    );
    // A roster of blanks is a roster that has not arrived, and each kind says where the
    // user is in its own words.
    expect(callPresenceLabel(meeting({ others: ["", " "] }))).toBe("In the meeting");
    expect(callPresenceLabel(call({ kind: "group", others: [] }))).toBe("In the call");
  });

  it("tells a meeting apart from a call", () => {
    expect(isMeeting(meeting())).toBe(true);
    expect(isMeeting(call())).toBe(false);
    expect(isMeeting(null)).toBe(false);
  });

  it("explains a join that failed", () => {
    expect(callEndLabel(meeting({ end_reason: "CallEndReasonJoinFailed" }))).toMatch(
      /could not be joined/,
    );
  });
});

// The microphone follows the call (incoming-and-conversation M1). The direction a mistake may
// go in is the whole of these rules: a frame may close a microphone, and only the window's own
// press opens one — at the press, or on the backend's answer to it.
const press = (muted: boolean, afterSeq: number, failed = false): PendingMute => ({
  callId: "call-1",
  muted,
  afterSeq,
  failed,
  resent: false,
});

describe("the microphone this window holds", () => {
  const open = (
    trackOpen: boolean,
    backendMuted: boolean,
    backendSeq: number,
    pending: PendingMute | null = null,
  ) => microphoneShouldBeOpen({ trackOpen, backendMuted, backendSeq, pending });

  it("closes on a mute pressed in another window", () => {
    expect(open(true, true, 1)).toBe(false);
  });

  it("never opens because a frame says unmuted — not with no press, not with this window's own", () => {
    expect(open(false, false, 2)).toBe(false);
    // A newer frame, even one that answers this window's own Unmute: opening is the press's job
    // (and the answer's), never a frame's.
    expect(open(false, false, 5, press(false, 4))).toBe(false);
    // …and an open microphone stays open: a frame only ever narrows.
    expect(open(true, false, 2)).toBe(true);
  });

  it("keeps an Unmute just pressed open on a frame from before the press", () => {
    // Rule 1's only work: without it the roster that predates the press re-mutes the user.
    expect(open(true, true, 4, press(false, 4))).toBe(true);
    expect(open(true, true, 3, press(false, 4))).toBe(true);
  });

  it("keeps a Mute just pressed closed on a frame from before the press", () => {
    expect(open(false, false, 4, press(true, 4))).toBe(false);
  });

  it("closes on a NEWER mute even while this window's own Unmute waits", () => {
    expect(open(true, true, 5, press(false, 4))).toBe(false);
  });
});

describe("a press this window waits on", () => {
  const pending = press(false, 4);

  it("is kept while frames are from before it", () => {
    expect(pendingMuteAfterFrame(pending, 3)).toBe(pending);
    expect(pendingMuteAfterFrame(pending, 4)).toBe(pending);
  });

  it("is forgotten at the first newer frame, whether or not that frame agrees", () => {
    expect(pendingMuteAfterFrame(pending, 5)).toBeNull();
  });

  it("is nothing when there was none", () => {
    expect(pendingMuteAfterFrame(null, 9)).toBeNull();
  });
});

describe("one frame applied to the microphone this window holds", () => {
  const live = (overrides: Partial<ActiveCall> = {}) =>
    call({ phase: "connected", mute_seq: 3, muted: false, ...overrides });
  const follow = (
    frame: ActiveCall,
    trackOpen: boolean,
    pending: PendingMute | null = null,
    applied: { callId: string; seq: number } | null = null,
  ) => followMuteFrame({ call: frame, callId: "call-1", trackOpen, pending, applied });

  it("closes the track on a newer mute and records what it applied", () => {
    expect(follow(live({ muted: true, mute_seq: 4 }), true, null, { callId: "call-1", seq: 3 })).toEqual({
      open: false,
      pending: null,
      applied: { callId: "call-1", seq: 4 },
    });
  });

  it("ignores a frame OLDER than one already applied — two frames can reach the socket in the other order", () => {
    expect(follow(live({ muted: true, mute_seq: 2 }), true, null, { callId: "call-1", seq: 3 })).toBeNull();
    // The same sequence is not older: it is the same press, and applying it again changes nothing.
    expect(follow(live({ muted: true, mute_seq: 3 }), false, null, { callId: "call-1", seq: 3 })).not.toBeNull();
  });

  it("says nothing on a backend that does not count presses, or about another call", () => {
    expect(follow(live({ mute_seq: undefined, muted: true }), true)).toBeNull();
    expect(follow(live({ id: "call-2", muted: true }), true)).toBeNull();
  });

  it("reads only this call's press, and forgets it at the first newer frame", () => {
    const mine = press(false, 3);
    expect(follow(live({ muted: true, mute_seq: 3 }), true, mine)).toEqual({
      open: true,
      pending: mine,
      applied: { callId: "call-1", seq: 3 },
    });
    expect(follow(live({ muted: true, mute_seq: 3 }), true, { ...mine, callId: "call-0" })?.open).toBe(false);
    expect(follow(live({ mute_seq: 4 }), true, mine)?.pending).toBeNull();
  });

  it("never lets a failed Unmute kept past another window's Mute open the microphone again", () => {
    // Forgotten at the newer Mute, it cannot be read against the old build's Unmute after it.
    const first = follow(live({ muted: true, mute_seq: 4 }), true, press(false, 3));
    expect(first).toMatchObject({ open: false, pending: null });
    const second = follow(live({ muted: false, mute_seq: 5 }), false, first!.pending, first!.applied);
    expect(second?.open).toBe(false);
  });
});

describe("the backend's answer to this window's newest press", () => {
  const answer = (muted: boolean, answerSeq: number | undefined, appliedSeq: number | null) =>
    followMuteAnswer({
      muted,
      answerSeq,
      applied: appliedSeq === null ? null : { callId: "call-1", seq: appliedSeq },
      callId: "call-1",
    });

  it("is the decision when nothing newer has been applied", () => {
    // Mute then Unmute inside one round trip: the Mute's frame (4) closed the track and the
    // Unmute's frame (5) cannot open it — the Unmute's answer does.
    expect(answer(false, 5, 5)).toBe(true);
    expect(answer(false, 5, 4)).toBe(true);
    expect(answer(true, 5, 5)).toBe(false);
  });

  it("is nothing once a newer press — from any window — has been decided", () => {
    expect(answer(false, 5, 6)).toBeNull();
  });

  it("is nothing from a backend that does not say where it recorded the press", () => {
    expect(answer(false, undefined, 4)).toBeNull();
  });

  it("ignores what was applied to ANOTHER call", () => {
    expect(
      followMuteAnswer({ muted: false, answerSeq: 1, applied: { callId: "call-0", seq: 9 }, callId: "call-1" }),
    ).toBe(true);
  });
});

describe("sending a lost press again after a reconnect", () => {
  const failedMute = press(true, 2, true);
  const live = (overrides: Partial<ActiveCall> = {}) =>
    call({ phase: "connected", mute_seq: 2, ...overrides });

  it("sends a failed Mute when the backend has decided no press since", () => {
    expect(muteToResend(failedMute, live())).toBe(true);
  });

  it("never sends a press that did not fail — one made just after the socket came back is in flight, not lost", () => {
    expect(muteToResend(press(true, 2), live())).toBe(false);
  });

  it("never sends an Unmute: it took itself back, and opening the microphone later would open it on a press the user saw fail", () => {
    expect(muteToResend(press(false, 2, true), live())).toBe(false);
  });

  it("never sends it over a press made since, in any window", () => {
    expect(muteToResend(failedMute, live({ mute_seq: 3, muted: false }))).toBe(false);
  });

  it("sends it once", () => {
    expect(muteToResend({ ...failedMute, resent: true }, live())).toBe(false);
  });

  it("sends nothing for another call, an ended one, or a backend that does not count presses", () => {
    expect(muteToResend(failedMute, live({ id: "call-2" }))).toBe(false);
    expect(muteToResend(failedMute, live({ phase: "ended" }))).toBe(false);
    expect(muteToResend(failedMute, live({ mute_seq: undefined }))).toBe(false);
    expect(muteToResend(failedMute, null)).toBe(false);
    expect(muteToResend(null, live())).toBe(false);
  });
});

describe("what a window draws for Mute", () => {
  const live = (muted: boolean) => call({ phase: "connected", muted });

  it("follows the track where the window holds the microphone", () => {
    // The backend says live, the track is closed (a press that never arrived): the track wins.
    expect(callShowsMuted(live(false), "muted")).toBe(true);
    expect(callShowsMuted(live(true), "live")).toBe(false);
  });

  it("follows the backend everywhere else", () => {
    expect(callShowsMuted(live(true), null)).toBe(true);
    expect(callShowsMuted(live(false), null)).toBe(false);
  });

  it("refuses an Unmute only where the window holds no microphone and is not opening one", () => {
    expect(unmuteRefusal(live(true), null, false)).toBe(UNMUTE_REFUSED_HERE);
    expect(unmuteRefusal(live(false), null, false)).toBeNull();
    expect(unmuteRefusal(live(true), "muted", false)).toBeNull();
    // The window still opening its microphone for the call it started is the one that will
    // carry the audio, so its Unmute is its own.
    expect(unmuteRefusal(live(true), null, true)).toBeNull();
  });
});
