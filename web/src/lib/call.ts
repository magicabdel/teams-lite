/**
 * Audio calling, as the backend describes it (`call_status` / `call_state` in
 * src/bin/server.rs, over src/calling.rs — and NATIVE-CALLING.md for the protocol).
 *
 * Two facts shape everything in this module and the next:
 *
 * - **The backend signals; this page carries the audio.** The tokens never reach a
 *   browser and the microphone is only reachable from one, so an SDP crosses the local
 *   WebSocket in each direction and nothing else about a call does. See
 *   {@link ./call-media}.
 * - **Calling is ON, with no switch anywhere.** Every backend the user can open registers as
 *   a device their calls ring on, the way every other Teams client they are signed in on
 *   does, and registering reaches nobody by itself: the actions that do (place, answer,
 *   hang up, mute) are each the user's own click, gated one by one in the backend. So what
 *   this module reports about the registration is a STATE to explain, never a setting to
 *   change — and no sentence in it ever sends the user to look for one.
 *
 * Everything here is pure. The state comes from the backend, the actions live on the
 * controller, and nothing in this file reaches the network.
 */

import type { Conversation, ConversationKind } from "./protocol";

/** How far along a call is, as the backend states it. */
export type CallPhase = "ringing" | "dialing" | "connecting" | "connected" | "ended";

/** Which way it was set up. */
export type CallDirection = "incoming" | "outgoing";

/** A one-to-one call, a call that rang a whole group chat, or a meeting this machine
 *  joined. It changes what the UI says and what the roster means — the signaling is the
 *  same in all three. A `group` and a `meeting` both name the CONVERSATION where a 1:1
 *  names the person, because several people have no one name. */
export type CallKind = "call" | "group" | "meeting";

/** The one call this machine is in, as every client is shown it.
 *
 *  It deliberately carries no SDP, no links and no credentials: those leave the backend
 *  only through a token-gated method, and only to the page that asked to place or answer
 *  this exact call. */
export type ActiveCall = {
  id: string;
  direction: CallDirection;
  kind: CallKind;
  phase: CallPhase;
  /** The chat, channel or meeting thread the call belongs to, when it has one. */
  conversation_id: string | null;
  /** Who is on the other end of a one-to-one call. A meeting names its own title here
   *  instead, because "who" is the roster below. */
  peer: string;
  peer_mri: string;
  /** Everybody else in a meeting, by name. Empty for a one-to-one call. */
  others: string[];
  /** Their MRIs, aligned with `others`, for their faces. */
  other_mris: string[];
  /** True while the meeting has us in its LOBBY: joined, and waiting to be let in. */
  in_lobby: boolean;
  /** How many of `others` are themselves still in the lobby — people who cannot hear
   *  the user yet. */
  waiting_in_lobby: number;
  /** Whether the user asked to be muted — the newest press, from whichever window made it.
   *  The window that carries the call's audio closes its microphone on it and never opens one
   *  because of it ({@link microphoneShouldBeOpen}); every other window draws its button from
   *  it. */
  muted: boolean;
  /** How many mute presses the backend has accepted for this call, so a window waiting on its
   *  own press can tell a frame that already reflects it from one that went out before it.
   *  ABSENT from a backend too old to count them — which reads as today's behaviour: a frame
   *  then never touches the microphone at all. */
  mute_seq?: number;
  /** When audio started (epoch ms), so a duration counts from the backend's clock. */
  connected_at_ms: number | null;
  /** Why it ended, on the one frame that says it did. */
  end_reason: string | null;
  /** Who took a ring this machine was offered, when the service said it was answered
   *  somewhere else and named by whom (`CALL_END_ANSWERED_ELSEWHERE`): the user themselves on
   *  another device, or somebody else by the name this app gives them. Null on every other
   *  ending and on an answer elsewhere that named nobody; absent from an older backend. */
  ended_elsewhere?: { by_self: boolean; name: string } | null;
  /** How THIS call speaks to Teams — the protocol it kept from its start, which a setting
   *  changed since does not move. Absent from an older backend. */
  protocol?: CallProtocol;
  /** What everybody ELSE in the meeting is publishing, so this page can ask for it. Empty
   *  for a one-to-one call and for a backend too old to say. */
  publishing: PublishingParticipant[];
  /** What this MACHINE is sending beyond audio: `"camera"`, `"screen"`, or neither. Published
   *  by the backend so two open pages agree and a reconnecting one is told rather than
   *  guessing from its own memory. */
  sending: string[];
  /** Whether answering is possible — decided by the backend, which holds the links. */
  can_accept: boolean;
  can_hangup: boolean;
  /** Whether new media can be offered at all. The service refuses it on a call that is not
   *  established, and only the backend knows whether the link exists. */
  can_send_media: boolean;
};

/** The modality name the service reads for one of the two things this app can send.
 *
 *  `ScreenSharer` and not `ScreenViewer`: sending a screen and watching one are two different
 *  modalities, and the one that says "my screen is on the wire" is this. */
export function modalityFor(kind: "camera" | "screen"): string {
  return kind === "camera" ? "Video" : "ScreenSharer";
}

/** One person in the meeting and the streams they publish. */
export type PublishingParticipant = {
  mri: string;
  name: string;
  streams: PublishedStream[];
};

/**
 * One stream somebody publishes into the meeting.
 *
 * **`source_id` is why this type exists.** It is the media source id a subscription is
 * addressed by, and the roster is the only place it exists (NATIVE-CALLING.md § 10.2). It is
 * per meeting and it moves between joins, so it is never cached across calls.
 */
export type PublishedStream = {
  /** `main-audio` / `main-video` / `applicationsharing-video` / `data`. */
  label: string;
  kind: string;
  source_id: number;
  direction: string;
  server_muted: boolean;
  /** Whether this is somebody's screen being shared. Decided by the backend, so the label
   *  vocabulary is known in one place rather than two. */
  shared_screen: boolean;
  /** Whether this is somebody's camera, actually being sent. */
  camera: boolean;
};

/**
 * Where this machine's calling registration stands, as the backend states it
 * (`CallingPlane.registration` in src/bin/server.rs).
 *
 * It exists because `ready: false` alone was one sentence for situations with different next
 * moves: a sign-in outage (nothing to do here — it comes back on its own once sign-in does), a
 * directory that names no calling service at all (nothing will ever ring here), a connection
 * on its way up, and a registration the registrar has already answered. Read as one,
 * the user was told "not registered yet" about an account that cannot call, and about a
 * machine that was a sign-in away from calling again.
 *
 * - `"starting"` — the backend has not tried yet.
 * - `"waiting_for_sign_in"` — the last attempt had no session to start from.
 * - `"unavailable"` — the account's Teams directory names no calling service.
 * - `"connecting"` — the calling socket is (re)connecting.
 * - `"registered"` — the registrar answered 2xx (`registrar_status`).
 * - `"refused"` — the registrar answered something else (`registrar_status`). Informational
 *   for now: the connection proceeds exactly as before, so it normally comes with
 *   `ready: true`.
 *
 * The backend's own test reads THIS TYPE for its words (`the_registration_words_are_the_pages_own`
 * in src/bin/server.rs) and holds it to exactly the backend's six, in this order: a word one side
 * spells and the other does not would otherwise silently put the user back in front of "not
 * registered yet". So keep the members one quoted word each — that is the shape the test reads.
 */
export type CallRegistrationState =
  | "starting"
  | "waiting_for_sign_in"
  | "unavailable"
  | "connecting"
  | "registered"
  | "refused";

/** The registration as `call_status` / `call_state` carry it. It names no URL and no
 *  token: a state word, and the registrar's HTTP status where it answered one. */
export type CallRegistration = {
  state: CallRegistrationState;
  /** The registrar's HTTP status for `"registered"` and `"refused"`; null otherwise. */
  registrar_status: number | null;
};

/** Everything `call_status` reports, and the payload of every `call_state` event. */
export type CallStatus = {
  /** Whether this window's backend takes calls at all. True on every install the user
   *  can open — each registers as a device their calls ring on, like every other Teams
   *  client they are signed in on, and two on one machine are two devices. False on the
   *  ONE backend they never opened: a read-only one. There is no switch. */
  enabled: boolean;
  /** Whether a call could start right now: the calling connection is up and
   *  registered. A backend that calls while this is false is honest about a connection
   *  that has not come back yet — and {@link CallStatus.registration} says why. */
  ready: boolean;
  call: ActiveCall | null;
  /** Where the registration stands. Null where `enabled` is false (nothing registers), and
   *  ABSENT from a backend too old to say — which reads as today's "not registered yet",
   *  never as a guess. A word this build does not know reads the same way. */
  registration?: CallRegistration | null;
  /** When this backend process began serving (epoch ms), or null when it cannot say. A
   *  process start time rather than an identity: what it tells a page is that the backend
   *  it is talking to is not the one it was talking to before. */
  backend_started_ms?: number | null;
  /** How a call this backend starts FROM NOW ON speaks to Teams — Settings › This app ›
   *  Calling protocol. Per backend, so the two installs on one machine may differ. Absent from
   *  a backend too old to have the setting, which then offers no row at all. */
  protocol?: CallProtocol;
};

/**
 * Where the user's own mute is published, as `call_protocol_set` names it.
 *
 * `legacy` is what this app has always sent: `muteUnmute` to the call's own `mute` link.
 * `endpoint-state` is the Teams web client's own toggle, `updateEndpointState` on the call's
 * conversation — what a meeting draws as the crossed-out microphone beside a name
 * (docs/calling-roadmap/incoming-and-conversation.md § 4.7). A Rust test holds this TYPE to
 * exactly the backend's words (`calling::MuteProtocol::WORDS`).
 */
export type MuteProtocol = "legacy" | "endpoint-state";

/** How a backend's calls speak to Teams, where it has a choice between the flow this app
 *  measured and the one the Teams web client uses. One field today; the roadmap's others join
 *  it in the milestone that gives them something to switch. */
export type CallProtocol = { mute: MuteProtocol };

/** What `call_prepare` hands back: what one `RTCPeerConnection` needs, and — when
 *  answering — the offer to answer. */
export type CallPreparation = {
  call_id: string;
  /** ICE servers in the shape `RTCPeerConnection` takes. */
  ice_servers: RTCIceServer[];
  /** The caller's SDP offer. Present only when answering. */
  offer_sdp?: string;
  /**
   * Whether this call is between exactly two people.
   *
   * It decides how a camera and a screen are negotiated, and the difference is the real
   * client's own: it reserves both sections in the FIRST offer of a one-to-one and adds them
   * mid-call in a conference. A backend too old to say it reads as `false`, which is the
   * behaviour this app already had.
   */
  one_to_one?: boolean;
};

/** The `call_media` event: the far side's SDP, which is the frame that turns a call
 *  that is ringing into audio. */
export type CallMediaSignal = {
  call_id: string;
  sdp: string;
  kind: "answer" | "offer";
};

/** The status a client holds before the backend has answered.
 *
 *  Both flags are false on purpose: no call can start through a backend that has not
 *  spoken yet, and a hopeful `true` on either would tell the user their calls ring here
 *  while nothing is registered. The backend says otherwise within one round trip. */
export const UNKNOWN_CALL_STATUS: CallStatus = { enabled: false, ready: false, call: null };

/** Whether this app could place a call at all: this backend takes calls, the connection
 *  is up, and no call is in flight (one at a time — one microphone). */
export function canPlaceCall(status: CallStatus): boolean {
  return status.enabled && status.ready && !isLive(status.call);
}

/** Whether a call is still going on. `ended` arrives once, so the UI can say why. */
export function isLive(call: ActiveCall | null): boolean {
  return !!call && call.phase !== "ended";
}

/**
 * Whether the microphone a page holds for call `heldFor` still belongs to a call that is going
 * on, by what the backend says now — a `call_state` frame, or a `call_status` read.
 *
 * It is keyed on the CALL, not on the phase. An answer that is still travelling holds the
 * microphone while the backend's phase is still `ringing` (the phase moves when the attach
 * answers), so reading the phase would release it under the very answer it belongs to. What
 * really says the media is orphaned is the call being over, gone, or a different call — which
 * is what a page that reconnects to a RESTARTED backend finds.
 */
export function callStillHoldsMedia(call: ActiveCall | null, heldFor: string | null): boolean {
  return heldFor !== null && !!call && isLive(call) && call.id === heldFor;
}

/** Whether call `callId` can still be answered here, by what the backend says now — which is
 *  what a failed Answer may promise the user, and nothing more. */
export function callIsStillRinging(call: ActiveCall | null, callId: string): boolean {
  return !!call && call.id === callId && call.phase === "ringing" && call.can_accept;
}

// ---------------------------------------------------------------------------
// The microphone follows the call (incoming-and-conversation M1)
// ---------------------------------------------------------------------------
//
// A call is drawn in every window that has the app open, and only ONE of them carries its
// audio: the window that placed or answered it. Mute pressed anywhere has to silence THAT
// microphone — before this, Mute on the phone published a mute while the laptop went on
// sending the user's voice. So every window reads the backend's `muted`, and the window that
// holds the microphone applies each frame to its track. The whole of the rules below is the
// direction a mistake may go in: a frame may CLOSE a microphone, and only this window's own
// press OPENS one — at the press, or when the backend answers that press.

/** What the microphone this window holds is doing, read off the track itself. */
export type CallMicrophone = "live" | "muted";

/**
 * A mute press this window made on the microphone it holds (or is opening), until the backend
 * has decided it.
 *
 * The track moves at the press, and the backend is told after it, so for a round trip — or
 * for good, when the request never arrives — the backend's state is from BEFORE the press.
 * Every frame in that window (a roster, a renegotiation) still carries that older state, and
 * read as an answer it would undo the press. `afterSeq` is the backend's `mute_seq` when the
 * press was made, which is what tells a frame from before the press apart from one after it.
 */
export type PendingMute = {
  callId: string;
  muted: boolean;
  /** The backend's `mute_seq` at the press: a frame at or below it went out before the press. */
  afterSeq: number;
  /** Whether the request was REFUSED or lost on the way. Only a failed Mute is ever kept with
   *  this set — a failed Unmute takes itself back (see the store's `setCallMuted`) — and only a
   *  failed press is sent again after a reconnect. */
  failed: boolean;
  /** Whether it has already been sent again after a reconnect. It is sent again at most once. */
  resent: boolean;
};

/**
 * Whether the microphone this window holds should be open once a frame of call state arrives.
 *
 * 1. A frame that went out before this window's own press says nothing about it: the press
 *    stands, whatever the frame says. That is what keeps an Unmute just pressed open when a
 *    roster that predates it still says muted.
 * 2. A newer frame saying muted closes the microphone, whoever pressed — Mute in any window
 *    silences the one microphone that is live.
 * 3. Otherwise a frame leaves the microphone as it is. A frame never OPENS one: a window that
 *    does not carry the audio may mute it and may not unmute it, and this window's own Unmute
 *    is opened at the press and confirmed by the backend's answer ({@link followMuteAnswer}).
 *
 * `pending` is this window's press on THIS call, already filtered by the caller. A server mute
 * is not an input yet: meeting-awareness M6 adds it as the rule above all three.
 */
export function microphoneShouldBeOpen(input: {
  trackOpen: boolean;
  backendMuted: boolean;
  backendSeq: number;
  pending: PendingMute | null;
}): boolean {
  const { trackOpen, backendMuted, backendSeq, pending } = input;
  if (pending && backendSeq <= pending.afterSeq) return !pending.muted;
  if (backendMuted) return false;
  return trackOpen;
}

/**
 * The press still waiting once a frame at `backendSeq` has been applied.
 *
 * It is forgotten at the FIRST frame newer than it, whether or not that frame agrees with it —
 * stricter than waiting for the frame that confirms it, and the strictness is the point. A
 * newer frame means the backend has decided this press or another made after it was recorded;
 * a press kept past that would let a LATER frame be read against a stale intention, and be
 * sent again over a press made since. What it costs is the safe direction only: a frame that
 * answers one of this window's OWN earlier presses forgets the newer one too, and closes the
 * microphone if it said muted — which the backend's answer to the newer press then puts right
 * ({@link followMuteAnswer}).
 */
export function pendingMuteAfterFrame(
  pending: PendingMute | null,
  backendSeq: number,
): PendingMute | null {
  return pending && backendSeq > pending.afterSeq ? null : pending;
}

/** The newest `mute_seq` this window has applied to the microphone it holds, for one call. */
export type AppliedMuteSeq = { callId: string; seq: number };

/**
 * What one frame of call state does to the microphone this window holds: whether the track
 * should be open, the press still waiting, and the newest sequence applied — or `null` when the
 * frame says nothing about the track.
 *
 * It says nothing in three cases. A backend too old to count presses sends no `mute_seq`, and
 * there a frame never touched the track, so it still does not. A frame about another call is
 * not this call's. And a frame OLDER than one already applied is stale: frames are built under
 * the backend's lock and sent after it, by whichever task changed the call, so two of them can
 * reach the socket in the other order.
 */
export function followMuteFrame(input: {
  call: ActiveCall;
  callId: string;
  trackOpen: boolean;
  pending: PendingMute | null;
  applied: AppliedMuteSeq | null;
}): { open: boolean; pending: PendingMute | null; applied: AppliedMuteSeq } | null {
  const { call, callId, trackOpen, applied } = input;
  if (call.id !== callId || typeof call.mute_seq !== "number") return null;
  const seq = call.mute_seq;
  if (applied && applied.callId === callId && seq < applied.seq) return null;
  const pending = input.pending?.callId === callId ? input.pending : null;
  return {
    open: microphoneShouldBeOpen({ trackOpen, backendMuted: call.muted, backendSeq: seq, pending }),
    pending: pendingMuteAfterFrame(pending, seq),
    applied: { callId, seq },
  };
}

/**
 * What the backend's ANSWER to this window's newest press does to the microphone it holds:
 * the state to put the track in, or `null` to leave it.
 *
 * The answer names the sequence the press was recorded at. When nothing newer than it has been
 * applied here, that press IS the backend's newest decision, so the track follows it — open for
 * an Unmute, closed for a Mute. It exists for the press a frame cannot settle: Mute then Unmute
 * inside one round trip, where the frame for the Mute is newer than the Unmute's `afterSeq`,
 * closes the microphone and forgets the Unmute, and the frame for the Unmute then finds nothing
 * to open it with. It opens a microphone only for this window's own press, and only while no
 * other press has been decided since — so a Mute from another window recorded after it still
 * wins, on its own frame.
 */
export function followMuteAnswer(input: {
  muted: boolean;
  answerSeq: number | undefined;
  applied: AppliedMuteSeq | null;
  callId: string;
}): boolean | null {
  const { muted, answerSeq, applied, callId } = input;
  if (typeof answerSeq !== "number") return null;
  if (applied && applied.callId === callId && applied.seq > answerSeq) return null;
  return !muted;
}

/**
 * Whether a press that did not arrive is sent again, now that the page has reconnected.
 *
 * Only a failed MUTE, once, for the same call, and only while the backend has decided NO press
 * since it (`mute_seq` has not moved past it). That last condition is what keeps this from
 * being a decision: it delivers the user's own press that was lost on the way, never one a
 * later press has already overtaken — and the backend checks it again (`if_seq`), because the
 * read this is decided on is a round trip old by the time the press arrives. A failed Unmute is
 * never sent again: it took itself back, and the button has said "muted" since — opening the
 * microphone at the next reconnect would open it on a press the user saw fail. Whether this
 * window still holds the call's microphone is the caller's to check.
 */
export function muteToResend(pending: PendingMute | null, call: ActiveCall | null): boolean {
  return (
    !!pending &&
    pending.failed &&
    pending.muted &&
    !pending.resent &&
    !!call &&
    isLive(call) &&
    call.id === pending.callId &&
    typeof call.mute_seq === "number" &&
    call.mute_seq <= pending.afterSeq
  );
}

/** Whether a window draws the user as muted: its own TRACK where it holds the microphone,
 *  and the backend's state everywhere else. The track is the truth that can be heard — a
 *  button that followed the backend there would say "live" over a press that failed to
 *  arrive. */
export function callShowsMuted(call: ActiveCall, microphone: CallMicrophone | null): boolean {
  return microphone ? microphone === "muted" : call.muted;
}

/** Why this window may not unmute, or `null` where it may. Only the window that carries the
 *  call's audio unmutes it — a frame never opens a microphone, so an Unmute anywhere else would
 *  change what the meeting is told and nothing anybody can hear. A window still OPENING its
 *  microphone for the call it is starting is that window, so it may. Said without claiming
 *  WHERE the audio is: this window cannot tell another window holding it from a reload that
 *  lost it (that is incoming-and-conversation M8). */
export function unmuteRefusal(
  call: ActiveCall,
  microphone: CallMicrophone | null,
  starting: boolean,
): string | null {
  return microphone === null && !starting && call.muted ? UNMUTE_REFUSED_HERE : null;
}

/** The sentence a window that does not carry the call's audio says beside its disabled Unmute. */
export const UNMUTE_REFUSED_HERE = "This window does not carry the call's audio, so it cannot unmute you.";

/** The same fact in the folded window's one short line, which has room for a few words. */
export const UNMUTE_REFUSED_HERE_SHORT = "Muted · no audio in this window";

/** What a window that does not carry the call's audio says when its Mute was REFUSED — and only
 *  then: an answer that was merely lost may have been recorded, and saying "nothing was muted"
 *  over a microphone the frame already closed would be false. Silence would be worse than in the
 *  window that holds the microphone: there the track closed at the press, while here nothing
 *  closed at all. */
export const MUTE_DID_NOT_ARRIVE = "The mute did not reach the call, so nothing was muted. Try again.";

/**
 * What a page says when it reconnects holding a microphone for a call the backend no longer has.
 *
 * The page missed the frame that ended it, so this is the one place that can still say it. When
 * the backend's start moment moved, the call went with a restart — which is what the user can
 * act on, because a restart is something somebody did. When it did not move, the call ended
 * while the page could not hear about it. A backend too old to state the moment reads as the
 * second, which is true either way.
 */
export function lostCallMessage(
  heldWithBackendStartedMs: number | null | undefined,
  nowBackendStartedMs: number | null | undefined,
): string {
  const restarted =
    typeof heldWithBackendStartedMs === "number" &&
    typeof nowBackendStartedMs === "number" &&
    heldWithBackendStartedMs !== nowBackendStartedMs;
  return restarted
    ? "teams-lite restarted, so you left the call."
    : "The call ended while this page was disconnected.";
}

/**
 * What a press that ends a call MEANT (`call_hangup`'s `intent`).
 *
 * Only `"decline"` — the ringing card's Decline — ever declines a caller. Every other ending is
 * `"end"`, and on a call still ringing that lets the call go here and tells nobody, so it goes
 * on ringing the user's other devices. The backend decides the door (`hangup_door` in
 * src/bin/server.rs) and refuses any other word.
 */
export type CallHangupIntent = "decline" | "end";

/** Whether a conversation is one this app can call: a chat with people in it to ring.
 *
 *  A one-to-one rings the one person; a GROUP chat rings every member at once, which is
 *  the same POST (`calling::invitation_payload`) and the same call this UI already draws —
 *  the service mixes the voices, the page keeps one `<audio>` per remote stream, and "who
 *  is in it" is the roster a meeting already answers from. The backend still decides how
 *  many is too many (`MAX_GROUP_CALL_PEOPLE`), because that is a fact about the thread it
 *  has to fetch and this is a pure function.
 *
 *  Notes — the chat with oneself — is the one chat with nobody to ring. A CHANNEL is not a
 *  conversation here at all (it has no row in this list), and a MEETING chat is joined
 *  rather than rung: see {@link conversationCallAction}.
 *
 *  The value is the backend's own (`ConversationKind::OneOnOne` → `"one_on_one"`), never
 *  a guess: a spelling that matches nothing would silently hide the button everywhere. */
export function conversationIsCallable(kind: ConversationKind | undefined): boolean {
  return kind === "one_on_one" || kind === "group" || kind === "unknown";
}

/** What a conversation's header offers: nothing, a call it places, or a meeting it joins.
 *
 *  One control per conversation, and the conversation's own origin decides which. A thread
 *  Teams minted FOR a meeting is joined — that meeting is the thing the thread is about, and
 *  it is reachable from here without going to the calendar for a link, which is what this
 *  pair of buttons exists for. Every other chat with somebody in it is called.
 *
 *  A meeting chat therefore does not offer a ring. Both would fit, but they answer the same
 *  question twice ("talk to these people now") and only one of them is what the thread is
 *  for — and the person who wants the other one still has the 1:1s and real Teams. */
export type ConversationCallAction = "none" | "call" | "join";

export function conversationCallAction(
  conversation: Conversation | undefined,
): ConversationCallAction {
  if (!conversation) return "none";
  // The ADDRESS decides, never a flag about the thread's origin: CSA also calls a thread a
  // meeting through `thread_type`, and a row flagged that way whose id names no meeting has
  // nothing to JOIN. Deciding on the flag would leave that row with no control at all,
  // where deciding on the address leaves it the call every other group chat gets.
  if (meetingAddressOf(conversation)) return "join";
  return conversationIsCallable(conversation.kind) ? "call" : "none";
}

/**
 * How a meeting is ADDRESSED, in the two ways the user can reach one.
 *
 * `link` is what a calendar event carries (`onlineMeeting.joinUrl`), in either shape Teams
 * writes one. `thread` is the meeting's own conversation out of the chat list, which is an
 * address on its own — `calling::MeetingJoin::from_thread_id`. Both exist because each
 * covers what the other cannot: this tenant's invitations carry the short `/meet/{code}`
 * link, whose code lives in the calendar event and nowhere in the conversation, while a
 * meeting whose event has rolled out of the synced window still has its chat.
 */
export type MeetingAddress = { kind: "link"; joinUrl: string } | { kind: "thread"; thread: string };

/** The meeting address of a THREAD id, or null when it names no meeting.
 *
 *  A port of the same `19:meeting_` rule the backend applies (`MeetingJoin::from_thread_id`),
 *  so a button is offered only where the backend would agree — and the backend parses it
 *  again, so the worst a disagreement costs is a button that reports a refusal.
 *
 *  It takes the bare id, because a call is reached from two states that hold two different
 *  things: a chat header has the whole {@link Conversation}, and the incoming-call banner has
 *  only the id the awareness event named. One spelling of the rule, for both. */
export function meetingAddressOfThread(id: string | undefined | null): MeetingAddress | null {
  if (!id?.startsWith("19:meeting_")) return null;
  return { kind: "thread", thread: id };
}

/** The meeting address of a conversation, or null when it names no meeting. */
export function meetingAddressOf(conversation: Conversation | undefined): MeetingAddress | null {
  return meetingAddressOfThread(conversation?.id);
}

/** What the call is doing, in the words the UI shows. Written for somebody glancing at
 *  a bar while doing something else, so it names the person once and the state once.
 *
 *  A meeting says different things for the same phases: nobody is being rung, and the
 *  lobby is a state a call does not have. */
export function callPhaseLabel(call: ActiveCall): string {
  if (call.kind === "meeting") {
    if (call.phase === "ended") return "Meeting left";
    if (call.in_lobby) return "Waiting to be let in…";
    if (call.phase === "connected") return callPresenceLabel(call);
    return "Joining…";
  }
  // A GROUP call rang several phones, so who is in it is a fact that CHANGES while it runs
  // and the roster is the only thing that knows: one person may pick up while two never do.
  // Until it is connected it says the same words a 1:1 does — the bar names the conversation
  // beside this, so "Calling…" over a group's title is already the whole sentence.
  if (call.kind === "group" && call.phase === "connected") {
    return callPresenceLabel(call);
  }
  switch (call.phase) {
    case "ringing":
      return "Incoming call";
    case "dialing":
      return "Calling…";
    case "connecting":
      return "Connecting…";
    case "connected":
      return "In a call";
    case "ended":
      return "Call ended";
  }
}

/** Who else is in this meeting or group call, in the fewest words that are true.
 *
 *  One or two people are NAMED — that is what somebody glancing at the bar wants to
 *  know — and a crowd is counted, because six names do not fit and would not be read.
 *  A roster that has not arrived says nothing about it rather than "0 others". */
export function callPresenceLabel(call: ActiveCall): string {
  const others = call.others.filter((name) => name.trim().length > 0);
  if (others.length === 0) return call.kind === "meeting" ? "In the meeting" : "In the call";
  if (others.length <= 2) return `With ${others.join(" and ")}`;
  return `With ${others.length} others`;
}

/** Whether this call is a meeting the user joined. */
export function isMeeting(call: ActiveCall | null): boolean {
  return call?.kind === "meeting";
}

/** Whether this call is about a CONVERSATION rather than one person — a meeting, or a
 *  call that rang a whole group chat.
 *
 *  It is what the UI reads before drawing a face: `peer_mri` is empty on both, so an
 *  avatar seeded from it would be a tinted circle standing in for a group of five. And
 *  it is what makes the roster worth stating, because in both there is more than one
 *  person the answer could be. */
export function callNamesAConversation(call: ActiveCall | null): boolean {
  return call?.kind === "meeting" || call?.kind === "group";
}

/** A running call duration as "0:07" / "12:45" / "1:02:03", from the backend's own
 *  clock so two open pages agree. Returns "" before audio started, because a call that
 *  is still ringing has no duration to state. */
export function callDurationLabel(call: ActiveCall | null, nowMs: number): string {
  if (!call?.connected_at_ms) return "";
  const seconds = Math.max(0, Math.floor((nowMs - call.connected_at_ms) / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`;
}

/**
 * The backend's name for a call that rang nothing, because the callee has no client signed
 * in — `calling::END_REASON_UNREACHABLE`, spelled once here.
 *
 * The pair is pinned from the Rust side (`the_page_knows_the_name_of_an_unreachable_ending`),
 * because two spellings of it would silently put the user back in front of "The call ended."
 */
export const CALL_END_UNREACHABLE = "CallEndReasonNobodyReachable";

/**
 * The backend's name for a ringing call this machine let go WITHOUT declining it —
 * `calling::END_REASON_LEFT_RINGING`, spelled once here and pinned from the Rust side
 * (`the_page_knows_the_name_of_a_ring_let_go`). The user ended it themselves, so it says
 * nothing; unnamed, it would fall to "The call ended." about a call they ended.
 */
export const CALL_END_LEFT_RINGING = "CallEndReasonLeftRinging";

/**
 * The backend's names for a RING that ended somewhere else — answered on another device,
 * forwarded, sent to voicemail, given up by its caller — and for one nobody answered within
 * this app's own ring bound (`calling::END_REASON_*`, spelled once here and pinned from the
 * Rust side by `the_page_knows_the_names_of_a_ring_that_ended_elsewhere`).
 *
 * Unnamed, every one of them falls to "The call ended." — about a call the user missed, or
 * took on their phone, which is the one thing they cannot work out for themselves.
 */
export const CALL_END_ANSWERED_ELSEWHERE = "CallEndReasonAnsweredElsewhere";
export const CALL_END_FORWARDED = "CallEndReasonForwarded";
export const CALL_END_VOICEMAIL = "CallEndReasonForwardedToVoicemail";
export const CALL_END_CALLER_CANCELLED = "CallEndReasonCallerCancelled";
export const CALL_END_UNANSWERED = "CallEndReasonUnanswered";

/** "Missed call from Ana." — a ring the user never took, named after its caller. */
function missedCall(call: ActiveCall): string {
  return call.peer ? `Missed call from ${call.peer}.` : "Missed call.";
}

/** Why the call is over, in one short line — or "" when there is nothing worth saying.
 *
 *  The service's own phrases are machine words ("CallEndReasonHangup"), and the ordinary
 *  endings need no explanation at all: a call the user hung up does not have to be
 *  reported back to them. Only the endings they did not cause get a line. */
export function callEndLabel(call: ActiveCall | null): string {
  const reason = call?.end_reason;
  if (!reason) return "";
  switch (reason) {
    case "CallEndReasonHangup":
    case "CallEndReasonDeclined":
    case CALL_END_LEFT_RINGING:
      return "";
    // The service could not ring anybody: nothing of theirs is signed in (`calling::
    // END_REASON_UNREACHABLE`, and `calling::invite_failed` for how it is known). It reads
    // exactly like this app dropping the call after two seconds, which is what it looked
    // like to the user until the reason was carried this far — so it names the person and
    // it names the cause, and it never says a device rang.
    case CALL_END_UNREACHABLE:
      return call.kind === "group" || call.kind === "meeting"
        ? "Nobody could be reached: none of their devices is signed in."
        : `${call.peer || "They"} could not be reached: no device of theirs is signed in.`;
    // A RING that ended somewhere else (`calling::end_reason_for`). The backend names these
    // only about a call that was still being offered to this machine, so each says what
    // happened to the ring rather than to a conversation.
    case CALL_END_ANSWERED_ELSEWHERE: {
      const by = call.ended_elsewhere;
      if (!by) return "The call was answered elsewhere.";
      if (by.by_self) return "You answered on another device.";
      return by.name ? `${by.name} answered this call.` : "Somebody else answered this call.";
    }
    case CALL_END_FORWARDED:
      return "The call was forwarded.";
    case CALL_END_VOICEMAIL:
      return "The call went to voicemail.";
    case CALL_END_CALLER_CANCELLED:
      return missedCall(call);
    // Nobody answered within this app's own ring bound, and WHICH side nobody was on is the
    // whole sentence: a ring the user did not take is a missed call, a call they placed that
    // nobody took is "they did not answer". A group call has no one name to put there.
    case CALL_END_UNANSWERED:
      if (call.direction === "incoming") return missedCall(call);
      return call.kind === "call" && call.peer ? `${call.peer} did not answer.` : "Nobody answered.";
    case "CallEndReasonPlaceFailed":
      return "The call could not be placed.";
    case "CallEndReasonAcceptFailed":
      return "The call could not be answered.";
    case "CallEndReasonJoinFailed":
      return "The meeting could not be joined.";
    case "CallEndReasonNoAcceptLink":
      return "This call cannot be answered here.";
    case "CallEndReasonReconnected":
      return "The connection moved, so the call ended.";
    case "CallEndReasonCallingTurnedOff":
      // No switch says this any more: this machine stopped being a device calls ring on,
      // which is what the app does as it shuts down.
      return "This machine stopped taking calls.";
    default:
      return "The call ended.";
  }
}

/**
 * Whether a URL is a Teams meeting link this app can join.
 *
 * A small port of `calling::MeetingJoin::from_join_url`, and deliberately only the part
 * that decides whether to OFFER the join: the path names a meetup-join, and it addresses
 * a thread. The backend parses it again and refuses anything it disagrees with, so the
 * worst a mismatch here costs is a button that reports a refusal — never a join to
 * somewhere else.
 */
export function isMeetingJoinLink(url: string | undefined | null): boolean {
  if (!url) return false;
  const base = url.split("?")[0] ?? "";
  // The SHORT shape Teams' newer meetings use: `…/meet/{code}`. It names no thread —
  // the service resolves one from the code — so the code is what makes it a join link.
  const code = base.split("/meet/")[1]?.split("/")[0] ?? "";
  if (code.length > 0 && /^[A-Za-z0-9]+$/.test(code)) return true;
  // The long shape: `…/l/meetup-join/{thread}/{message}`.
  const after = base.split("/meetup-join/")[1];
  if (!after) return false;
  return decodeURIComponentSafe(after.split("/")[0] ?? "").startsWith("19:");
}

function decodeURIComponentSafe(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** Whether a meeting could be joined right now: this backend takes calls, the connection
 *  is up, and this machine is not already in a call.
 *
 *  The one-to-one rule does NOT apply — a meeting is many people by definition, and it is
 *  the service that mixes them. What still applies is one call at a time. */
export function canJoinMeeting(status: CallStatus): boolean {
  return status.enabled && status.ready && !isLive(status.call);
}

/** Why a meeting cannot be joined right now, for a disabled Join control. Empty when it can.
 *
 *  The same three questions, in the same order, as {@link callUnavailableReason} — they share
 *  one helper so the two controls can never give two answers about one machine. Only the
 *  first sentence names the act, because only the first is about what this WINDOW is. */
export function meetingUnavailableReason(status: CallStatus): string {
  return unavailableReason(status, "This window cannot take calls, so it cannot join a meeting.");
}

/** Why calling cannot be used right now, for a disabled call control. Empty when it can.
 *
 *  It names only the three things about this MACHINE, because the control is drawn where a
 *  call is possible in the first place ({@link conversationCallAction}) — a conversation
 *  nothing can be done with has no control to hang a reason off. */
export function callUnavailableReason(status: CallStatus): string {
  return unavailableReason(status, "This window cannot take calls, so it cannot place one.");
}

/**
 * The one decision behind both reasons, in the order the user can act on.
 *
 * 1. **This window does not call at all** (`enabled` false): a read-only backend. There is no
 *    switch, so the sentence says what the window IS rather than what to go and turn on.
 * 2. **The registration is not up** (`ready` false): say WHY, from
 *    {@link CallStatus.registration} — see {@link notReadyReason}.
 * 3. **A call is already live**: one microphone, one call.
 *
 * The order is the point: a read-only window has no registration to explain, and a machine
 * whose registration is down has nothing to say about a call it could not have placed.
 */
function unavailableReason(status: CallStatus, cannotTakeCalls: string): string {
  if (!status.enabled) return cannotTakeCalls;
  if (!status.ready) return notReadyReason(status.registration);
  if (isLive(status.call)) return "This app holds one call at a time.";
  return "";
}

/**
 * Why a backend that calls is not ready, one sentence per registration state.
 *
 * Two rules shape the words. NONE of them promises a time — a sign-in outage or a network can
 * last for hours, and "in a few seconds" that turns out false is a promise this app broke. And
 * none sends the user to do something this machine does on its own: a registration comes back
 * by itself once sign-in or the connection does, which is the whole of what
 * "waiting_for_sign_in" and "connecting" have to say.
 *
 * A registration that is up (`"registered"`, or `"refused"`, which is informational and
 * changes nothing yet) while `ready` is false is a connection the backend has not finished
 * reporting, which is today's sentence — and so are an ABSENT field (a backend too old to
 * say) and a word this build does not know. Read from the wire, so it never assumes the
 * shape it was promised: anything that is not one of the known words falls through.
 */
function notReadyReason(registration: CallRegistration | null | undefined): string {
  const state: unknown = registration?.state;
  switch (state) {
    case "waiting_for_sign_in":
      return "Calls connect on their own once sign-in works again.";
    case "unavailable":
      return "This account's Teams directory names no calling service, so this machine cannot take calls.";
    case "starting":
    case "connecting":
      return "Connecting for calls…";
    case "registered":
    case "refused":
    default:
      return "This machine is not registered for calls yet.";
  }
}
