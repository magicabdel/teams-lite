# Advanced call control: hold, call waiting, keypad, add someone, transfer (and why not park or PSTN)

**Workstream:** `advanced-call-control` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

This design gives teams-lite's one-to-one calls the in-call controls people expect from a phone,
and it does not regress the audio paths verified live on 2026-09-04 (a 1:1 in both directions)
and 2026-08-05 (a meeting join). It covers seven things:

1. **See when the other side holds the call**, and answer that hold the way the client does.
2. **Put a 1:1 on hold and resume it.** The microphone and the speaker go silent on this page at
   the press, whatever the service answers.
3. **Keypad tones (DTMF)** in a 1:1, for a menu that asks for a number.
4. **Call waiting:** a second call that arrives during a call is shown instead of being silently
   dropped. The user can Decline it, or End the current call and answer it.
5. **Survive a colleague adding a third person** to our 1:1 (the media moves to a conference).
6. **Add someone** to a 1:1 the user placed.
7. **Blind (or safe) transfer** of a 1:1.

Every outward act is one explicit press through a gated RPC named in the automation guard. Live
proof comes only from `call-live` with the pinned, consenting colleague, or from the user's own
click. Park, PSTN dial-out, consultative transfer, transfer to voicemail, hold-and-answer (two calls
at once), following a transfer somebody else makes, and "move the call to this device" are deferred
or rejected, each with its reason (§ 3).

**What hold measures for the rest of calling.** An outgoing renegotiation that ADDED a section has
been answered on this tenant (2026-08-06, § 4.0.3). One that only CHANGES THE DIRECTION of sections
that already exist never has. That second kind is exactly how a reserved 1:1 camera is switched on
(video-send § 4.13). So the first local hold (M3) is the cheapest measurement of that mechanism:
audio only, no camera, no SFU. It can still meet glare, because a 1:1 renegotiates on its own
(NATIVE-CALLING.md § 8b).

The design consumes four sibling workstreams instead of duplicating them:

- **incoming-and-conversation:** frame routing by session id (§ 4.0), and which page holds the
  media (§ 4.15);
- **plane-reliability:** bounded signaling requests (§ 4.6), and `call_hangup {intent}` (§ 4.9);
- **media-robustness-devices:** the refusal reader on `/call/rejection/` (§ 4.1), offer purpose,
  page experiment flags, and the stage's More control;
- **video-send:** the SDP section list (§ 4.0.2), and modality parity (§ 4.11).

Where a dependency has not landed when a milestone is ready, the milestone lands exactly the
sibling's piece, under the sibling's name, so the two cannot diverge.

## 2. Where we are today

All references were re-checked against this worktree (HEAD `c941850`) and the 2026-11-05 bundle.

| Area | Today | Where |
| --- | --- | --- |
| One call | `CallingPlane` holds `call: Option<CallSession>`. Its doc says a second call "is left for the user's other devices to ring". | `src/bin/server.rs:2134-2163` |
| A second invite | `handle_incoming_call` builds a full session, then drops it with `busy — leaving this call for the user's other devices to ring`. Nothing is shown to the user. | `src/bin/server.rs:15022-15033` |
| Every ending | `end_call_locally` marks the call `Ended`, emits, then sets `call = None`. Nothing is promoted. | `src/bin/server.rs:15074-15106` |
| Endings from the plane itself | A changed surl stores the NEW channel and then calls `end_call_locally("CallEndReasonReconnected")`. `stop_calling` takes the channel first and then ends with `CallingTurnedOff`. | `src/bin/server.rs:14491-14500`, `:14570-14590` |
| Ring expiry | `expire_unanswered_call` (Ringing and Dialing, 60 s), run on the 20 s `CALL_KEEPALIVE` ticker today; incoming-and-conversation M6 moves it to the 5 s `CALL_TICK` (README § 3, collision 4). | `src/bin/server.rs:757`, `:773`, `:14537`, `:15168-15190` |
| Frame routing | Every non-invite frame's links are merged into the one live call. Any `callEnd`/`conversationEnd` ends it. The callback session id is never compared. | `src/bin/server.rs:14676-14720`; `src/calling.rs:283-291` |
| Renegotiation | Glare refusal while `outgoing_negotiation_at` is under 35 s; otherwise the offer goes to the page. | `src/bin/server.rs:14805-14885` |
| Answer path | Any media answer sets `Connected` and clears `outgoing_negotiation_at`. | `src/bin/server.rs:14889-14944` |
| Our outgoing offer | Door: `mediaRenegotiation`, then `startOutgoingNegotiation`, each with its own envelope. `outgoing_negotiation_at` is set **after** the POST returns. An answer in the response clears it. | `src/bin/server.rs:8230-8300` |
| Declared modalities on an answer | The page always declares `["audio","Video","ScreenViewer"]`. `param_modalities` turns an empty list into `["audio"]`. | `web/src/lib/store.ts:2733-2737`; `src/bin/server.rs:10182-10199` |
| `/call/rejection/` | Published on every offer. No reader exists. | `src/calling.rs:105`, `:1429` |
| Offer content | `MediaContent::json` is `{blob, contentType}`. The client adds `mediaLegId` (and more) on every offer and answer. | `src/calling.rs:228-230` |
| Links | Accessors exist for `reject`, `hangup`, `mediaRenegotiation`, `startOutgoingNegotiation` and others. There is none for `transfer` or `addParticipant`. `collect_links` gathers every URL, `hold` included. | `src/calling.rs:303-454` |
| Accept bodies | `taking_the_call` publishes `mediaRenegotiation`, `transfer`, `replacement`, `end`. There is no `retargetCompletion` on an incoming call; an outgoing call's acknowledgement publishes seven links. | `src/calling.rs:1303-1321`, `:1136-1151` |
| Unreachable | Any `addParticipantFailure` with a no-endpoint code sets `unreachable`, whatever the phase. That would relabel a later ending. | `src/calling.rs:651-659`; `src/bin/server.rs:14697-14710` |
| Call kind | Every incoming invite is `CallKind::Call`, a group call's included. Only an outgoing call that rings several people is `Group`. | `src/bin/server.rs:14983-14988`, `:7560` |
| Hangup | `call_hangup` looks only at `plane.call`, POSTs (no timeout), then always runs `end_call_locally`. `post_call_signal` takes `plane.call`'s id as the correlation. | `src/bin/server.rs:8369-8414`, `:15138-15151` |
| Stage | `[data-testid=call-stage]` carries `data-mode`, `data-phase`, `data-call-id`, `data-sending`, and no conversation id. It is `z-[96]`. Leave is `call-hangup`. | `web/src/components/call-stage.tsx:168-176`, `:194`, `:891` |
| Ringing card | Decline is ALSO `data-testid="call-hangup"`. The stack is `z-[95]`. | `web/src/components/call-bar.tsx:51`, `:150-156` |
| Menus | The shared `DropdownMenuContent` is `z-50`, which is under the stage. | `web/src/components/ui/dropdown-menu.tsx:54` |
| Drivers | `hangUp` clicks the FIRST `call-hangup` on the page, unproven. `call-live` runs it in `finally`. | `web/scripts/join-live.ts:1253-1259`; `web/scripts/call-live.ts:186-190` |
| Awareness card | Filters only the LIVE call's conversation. | `web/src/components/incoming-call-banner.tsx:39-43` |
| Keypad | `web/src/` has no `insertDTMF`. Calls ring only `8:` mris. | `src/bin/server.rs:7544` |

## 3. Gaps covered, deferred and rejected

**Covered**

- `hold-resume`: remote-hold detection (M1) and local hold/resume (M3). This corrects round 1:
  `/call/holdCompletion/` and `/call/resumeCompletion/` are PARK callbacks, and the `hold` link is
  the park door.
- `call-waiting`: the second ring is kept, with Decline and End-and-answer (M4).
- `add-participant-escalation`: surviving an inbound escalation (M5), and adding someone to our own
  outgoing 1:1 (M6).
- `outgoing-transfer-consult`: blind/safe transfer (M7).
- `pstn-dialpad-dtmf`: the keypad (M2), and naming an incoming `4:` caller by number (M2).
- `call-park`: rejected, with its protocol recorded so nobody posts to `hold` for Hold.

**Deferred or rejected**

| Item | Decision | Why |
| --- | --- | --- |
| Park / unpark (`call-park`) | REJECTED | User value 1. It needs a tenant Call Park policy nobody has confirmed, and a pickup needs another device that has the code. It is a different door (`hold`, stored by the client as `LINKS.PARK`), not a variant of hold. A Rust test keeps any `Links` accessor and `call_hold` from naming `"hold"` (M3). |
| Server hold with music (`musicOnHold`) | REJECTED | It posts to the same park door and moves the call leg off this endpoint to a music server. From the user's side the call would vanish. |
| Outgoing PSTN dial and number entry | DEFERRED | The licence is unknown (the client learns `isPSTNEnabled` from an app-layer GraphQL subscription), calls cost money, and emergency calling carries obligations (`emergencyContent`, `locationContent`, `emergencyGroupCall`). If built, it must be its own OUTWARD method (`call_dial`), refusing emergency and short numbers with a sentence. Protocol in § 4.13. |
| Consultative transfer | DEFERRED | It needs two calls at once: the consult call's `replacement` link becomes `replacementDetails` on the first call's transfer. teams-lite holds one call and one peer connection. |
| Transfer to voicemail | DEFERRED | Low value. Once M7 exists it is the same body with `target.endpointType: "voicemail"`. |
| Hold-and-answer, merge | DEFERRED (XL) | Several sessions with frame routing per session, a second `CallMedia` sharing one microphone, and a UI listing two calls. End-and-answer covers the common case. Revisit after M3 is proven live. |
| Following a transfer somebody else makes (we are the transferee) | DEFERRED, read-only notice built in M7 | Deferred here by incoming-and-conversation (its § 3). The client answers a `callTransfer` on `/call/transfer/` with `transferAcceptance {}` and then PLACES A NEW CALL to the target while the first one is still up. That rings a third party, so it needs the user's press, and it needs two calls at once. M7 only reads the frame and says who tried to transfer the user to whom (§ 4.10). Nothing is posted, not even a refusal: a refusal is something the transferor is SHOWN, which no timer may decide. |
| "Take the call here / move it to this device" | DEFERRED | Deferred here by incoming-and-conversation. It needs a transfer to our own endpoint and the transferee machinery above. |
| Redirect (`/call/redirection/`) and replacement (`/call/replacement/`) frames | DEFERRED | incoming-and-conversation M2 already journals their key names. A redirect is an ending code (`CALL_REDIRECTED:302`). |
| Add someone to an INCOMING call | DEFERRED | An incoming 1:1 never joins the conversation, so it holds no `addParticipant` link. Add is offered only where the link exists (today: outgoing calls). It waits for incoming-and-conversation's join-at-answer (M4a). |
| Redirect a ringing call to voicemail | REJECTED for now | The forked invite carries no `redirect` door (measured, NATIVE-CALLING.md § 8a). Declining already lets the service apply the callee's own unanswered-call settings. |

## 4. Protocol

### 4.0 Building blocks every action uses

#### 4.0.1 Frame routing by call session (owned by incoming-and-conversation § 4.0)

Every callback this app publishes is `{surl}callAgent/{sessionId}/{causeId}{path}`
(`CallbackBase::link`, `src/calling.rs:283-291`). incoming-and-conversation's M0 reads that segment
(`calling::session_of`) and ignores a frame whose segment names no live session, journaling it
once per `(session, path)`. A frame without the segment keeps today's handling.

This workstream depends on it for one thing: after End-and-answer (M4), the ENDED call's late
`callEnd`/`conversationEnd` must not end the promoted call. Without routing they would:
`handle_call_frame` merges every frame into whatever call is live (`:14676-14720`).

**M4's promotion is switched on only after a proof.** The observation journal must show zero
"another session" lines touching the live call, and no un-segmented frame acting on it, over:

- one `call-live` run;
- one `join-live -- --share --pair` run;
- one incoming call the user answers.

These are the three provenances (outgoing, meeting, incoming). If incoming-and-conversation's M0
has not landed, M0 here lands exactly its § 4.0 under its names.

#### 4.0.2 The renegotiation door and the pending-offer bookkeeping

- **One helper, shared with `call_offer_media`.** `calling::offer_door(links) -> Option<(url,
  OfferDoor { Renegotiation, StartOutgoing })>` returns the door, and
  `calling::offer_payload(door, local, offer, callbacks, modalities)` returns the envelope that
  door requires:
  - `media_offer_payload` for `mediaRenegotiation`;
  - `start_outgoing_negotiation_payload` (which adds the `mediaRenegotiation` link the service
    named in a 400) for `startOutgoingNegotiation`.

  The order stays `mediaRenegotiation` first, which is the client's own door
  (`startRenegotiationAsync` posts to `this.links[I.LINKS.MEDIA_RENEGOTIATION]`).
- **The pending state is recorded BEFORE the POST**, under the lock, and rolled back if the POST
  fails. Today `call_offer_media` sets `outgoing_negotiation_at` after the POST returns
  (`:8262-8275`). An answer frame that lands while the POST is in flight therefore clears nothing,
  and the POST's return then re-opens a 35 s glare window that refuses the service's own
  renegotiations. The client moves its FSM to `OUTGOING_RENEGOTIATION` before it posts:

  ```js
  // calling-pluginless.js @1076731 (startRenegotiationAsync)
  switch(this.fsmState){case I.MEDIA_RENEGOTIATION_FSM_STATE.CALL_CONNECTED:this.setFsmState(I.MEDIA_RENEGOTIATION_FSM_STATE.OUTGOING_RENEGOTIATION,a);break;
  case …OUTGOING_RENEGOTIATION:case …INCOMING_RENEGOTIATION:case …RENEGOTIATION_GLARE:return …this.signalingSessionCallback.onMediaRenegotiationRejection(this.getGlareError(),a),Promise.resolve(null);…}
  … this.signalingSession.http.sendPostRequest({url:n,payload:_V(this.signalingSession,r,s),…})
  ```

  The order fix is part of the shared helper, so `call_offer_media` gets it too. Tell video-send.
- **An answer and a refusal both settle a hold through ONE helper**
  (`Ctx::settle_hold(call_id, outcome)`), called from the POST response path, the answer frame
  branch (`:14889`), the refusal reader (§ 4.0.4) and the hold timer. A late answer that arrives
  after the timer has fired finds `hold_requested` already cleared, and changes nothing.
- **The bound is per request.** It is a spawned `tokio` sleep of `OUTGOING_NEGOTIATION_TIMEOUT_MS`
  (35 s, the client's `mediaAnswerTimeoutSec:35`), not the 20 s keep-alive tick. The page's own
  `boundTheMediaAnswer` fires at the same 35 s (`store.ts:1102`, `:2970`), so two pages cannot
  disagree for 20 s.

#### 4.0.3 What is measured about an outgoing renegotiation

- **2026-08-06, meeting, measured.** An offer adding `applicationsharing-video` was answered on a
  frame: `a media answer arrived: audio mid=0 label=main-audio accepted | video REJECTED`
  (NATIVE-CALLING.md § 10.8).
- **2026-08-06, real 1:1, measured.** A screen offer was answered, with the new section zeroed. The
  answer arrived in the same second, so it was read as being in the POST response; that part is
  inferred from timing (§ 10.8).
- **2026-09-04, meeting, measured.** A camera offer got no answer at all. The cause was glare: the
  app then APPLIED the service's own offer (§ 8, "IT WAS GLARE").
- **Not measured.** Since the glare refusal and the door order landed, no outgoing renegotiation
  has been answered on a 1:1. No offer that only changes directions, which is what a hold is, has
  ever been sent.

#### 4.0.4 A refusal of OUR offer (owned by media-robustness-devices § 4.1)

The service posts a `mediaNegotiationFailure` to our `/call/rejection/`, and the client reads
`r.body.mediaNegotiationFailure` (`this.handleMediaNegotiationFailure=r=>{…}`, calling-pluginless.js
@1079893). media-robustness-devices M6 adds the reader. It clears `outgoing_negotiation_at`, emits
`call_media {call_id, kind:"refused", code, sub_code, phrase}`, and the page routes it by the
pending offer's PURPOSE (`send | reconnect | takeback`).

This workstream adds two purposes, `hold` and `resume`. If that reader has not landed when M3 is
ready, M3 lands it under the same names.

The expected inbound body, read from the bundle (the shape this app already sends on glare,
`media_glare_rejection_payload`):

```json
{ "mediaNegotiationFailure": {
    "sender": { "id": "8:orgid:…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
    "code": 491, "subCode": 3118, "phrase": "NegotiationIsInProgress" } }
```

`491 / 3118 / NegotiationIsInProgress` is the client's glare error (`getGlareError`,
`@1083346`). Any other code is a plain refusal.

#### 4.0.5 Sections and declared modalities (shared with video-send § 4.0.2 and § 4.11)

- `calling::media_section_list(sdp) -> Vec<Section {kind, mid, label, direction, rejected}>`
  comes from video-send. `direction` is the section's own attribute, else the session-level one,
  else `sendrecv`. If it has not landed when M1 is ready, M1 lands it under that name.
- `calling::signaling_media_types(&[Section]) -> Vec<&'static str>` is the client's
  `getSignalingMediaTypes`, in teams-lite's own spellings (`MODALITY_AUDIO` is `"audio"`,
  `src/calling.rs:50`):
  - `audio` unless the audio section is inactive;
  - `Video` only for a `main-video` section that is `sendonly` or `sendrecv`;
  - `ScreenSharer` for an `applicationsharing-video` section that is `sendonly`;
  - else `ScreenViewer` for a sharing section that is `recvonly`.

  It is the same derivation video-send's `modalities-parity` experiment uses, spelled once, and
  **video-receive M1 owns it** (README § 3, collision 8). video-receive's one answer function,
  `calling::declarable_modalities` (video-receive § 4.6), is built on it, and this workstream's
  held-one-to-one `[]` answer is a flag of that function, not a second derivation. This workstream's
  own hold OFFER (§ 4.1) calls `signaling_media_types` directly. Whoever lands it first lands it
  under this name.

```js
// calling-pluginless.js @1862757
getSignalingMediaTypes(t){const i=n=>n===this.mediaAgent.constants.MEDIA_STATE.send||n===this.mediaAgent.constants.MEDIA_STATE.sendReceive,r=[];return t.audio&&t.audio!==this.mediaAgent.constants.MEDIA_STATE.inactive&&r.push("Audio"),i(t.video)&&r.push("Video"),t.sharing===this.mediaAgent.constants.MEDIA_STATE.send?r.push("ScreenSharer"):t.sharing===this.mediaAgent.constants.MEDIA_STATE.receive&&r.push("ScreenViewer"),r}
```

- `calling::sdp_hold_state(sdp) -> Option<bool>`:
  - `Some(true)` when the first live audio section is `inactive` and every other live section is
    `inactive`;
  - `Some(false)` when the audio section is `sendrecv`;
  - `None` otherwise. A `sendonly`/`recvonly` audio (the SIP-style hold) is not called a hold: the
    client's `modalitiesAreHold` requires `inactive`.

```js
// calling-pluginless.js @1963129
modalitiesAreHold(t){return(!t.audio||t.audio==="inactive")&&(!t.video||t.video==="inactive")&&(!t.sharing||t.sharing==="inactive")}
```

#### 4.0.6 One-to-one is a predicate, not a kind

`CallKind::Call` is set for EVERY incoming invite, a group call's included (`:14983-14988`), so
`kind` cannot answer it. **There is ONE one-to-one predicate in the app, and video-receive M1 owns
it** (video-receive § 4.8; README § 3, collision 6): `CallSession::is_one_to_one()` =
`multi_party == Some(false)`, from the service's own `isMultiParty` (the outgoing ring length, the
invite's `conversationInvitation.isMultiParty`, and any later `conversationUpdate.state`, which is
also how an escalation flips it). An unknown call is never one-to-one, which keeps this
workstream's rule that an invite nobody can classify is not offered hold.

This workstream does NOT redefine it. What it adds is ELIGIBILITY for its own controls, layered on
top, in `CallSession::controls_eligible()`:

- `is_one_to_one()` and `!escalated`;
- `others().len() <= 1`;
- `peer_mri` starts with `8:` (a person, not an app or a number).

The thread shape (`@unq.gbl.spaces`) is no longer part of any predicate. M0 still journals the shape
of the next incoming invite's thread (`unq.gbl.spaces`, `thread.v2` or none) beside whether it
carried `isMultiParty`, which is the evidence that the service's word is present on this tenant.

Hold, keypad, add and transfer, the remote-hold detection, and the `[]` declaration of a hold
answer all require that eligibility. A meeting renegotiation's answer keeps exactly today's list,
whatever its directions say. `call_state` carries no `one_to_one` field of this workstream's; the
page reads `multi_party` and this workstream's `controls`.

#### 4.0.7 Switches

| Switch | Kind | Default | Who sets it | Promoted when |
| --- | --- | --- | --- | --- |
| `teams-lite:call-exp:hold` | page flag (media-robustness `call-experiments.ts`) | off | `call-live` in its own context; the user | M3 step 3 succeeds → on for OUTGOING 1:1 |
| `teams-lite:call-exp:hold-incoming` | page flag | off | the user | the user's own incoming hold-and-resume works |
| `teams-lite:call-exp:add` | page flag | off | the user | M6's live step, including the privacy check |
| `teams-lite:call-exp:transfer` | page flag | off | the user | M7's live step |
| `accept_links: measured \| client` | backend: a field of incoming-and-conversation's `CallProtocol` setting, else env `TEAMS_LITE_CALL_ACCEPT_LINKS=client` on a backend the user starts | `measured` | the user | the user's own incoming call connects with audio both ways |

How the user turns a page flag on for their own click is a decision (§ 12, D2). A flag decides
only whether the UI offers a row. Each RPC stays gated by the write token, and the guard blocks
scripts from naming it.

### 4.1 Put a 1:1 on hold (an outgoing renegotiation with every section inactive)

- **Method:** POST.
- **Link:** the door from § 4.0.2. `mediaRenegotiation` is measured on the incoming acceptance's
  answer: `["callLeg", "controlVideoStreaming", "hold", "mediaRenegotiation", …,
  "startOutgoingNegotiation", "transfer", …]` (NATIVE-CALLING.md § 8a). On an outgoing call it
  arrives on the `callAcceptance` frame (read from the bundle:
  `this.links[I.LINKS.MEDIA_RENEGOTIATION]=e.callAcceptance.links.mediaRenegotiation`, @1386651).
  **Never the `hold` link**, which is the park door (§ 4.12).
- **Body** (no `payload` envelope, NATIVE-CALLING.md § 2.3):

  ```json
  { "mediaNegotiation": {
      "sender": { "id": "8:orgid:…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
      "callModalities": [],
      "links": {
        "mediaAnswer": "{surl}callAgent/{session}/{cause}/call/mediaAnswer/",
        "rejection":   "{surl}callAgent/{session}/{cause}/call/rejection/" },
      "mediaContent": { "blob": "<toMsSdp(offer): every live m-section a=inactive>",
                        "contentType": "application/sdp-ngc-1.0" } } }
  ```

  On the fallback door the envelope is `startOutgoingNegotiation` with `mediaRenegotiation` added
  to `links`. `callModalities` is `signaling_media_types` of the SDP's own sections, which is `[]`
  for a hold. It is never `param_modalities`, which would turn `[]` into `["audio"]`.
- **The body validated by the backend.** `call_hold {held:true}` requires
  `sdp_hold_state(sdp) == Some(true)`. Without that check, a "hold" could carry a camera section
  that the backend's `sending` list never records.
- **A divergence, stated.** The client's `mediaContent` is the offer object from
  `createOfferAsync`. It carries `mediaLegId` (an uppercase uuid, adopted or minted once per call:
  `i.mediaLegId=this.mediaLeg.getId(i.mediaLegId)`, @2326395) and may carry `clientLocation`, a
  bandwidth seed and, on a 1:1 offer, `requiredFeatures:"nonByPass"`. teams-lite sends
  `{blob, contentType}`. Parity belongs to video-send (`video-sdp-mediacontent-parity`), so the hold
  offer inherits whatever it makes default. M0 journals the SHAPE of every `mediaContent` the
  service sends us: key names, and whether `mediaLegId` and `fromMixer` are present. That makes the
  first M3 run name whether the service echoes a leg id this app never sends.
- **Callbacks:**
  - the answer on our `/call/mediaAnswer/` (`media_answer_from_frame`, `:14891`), or in the POST
    response;
  - a refusal on `/call/rejection/` (§ 4.0.4);
  - both bounded at 35 s (§ 4.0.2).
- **Glare.** A service offer that arrives while ours is pending is refused by the existing branch
  (`:14811`). If the service refuses OURS with 491 / 3118, the page re-offers the hold ONCE, after it
  has answered the service's next offer or after 3 s, whichever comes first. The microphone stays
  silenced throughout. This is the client's `retryOutgoingRenegotiation`, read from the bundle.
- **Bundle evidence** (calling-pluginless.js):

  ```js
  // @1777712
  async hold(t,i=Ie(),r,n){…return …this.state===4?(…Promise.resolve(void 0)):r?.isLocal?this.localHold(i,r,n):this.holdWithRenegotiate(i,r)}
  // @1862185
  getMediaModalitiesForHold(t){…const i="inactive",r={audio:i,video:i,data:this.allowDataChannel()?i:void 0,sharing:i};return this.deleteExtraneousModalities(r,t),r}
  // @1919742 renegotiateOutgoing
  s=this.inLocalHold()?this.getMediaModalitiesForHold(t):this.calculateModalityDirections({},t);…const o=await a.execute("MediaCreateOffer",…),l=this.getSignalingMediaTypes(s);await a.execute("SignalingStartNegotiation",()=>this.signalingSession.startRenegotiationAsync(o,l,t))
  // @1946453
  isValidHoldResumeCallState(t){return[3,5,10,13].includes(t)}
  ```

  The client's `setHold` skips the renegotiation entirely when the hold is LOCAL (mute-on-hold).
  Which path a Teams client's own Hold button takes is not in the fetched chunks (§ 4.3, § 12 Q2):

  ```js
  // @1782947
  t?this.isMuteOnHold=r&&r.isLocal&&this.state!==5:this.prepareModalitiesForResume(i),!this.isMuteOnHold||!t&&this.callUsesMixer&&…renegotiateOnHoldAfterEscalation){const m=await this.updateMediaModalities(void 0,i);…}
  ```

- **Status:** read-from-bundle. The door is measured to answer an offer that ADDS a section
  (§ 4.0.3). A directions-only offer is unmeasured.

### 4.2 Resume a call we held

- **Method / link:** as § 4.1.
- **Body:** the same envelope. Each transceiver's direction is restored to its value before the hold
  (a `mid → direction` map kept in `call-media.ts`). `callModalities` comes from the restored SDP:
  `["audio"]`, plus `ScreenViewer` when a sharing section is `recvonly`.
- **Validation:** `call_hold {held:false}` requires audio `sendrecv` and `signaling_media_types` ⊆
  `{audio, ScreenViewer}`. Hold is refused while anything is being sent beyond audio, so a resume
  never needs `Video` or `ScreenSharer`.
- **Resume always signals**, even after an unconfirmed hold. The far side may have applied our hold
  offer although its answer never reached us, and a `sendrecv` re-offer to a peer that was never
  held is a harmless renegotiation.
- **Callbacks:** as § 4.1.
- **Bundle evidence:**

  ```js
  // calling-pluginless.js @1778612
  async unhold(t,i=Ie()){…return this.state===3?(…Promise.resolve(void 0)):this.state===4?(r.info(`current hold state ${this.requestedHoldState}`),this.setHold(!1,i)):(…Promise.reject(…))}
  ```

- **Status:** read-from-bundle; unmeasured.

### 4.3 Detect that the OTHER side held us or resumed, and answer it

- **Method:** POST (our answer).
- **Link:** the offer frame's own `mediaAnswer` (`MediaRenegotiation.answer_link`,
  `src/calling.rs:988-1046`), as today.
- **Detection** (backend, on the NON-glare path of `:14811`, and only when
  `controls_eligible()`, § 4.0.6):
  - Skipped while we hold (`held_by_me`) or have a hold request out. An inactive offer then merely
    reflects our own hold; the client tells the two apart with its own call state (`LocalHold` 4
    vs `RemoteHold` 5).
  - `sdp_hold_state(offer) == Some(true)` sets `held_by_them`.
  - `Some(false)` while `held_by_them` clears it.
  - Journal: `[calling] the other side put the call on hold` / `… resumed it`.
- **Answer body:** today's `media_answer_payload` (`src/calling.rs:1357-1379`). `callModalities` is
  `[]` only when the call is one-to-one AND the answer itself is all-inactive. Every other answer
  keeps exactly what the page declares today.

  ```json
  { "mediaAnswer": {
      "sender": { … }, "callModalities": [],
      "links": { "mediaAcknowledgement": "{surl}callAgent/{session}/{cause}/call/mediaAcknowledgement/" },
      "clientContentForMediaController": { "controlVideoStreaming": "…", "csrcInfo": "…" },
      "mediaContent": { "blob": "<answer: audio a=inactive>", "contentType": "application/sdp-ngc-1.0" } } }
  ```

- **Callbacks:** none new.
- **Bundle evidence:**

  ```js
  // calling-pluginless.js @1946020
  getRenegotiationOfferType(t){if(t){if(this.state===5){if(t.audio==="sendrecv")return 2}else if(this.modalitiesAreHold(t))return 1}return 0}
  // @1946847 onNegotiationCompletion — RemoteHold is set from a negotiation ONLY on a non-mixer 1:1
  const o=this.isOneToOneCall();if(o&&!this.callUsesMixer){const b=this.modalitiesAreHold(t.activeModalities)?5:3;this.participants[0].setState(b,void 0,n),…}
  // calling-app.js @2011636
  case P.CallState.RemoteHold:return Ae.stringTranslate("calling","calling_hold_remote_ui_message")
  ```

- **Three outcomes are possible when a Teams client presses Hold, and "nothing arrives" is one of
  them:**
  1. an all-inactive renegotiation (`holdWithRenegotiate`);
  2. a LOCAL mute-on-hold, which sends no frame at all (`isLocal` skips `updateMediaModalities`);
  3. on a call through a media processor (`callUsesMixer`), the hold renegotiates with the mixer,
     and our leg sees no change. The client itself sets `RemoteHold` from a negotiation only when
     `!this.callUsesMixer`.

  M0 journals `fromMixer` from the acceptance's `mediaContent`. The UI never implies that the
  absence of a caption means the call is not held.
- **Status:** a 1:1 renegotiating on its own is MEASURED (NATIVE-CALLING.md § 8b). How a Teams Hold
  reaches us is unknown and is measured by M1's live step.

### 4.4 Send keypad tones (DTMF)

- **Method:** none. RFC 4733 `telephone-event` RTP on the call's existing audio sender, on the page
  that holds the media.
- **Body:** `sender.dtmf.insertDTMF((sender.dtmf.toneBuffer || "") + tones, 200, 100)`. `tones` is
  restricted to `[0-9*#]` and capped at 32 per batch. The client's keypad maps 0-11 only
  (`toneToString`), so `A-D` and `,` are refused. `canInsertDTMF` is the browser's own statement
  that `telephone-event` was negotiated.
- **Callbacks:** none. `ontonechange` counts the tones that went out (local only).
- **Offered only on a one-to-one call** (§ 4.0.6). In a meeting the events would reach the
  conference and possibly everybody in it, which is a reach the keypad's copy does not describe.
- **Bundle evidence:**

  ```js
  // calling-pluginless.js @2474908
  sendDtmf(e,t){return e?.getSenders?(this.syncSender(e),this.webRtcSender?(this.webRtcSender.ontonechange=this.onToneChange.bind(this),this.webRtcSender.insertDTMF((this.webRtcSender.toneBuffer||"")+t,this.configProvider.config.dtmf.toneDuration,this.configProvider.config.dtmf.toneGap),this.queue.waitForNotification(t)):Promise.reject(new Error("not available"))):Promise.reject(new Error("bad peerConnection"))}
  // @3059626
  dtmf:{toneDuration:200,toneGap:100}
  // @3089681 (one of the client's audio codec configurations)
  …{channels:1,clockRate:8e3,mimeType:"audio/telephone-event"}]
  ```

  `runtime.js` names the chunk `lazy-calling-dtmf-keypad`.
- **Reach, stated.** teams-lite rings only `8:` people (`:7544`), so a menu that reads tones is
  reached today only through the callee's own forwarding (a call queue, an auto attendant). How an
  auto attendant is addressed from Teams is not in the fetched chunks (§ 12 Q3).
- **Status:** read-from-bundle. The browser half is measurable locally. Which `telephone-event`
  clock rate the service's answer keeps (`/8000` only, in the client's list, against Chrome's
  `/48000` and `/8000`) is unmeasured.

### 4.5 A second call while in one: keep it as a WAITING ring, and Decline it

- **Kept only while the live call is `Connecting` or `Connected`.** In any other phase today's busy
  drop stays. In particular, while our own call is `Ringing`, a second ring is left alone: it may
  be the same call re-forked, or a phantom ring on an install where the user answered elsewhere.
  Also dropped:
  - an invite whose `call_id` equals the live or the waiting call's id (a re-delivery);
  - a second waiting call: the cap is 1.
- **Never attached.** A waiting invite claims no leg. Claiming one while the user is in another
  call would be an automatic act. Even under incoming-and-conversation's attach-at-ring (M4b), a
  waiting invite is not attached.
- **Decline:** POST.
  - **Link:** the waiting invite's own `reject`, which a forked invite carries (measured,
    NATIVE-CALLING.md § 8a).
  - **Body:** whatever the ringing Decline posts. Today that is `rejection_payload`:

    ```json
    { "callRejection": { "sender": { … }, "code": 603, "phrase": "Decline" } }
    ```

    If incoming-and-conversation's `decline_payload` (`mM`, possibly a DELETE) lands, the waiting
    decline takes it with the ringing one: one decline, one shape.
  - **Correlation:** the WAITING call's own id. `post_call_signal` reads `plane.call`'s id
    (`:15138-15151`), which would correlate the decline to the call the user is in. A new
    `Ctx::post_call_signal_as(correlation, url, payload)` takes the id explicitly.
- **Expiry:** the service never closes a ring that was never attached (measured, § 8a). So a
  waiting ring expires on a per-invite timer at `CALL_RING_TIMEOUT` (60 s) from its own
  `started_at_ms`. It is journaled, and emits `waiting: null` with a one-frame
  `waiting_missed: {peer}`. Nothing is posted: declining on the user's behalf is something the
  caller would be shown.
- **Bundle evidence:** the client holds several calls (`canMerge(t,i){…isOneToOne ${r}
  isCallConnected ${n} …}`, @1779081; `runtime.js` chunk `web-multi-call-monitor`).
- **Status:**
  - the `reject` link on a forked invite is measured;
  - whether rejecting it stops the user's other devices ringing is unmeasured;
  - no decline has been measured live at all (plane-reliability § 4.9).

### 4.6 End the current call and answer the waiting one

- **Method:** POST ×3, all existing and measured: the active call's hangup, then the promoted
  invite's `attach` and the attach answer's `acceptance` (§ 8a).
- **Bodies:** unchanged: `hangup_payload` (`src/calling.rs:1522`), `attach_payload` (`:1333`),
  `acceptance_payload` (`:1343`).
- **Promotion** (in `end_call_locally`, after `call = None`):
  - It happens only when `plane.channel.is_some() && plane.connected`, the waiting ring has not
    expired, and the ending is not `CallingTurnedOff`.
  - The waiting session's `CallbackBase` is REBUILT from the current channel. Nothing was published
    on the old one (the invite was never attached), and a surl change ends calls through this very
    function (`:14491-14500`). Answering with callbacks on a dead surl would tell the caller the
    call connected while no frame could ever reach us.
  - On `CallingTurnedOff`, the waiting ring is dropped with a journal line.
- **Order.** The page presses End, `call_hangup {intent:"end"}` runs as today, and the promotion
  emits `call` = the waiting session at `ringing`. The page waits up to `HANGUP_WAIT` + 3 s (15 s)
  for that frame, then runs `answerCall()`.
  - The hangup POST must be bounded, or a hung POST costs the user both calls: plane-reliability
    § 4.6 (`HANGUP_WAIT` = 12 s; the End class is 10 s).
  - A waiting ring that expired meanwhile ends in the notice "You missed a call from {name} while
    you were on another call."
- **Callbacks:** unchanged. The OLD call's late frames are dropped by § 4.0.1.
- **Stated limit.** A waiting caller who gave up is unknown here until the 60 s timer, because
  the ring was never attached. "End call and answer" can therefore meet a call that is gone; it
  says "The call could not be answered." If the user answered the second call on another device,
  the attach is refused, with the same sentence.
- **Status:** each POST is measured; the promotion is new.

### 4.7 Survive the other side turning our 1:1 into a group call

- **Method:** POST (our answer, through the existing renegotiation path).
- **What arrives:** a `mediaNegotiation` whose `mediaContent` carries:
  - `isTwoPartyToMultiPartyEscalation: true`;
  - and/or `newOffer: true` (a retarget to the conference: new ICE ufrag/pwd and a new DTLS
    fingerprint);
  - and possibly `escalationOccurring`.

  `MediaContent::parse` drops these fields, so `MediaRenegotiation` gains `escalation`, `retarget`
  and `escalation_occurring` booleans, read from the raw `mediaContent`.
- **The client never answers a retarget on its existing connection.** `processOfferAsync` sends
  every `newOffer` through `reconnectManager.acceptReconnectStartAsync`, which creates a reconnect
  SESSION (a new peer connection), mutes the current one, and swaps over in `acceptReconnectFinish`:

  ```js
  // calling-pluginless.js @2327189
  processOfferAsync(e,t){…e.mediaLegId=this.mediaLeg.getId(e.mediaLegId);try{e.newOffer&&await this.reconnectManager.acceptReconnectStartAsync(e.escalationOccurring,t);const i=await this.getCurrentSession().processOfferAsync(e,t);if(e.newOffer){const r=await this.reconnectManager.acceptReconnectFinish(t);…}
  // @2312529
  async acceptReconnectStartAsync(e,t){…e?(this.escalationType=2,…):(…),…try{await this.initReconnect(t)}…}
  // @2318914
  async initReconnect(e){…await this.resetReconnectSession(e),this.setSignalingState(!1),this.session&&(this.blockCurrentSession(e),await Promise.all([this.reconnectSession.muteInputAsync(e),this.reconnectSession.muteOutputAsync(e)]))}
  ```

  **So the plan is a SECOND `RTCPeerConnection` that takes over.** On a retarget offer:
  1. the page builds a new connection with the call's ICE servers;
  2. it applies the offer and adds the microphone track;
  3. it answers through the existing `call_answer_media`, on the offer's own `mediaAnswer` link;
  4. it keeps the old connection until the new one is ICE-`connected` and inbound audio bytes grow,
     or a `retargetCompleted` code 0 arrives, whichever comes first;
  5. then it closes the old one.

  A Chrome experiment (`webrtc-retarget.spec.ts`) may show that one connection can follow a
  fingerprint change. That would be an optional optimisation, never the plan: a browser
  tolerating it says nothing about what the service expects of the answer.
- **While an escalation is in progress** (`escalated && !retarget_settled`), `call_hold`,
  `call_add_participant` and `call_transfer` are refused. The client ignores negotiations then:

  ```js
  // @1913117
  executeNegotiation(t,i,r,n,s){…if(this.isCallModeStreaming()||this.isEscalationInProgress)return a.info(`ignored…`),!1;
  ```

- **Callbacks:** `/call/retargetCompletion/` receives `{retargetCompleted:{code,…}}`. Code 0 is
  success; anything else is a failure, and the client ends the call:

  ```js
  // @1083346
  this.handleRetargetCompleted=r=>{const n=r.body.retargetCompleted,…n.code===0?this.signalingSessionCallback.onReTargetCompletedSuccess(s):this.signalingSessionCallback.onReTargetCompletedFailure(n,s)}
  ```

  teams-lite publishes `retargetCompletion` only on OUTGOING calls
  (`acceptance_acknowledgement_payload`, `src/calling.rs:1136-1151`). The client's acceptance `qV`
  publishes seven links (@1169867): `mediaRenegotiation`, `transfer`, `replacement`,
  `balanceUpdate`, `retargetCompletion`, `controlVideoStreaming`, `updateMediaDescriptions`.
  Adding the missing four to the incoming bodies is the `accept_links` switch (§ 4.0.7). It changes
  only `taking_the_call`'s `links` object, and NOT in the same release as incoming-and-conversation's
  attach change: one variable at a time, the rule that design states for `qV`.
- **The page reports escalation as a flag, never a kind.** `call_state` gains `escalated: bool`.
  `kind` is untouched, so an incoming group call's UI does not change.
- **Status:** read-from-bundle; never observed on this tenant. The failure mode today is probably a
  silent leg after a colleague adds someone: `answerRemoteOffer` never ends the call, and the old
  transport cannot follow a new fingerprint.

### 4.8 Add someone to a 1:1 we placed

- **Method:** POST.
- **Link:** the client's rule. `addParticipantAndModality` when the session has a thread or a
  group, otherwise `addParticipant`. Both come from the CONVERSATION's links:

  ```js
  // calling-pluginless.js @1243025
  o.groupId=this.groupId||o.groupId,o.threadId=this.threadId||o.threadId;const u=o.threadId||o.groupId?this.links[I.LINKS.ADD_PARTICIPANTS_AND_MODALITY]:this.links[I.LINKS.ADD_PARTICIPANT]
  // @1345210
  this.links[I.LINKS.ADD_PARTICIPANT]=e.links.addParticipant,this.links[I.LINKS.ADD_PARTICIPANTS_AND_MODALITY]=e.links.addParticipantAndModality
  ```

  The wire name is `addParticipantAndModality` (singular); the client's constant is spelled
  `addParticipantsAndModality`. Every 1:1 teams-lite places from a chat carries `groupChat.threadId`
  (`src/calling.rs:1280`), which the captured client invitation does too (NATIVE-CALLING.md § 2.3).
  So the client's rule picks the threaded door. If the threaded add is refused with a 4xx that
  names a field, the unthreaded `addParticipant` is tried once and the journal names both.
  A meeting join's links include `addParticipant` (measured, § 2.3a). The `call_place` reply already
  returns `placed.links.names()` (`:7711`), so the next `call-live` run states whether an outgoing
  1:1 carries it.
- **Bodies.** No `payload` envelope. Keys the client leaves `undefined` are OMITTED, not sent as
  `null`. `participantInvitationData` is `{}` (`XP` returns `{}` with no options).

  Threaded (`vT`, @1091707):

  ```json
  { "participants": {
      "from": { "id": "8:orgid:…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
      "to": [ { "id": "8:orgid:<added>", "displayName": "…" } ] },
    "participantInvitationData": {},
    "groupContext": null,
    "groupChat": { "threadId": "19:…@unq.gbl.spaces", "messageId": null },
    "links": {
      "addParticipantSuccess": "{surl}callAgent/{session}/{cause}/conversation/addParticipantSuccess/",
      "addParticipantFailure": "{surl}callAgent/{session}/{cause}/conversation/addParticipantFailure/" } }
  ```

  Unthreaded (`yV`, @1093239): the same `participants` (where `from` takes no `alternateId` or
  `callingLineIdentity` unless set), `participantInvitationData: {}`, the same `links`, and
  `debugContent: {}`.
- **Callbacks:** already published by `conversation_callback_links` (`src/calling.rs:1221-1235`):
  `/conversation/addParticipantSuccess/` and `/conversation/addParticipantFailure/`.
  - The client resolves the add when the person APPEARS IN THE ROSTER, bounded by
    `addParticipantTimeoutSec:130` (@982574).
  - ONE add is in flight at a time, so a failure needs no participant id to be attributed.
    `InviteFailed` carries none (`src/calling.rs:618-625`).
  - `invite_failed` sets `unreachable` only in `Dialing`/`Connecting`. In `Connected` the failure is
    the pending add's: it is emitted as a one-frame `add_failed: {name, phrase}` and the call goes
    on.
- **Bounds:**
  - refused when the call would exceed `MAX_GROUP_CALL_PEOPLE` (20, `:10173`), counting us, the
    roster and the pending add;
  - refused while an escalation is in progress (§ 4.7);
  - an add that times out is reported as "may still be ringing": no cancel is offered, because
    none is measured.
- **The escalation that follows retargets OUR media too**, so M6 needs M5.
- **Privacy, unmeasured.** A third person added under a two-person `@unq.gbl.spaces` thread: what
  happens to thread membership, history visibility and call-event lines in that chat is not known.
  A 1:1 chat's membership is fixed in Teams, so the service probably creates a new group context or
  refuses. Add stays behind `teams-lite:call-exp:add` until M6's live step has checked that the
  third person gained no access to the 1:1 chat.
- **Status:** read-from-bundle. Whether the link is present on an outgoing 1:1 is inferred; the M0
  read-out measures it.

### 4.9 Blind / safe transfer (the user is the transferor)

- **Method:** POST.
- **Link:** the service's `transfer`. It is measured on the incoming acceptance's answer (§ 8a). On
  an outgoing call it arrives on `callAcceptance`
  (`this.links[I.LINKS.TRANSFER]=e.callAcceptance.links.transfer`, @1386651). That frame is
  merged into `call.links` today, but the `call_place` reply cannot show it. So M0 journals
  `call.links.names()` (names only) once the call connects.
- **The new accessor `Links::transfer()` refuses a URL that contains `/callAgent/`.** That is one of
  OUR callbacks, and posting a transfer to our own trouter would answer nothing.
- **Body** (`lG`, @1189010). `transferor.details` carries NO `displayName`. `endpointType`,
  `replacementDetails`, `disableForwardingAndUnanswered` and `transferContext` are `undefined` in
  the client and therefore omitted:

  ```json
  { "callTransfer": {
      "target": { "id": "8:orgid:<target>" },
      "transferor": {
        "details": { "id": "8:orgid:<self>", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
        "authorizationToken": null },
      "links": {
        "transferAcceptance": "{surl}callAgent/{session}/{cause}/call/transferAcceptance/",
        "transferCompletion": "{surl}callAgent/{session}/{cause}/call/transferCompletion/" } } }
  ```

  Blind and safe are the same transfer type (0) in the client:

  ```js
  // @1784566
  callBlindTransfer(t,i=Ie(),r){return this.executeCallTransfer(t,i,0,r)}callSafeTransfer(t,i=Ie(),r){return this.executeCallTransfer(t,i,0,r)}
  // @1262903 transferHelper
  this.timeoutManager.startTimer(I.TIMEOUT_OPERATIONS.OUTGOING_CALL_ESTABLISHMENT,()=>{this.handleCallTransferTimeout(u,E)},this.getTimeoutValue("outgoingCallEstablishmentTimeoutSec")),this.http.sendPostRequest({url:this.links[I.LINKS.TRANSFER],…})
  ```

- **Callbacks** (two NEW paths, @927412):
  - `/call/transferAcceptance/`: the target took it; the client sets transfer state 2;
  - `/call/transferCompletion/`: `{transferCompletion:{code,subCode,phrase}}`. Code 0 means done
    (state 3); anything else is a failure, and the call goes on:

    ```js
    // @1886142
    onTransferAccepted:n=>{…this._setTransferState(2)…},onTransferCompleted:(n,s)=>{…const o=n.transferCompletion,…if(o&&(…o.code===0)){…this._setTransferState(3)}
    ```

  - The bound is `outgoingCallEstablishmentTimeoutSec:90`.

  `calling::transfer_outcome_from_frame(url, body)` is keyed on OUR two callback paths. A
  `callTransfer` posted to our `/call/transfer/` (we are the TRANSFEREE, § 4.10) is never read as
  one.
- **Our own leg's ending:**
  - While a transfer is accepted or completed, a `callEnd` is mapped to `CallEndReasonTransferred`,
    whatever its phrase. Its code and phrase are journaled, and go into NATIVE-CALLING.md once seen.
  - If no `callEnd` arrives within 10 s of a code-0 completion, the backend POSTs `hangup_payload`
    to `Links::hangup()` and only then ends locally. Ending locally with nothing posted risks the
    phantom participant `hangup_payload`'s own comment describes.
  - A `callEnd` that arrives while the transfer is merely requested keeps the service's own phrase.
- **Status:** read-from-bundle; never posted on this tenant.

### 4.10 Somebody transfers US (read-only notice, M7)

- **What arrives:** a `callTransfer` on our `/call/transfer/` with `target`, `transferor` and
  `links {transferAcceptance, transferCompletion}`. The client stores both links and raises
  `onTransferRequested` (@1397545, @1884712), which then places a new call to the target.
- **What teams-lite does:**
  - it journals `[calling] {transferor} is transferring this call to {target}` (mris shape-only);
  - it emits a one-frame `transfer_offered: {from, to}`;
  - the page shows "{transferor} tried to transfer this call to {target}. teams-lite can't follow a
    transfer yet."
  - It posts nothing. `transferAcceptance` would place a call to a third party, and a refusal is
    something the transferor is shown.
- **Status:** read-from-bundle. What the transferor's client does when no acceptance comes is
  unmeasured.

### 4.11 Consultative transfer (deferred)

- **Link:** the ORIGINAL call's `transfer`.
- **Body:** `lG` with `replacementDetails: {replaces: <the consult call's links.replacement>}`.
  The client builds it as
  `getCallReplacementDetails=…({replaces:this.links[I.LINKS.REPLACE]})` (@1261075), with
  `this.links[I.LINKS.REPLACE]=e.callAcceptance.links.replacement`.
- **Status:** read-from-bundle; deferred, because it needs two calls at once.

### 4.12 Park / unpark and server hold (rejected; recorded so `hold` is never posted for Hold)

- **Link:** the acceptance's `hold`, which the client stores as `LINKS.PARK`
  (`this.links[I.LINKS.PARK]=e.callAcceptance.links.hold`, @1386894). Unpark goes to
  `holdResponse.links.resume`.
- **Bodies:**

  ```json
  { "hold": { "holdType": "callPark", "links": { "holdCompletion": "{surl}…/call/holdCompletion/" } } }
  { "resume": { "links": { "resumeCompletion": "{surl}…/call/resumeCompletion/" } } }
  ```

  `holdType` is `callPark`, `musicOnHold` (from `serverHoldV2`) or `sharedLineAppearance`
  (`getHoldTypeInRequest`, @1308303). The pickup is a call or transfer to `4:*14` / `4:*13`
  (`Lc={PARK:"*11",SHARED_LINE:"*12",SERVER_HOLD:"*13",CALL_PARK:"*14"}`, @784279). The bounds are
  `parkCompletionTimeoutSec:45` and `unparkCompletionTimeoutSec:45`.
- **Callbacks:** `PARK_COMPLETION:"/call/holdCompletion/"`, `UNPARK_COMPLETION:"/call/resumeCompletion/"`
  (@927412).
- **Status:** read-from-bundle; rejected.

### 4.13 Dial a phone number (deferred)

- **Link:** `conversationServiceUrl`, as `call_place`.
- **Body:** `participants.to: [{id: "4:+<E.164>"}]`. `callInvitation` adds `pstnContent
  {emergencyCallCountry, platformName, publicApiCall:false}` (@1298974), `emergencyContent`, and on
  a 1:1 PSTN call `locationContent`/`networkContent`/`areaContent`
  (`getLocationContent(){return this.isOneToOnePSTNCall()?Ug(this.locationInfoContent):null}`). The
  bound is `longerOutgoingCallEstablishmentTimeoutSec:185`. The conversation type is
  `"emergencyGroupCall"` for an emergency (`getConversationType`, @1305775).
- **Callbacks:** as a call, plus `/call/balanceUpdate/` (already published on the outgoing ack).
- **Status:** read-from-bundle; deferred.

## 5. Changes by file

### `src/calling.rs`

- **[M0]**
  - `session_of` (if incoming-and-conversation's M0 has not landed; its name and its rule).
  - `pub fn media_content_shape(frame) -> Vec<String>`: the KEY NAMES of every `mediaContent`
    object in a frame, plus `fromMixer`'s and `newOffer`'s boolean values. Never a value of
    anything else.
  - `media_sections` appends ` dir=<direction>` ONLY when the SDP states one (section or session
    level). Every line measured before stays byte-identical, and
    `an_answers_sections_are_stated_as_accepted_or_rejected` (`:2758`) passes unchanged.
- **[M0] A source-scan test**: every function in this file that calls `callbacks.link(` builds
  links containing `callAgent/{session_id}/`. The functions are found by the scan rather than
  listed by hand: `invitation_payload`, `join_payload`, `taking_the_call`,
  `acceptance_acknowledgement_payload`, `media_answer_payload`, `offering_media`,
  `start_outgoing_negotiation_payload`, `conversation_callback_links`, `media_controller_content`,
  `content_sharing_payload`, and anything added later.
- **[M1]**
  - `media_section_list` (if video-send's has not landed), `sdp_hold_state`,
    `signaling_media_types`.
  - Tests: section-level vs session-level direction; a rejected section ignored; inactive video
    alone is not a hold; `sendonly` audio is `None`; hold `[]`, resume `["audio"]`, recvonly
    sharing `["audio","ScreenViewer"]`.
- **[M3]**
  - `offer_door` / `offer_payload` (§ 4.0.2). Both existing builders stay, unchanged.
  - `media_negotiation_failure_from_frame(url, body)`, if media-robustness M6 has not landed.
  - A doc note on `Links` that the service's `hold` is the PARK door and has no accessor on
    purpose.
- **[M5]**
  - `MediaRenegotiation` gains `escalation`, `retarget` and `escalation_occurring`.
  - `retarget_completed_from_frame(frame) -> Option<(i64, String)>` over `/retargetCompleted` and
    `/_decoded/retargetCompleted`.
  - `taking_the_call(local, answer, callbacks, links: AcceptLinks)`. `AcceptLinks::Measured` gives
    today's four links (the default); `AcceptLinks::Client` gives the `qV` seven plus `end`. A test
    pins both, and pins that `Measured` is byte-identical to today.
- **[M6]**
  - `Links::add_participant(threaded: bool)`.
  - `add_participant_payload(local, to: &[(mri, name)], thread: Option<&str>, callbacks)`, building
    `vT` or `yV` with omitted keys.
  - A test pins both shapes and that no `null` stands in for an `undefined` key.
- **[M7]**
  - Paths `CALL_TRANSFER_ACCEPTANCE = "/call/transferAcceptance/"` and
    `CALL_TRANSFER_COMPLETION = "/call/transferCompletion/"`.
  - `Links::transfer()`, refusing our own callbacks.
  - `transfer_payload(local, target)` (`lG`, no `displayName` in `details`).
  - `transfer_outcome_from_frame(url, body) -> Option<TransferOutcome { Accepted, Completed { code,
    phrase } }>`, keyed on our two paths.
  - `transfer_request_from_frame(url, body)` for § 4.10, keyed on our `/call/transfer/` path.

### `src/bin/server.rs`

- **[M0] `CallSession::new(id, direction, kind, phase, conversation_id, peer, local, callbacks)`**.
  It replaces the literals at `:7436`, `:7594`, `:14975` and the test literals at `:17246`,
  `:17302` before any field is added. incoming-and-conversation adds many fields to the same
  struct, so whichever lands first introduces it.
- **[M0] Journaling, shape-only:**
  - `media_content_shape` on every renegotiation, answer and acceptance;
  - `call.links.names()` once a call reaches `Connected`;
  - the thread shape of each incoming invite;
  - for a `callEnd`, whether `CallEnded.call_id` equals the call's own id (`match`/`differs`/`empty`).
- **[M1]**
  - `CallSession` gains `held_by_them`.
  - Detection on the non-glare path of `:14811` (§ 4.3). The markers the glare scan splits on stay
    (`:17392-17420`: `if glare {`, `// No live call means`, `// Their SDP answer`).
  - `call_answer_media` (`:7962`): the held-one-to-one `[]` is a flag passed to video-receive's one
    answer-modalities function (`calling::declarable_modalities`, README § 3 collision 8):
    `held_one_to_one = call.controls_eligible() && sdp_hold_state(&sdp) == Some(true)`, and the
    function answers `[]` for it and the narrowed page list otherwise.
  - `CallSession::json` adds `held: {by_me, by_them, pending, unconfirmed}` and `escalated`, and no
    `one_to_one` (the page reads video-receive's `multi_party`, README § 3 collision 6).
    `can_send_media` adds `&& !held_by_me && !held_by_them`.
  - Tests: the detection sits below the glare `return;`; a meeting answer keeps the page's list;
    detection is skipped while we hold.
- **[M3] `call_hold {call_id, held: bool, sdp}`**, a new OUTWARD method. It refuses, each with a
  reason code the page maps to a sentence:
  - `no_call`, `not_connected`;
  - `not_one_to_one`;
  - `capture_on` (`sending` non-empty or `sharing.is_some()`);
  - `pending` (a hold or any offer of ours is out);
  - `escalating`;
  - `transferring`;
  - `no_link`;
  - `bad_sdp` (§ 4.1, § 4.2 validation).

  It records `hold_requested` and `outgoing_negotiation_at` BEFORE the POST and rolls both back if
  the POST fails. It posts through `offer_door`/`offer_payload`, arms the 35 s timer, and returns
  `answer_sdp` when the response carries one. `settle_hold` handles the answer (from the response
  or a frame), the refusal and the timer:
  - **hold confirmed:** `by_me = true`;
  - **resume confirmed:** `by_me = false`;
  - **hold unconfirmed:** `by_me = true`, `unconfirmed = "hold"`;
  - **resume unconfirmed:** `by_me = false`, `unconfirmed = "resume"`;
  - **refused:** the request cleared, and the page is told through `call_media {kind:"refused"}`.

  `json` adds `controls.hold: {offered, enabled, reason}`. `OUTWARD_METHODS` +1; extend the loop at
  `:17138`.
  - Tests: a source scan that `call_hold` contains no `"hold"` link literal and no `Links` method
    passes `"hold"` to `get`; that `callModalities` comes from `signaling_media_types` and never from
    `param_modalities`; that the pending assignment precedes `post_call_signal` in `call_hold` and in
    `call_offer_media`; that a hold carrying a sending video section is refused; and both
    unconfirmed directions.
- **[M4]**
  - `CallingPlane` gains `waiting: Option<CallSession>`.
  - `handle_incoming_call`'s busy branch (`:15022-15033`) keeps the invite as `waiting` under
    § 4.5's rules, with no POST.
  - `call_state_payload` (`:14422`) adds a top-level `waiting: {id, peer, peer_mri,
    conversation_id, started_at_ms, can_decline, can_answer}` and the one-frame `waiting_missed`.
    `can_answer` is true only while the live call is `Connected`.
  - `call_hangup` (`:8369`) gets a FIRST branch: an id equal to `waiting.id` posts the decline
    through `post_call_signal_as(waiting.id, …)`, clears `waiting`, emits, and RETURNS. It never
    reaches `end_call_locally` and never touches `plane.call`. Any `intent` other than `decline`
    on a waiting id is refused, and so is a `reason` (media-robustness M2's), since a waiting call is a
    ringing one. These rules are one validation table with plane-reliability's `intent` and
    media-robustness' `reason`: `call_hangup {call_id, intent, reason}` is designed once (README § 3,
    collision 4).
  - `end_call_locally` (`:15074`) promotes under § 4.6's conditions, rebuilding the callbacks.
  - A per-invite timer expires `waiting`.
  - The comments at `:2134-2139`, `:14678-14680` and `:15028-15030` are updated: a second call is
    no longer simply left for other devices.
  - Tests: a waiting decline leaves `plane.call`'s phase untouched and never reaches
    `end_call_locally` (scan plus behaviour); neither `CallingTurnedOff` nor a missing channel
    promotes; a promotion rebuilds the surl; a re-delivered invite and a second waiting invite are
    dropped; a ring arriving while the live call is `Ringing` is dropped.
- **[M5]**
  - `call.escalated` and `retarget_settled` are set from the offer and from `retargetCompleted`. A
    non-zero code sets `escalation_failed`, and the page hangs up through its existing failed-answer
    path, with `CallEndReasonEscalationFailed`.
  - `accept_links` chooses `AcceptLinks` in `call_accept` (`:7815-7958`).
- **[M6] `call_add_participant {call_id, mri}`**, a new OUTWARD method. It refuses:
  - not `8:`, self, or already in the roster;
  - an add already pending;
  - not connected; not one-to-one;
  - a meeting; escalating;
  - over `MAX_GROUP_CALL_PEOPLE`;
  - no link.

  The `adding` entry is bounded at 130 s. `invite_failed` (`:14697`) sets `unreachable` only in
  `Dialing`/`Connecting`; in `Connected` it emits `add_failed` against `adding`. `OUTWARD_METHODS`
  +1.
- **[M7] `call_transfer {call_id, target_mri}`**, a new OUTWARD method. It refuses: not one-to-one;
  not connected; `held_by_them`; escalating; not `8:`; target equal to the peer or self; no link; a
  transfer already pending. It also handles the outcomes, the 90 s bound, the `callEnd` mapping and
  the 10 s hangup fallback (§ 4.9), and the § 4.10 notice. `OUTWARD_METHODS` +1.

### `.claude/hooks/guard-live-automation.sh`

- Add `call_hold` (M3), `call_add_participant` (M6) and `call_transfer` (M7) to BOTH alternations at
  `:981`.
- Add one line each to the call comment block at `:924`:
  - `call_hold` tells the other party the call is held;
  - `call_add_participant` makes a person's devices ring;
  - `call_transfer` moves the user's caller to a third person.
- Pinned by `every_gated_method_is_named_in_the_automation_guard` (`server.rs:17954`). Run
  `python3 .claude/hooks/guard-live-automation.test.py`.

### `web/src/lib/call.ts`

- `ActiveCall` (`:38`) gains `held`, `escalated` and `controls {hold, add, transfer}`, each
  `{offered, enabled, reason}`. It gains no `one_to_one`: `multi_party` comes from video-receive
  (README § 3, collision 6), and whether a control is offered is the backend's `controls`.
- `CallStatus` (`:115`) gains `waiting: WaitingCall | null`; `UNKNOWN_CALL_STATUS` (`:160`) gets
  `waiting: null`.
- `callPhaseLabel` (`:258`) puts the hold states before "In a call".
- New pure `callHoldCaption(call)` and `waitingCallLabel(waiting)`.
- `callEndLabel` (`:341`):
  - `CallEndReasonTransferred` returns "Transferred {peer} to {name}." This is the ONE success
    sentence; no separate notice.
  - `CallEndReasonEscalationFailed` returns "The call couldn't become a group call, so it ended."
- Tests in `call.test.ts`.

### `web/src/lib/call-media.ts`

`CallMedia` (`:116`) gains:

- `setHeld(held): Promise<string | null>`:
  - live: remember each live transceiver's direction keyed by mid, set every one inactive (or
    restore them), and `offerLocalMedia()`;
  - simulated: an SDP with an `a=inactive` (or `a=sendrecv`) audio section.
- `rollBackHold(): Promise<void>`: roll back to `stable` and restore the directions to their
  `currentDirection`, KEEPING the local silence.
- `pendingOffer: "hold" | "resume" | "send" | "reconnect" | "takeback" | null`, the purpose
  media-robustness defines, extended.
- `readonly canSendTones` and `sendTones(tones): number` (the count accepted).

`liveCallMedia` keeps `muted` and `held`, and `track.enabled = !muted && !held`. `setMuted`
(`:1087`) honours `held`. `RemoteAudio` (`:434`) gains `setMuted(boolean)`, so a call held BY US is
silent here at once. Pure exports: `holdDirections()` and `normalizeTones()`
(`[0-9*#]`, capped at 32). `answerRemoteOffer` is unchanged for the one-PC path. M5 adds
`takeOverWith(offerSdp, iceServers)` for the second-connection retarget.

### `web/src/lib/ws-client.ts`

`callHold(callId, held, sdp)`, `callAddParticipant(callId, mri)` and
`callTransfer(callId, targetMri)`, all through `writeRequest` (pinned by
`every_gated_method_is_called_with_the_write_token`, `server.rs:18015`). The ceiling for
`callHold` is 40 s, over the 35 s bound.

### `web/src/lib/store.ts`

- `setCallHeld(held)`: silence the mic and the remote audio FIRST, then `media.setHeld` →
  `backend.callHold` → apply `answer_sdp`. On a refusal: `rollBackHold`, keep the silence, and show
  a notice. On 491/3118: one retry (§ 4.1).
- Both answer-failure paths route by `pendingOffer`:
  - the `onCallMedia` catch (`:2663-2685`);
  - `boundTheMediaAnswer` (`:2970-2981`).

  A hold or resume goes to `rollBackHold` with `holdRefusedMessage`. A capture keeps
  `abandonCallRenegotiation`.
- `sendCallTones(tones)`.
- `declineWaitingCall()`, which is `callHangup(waiting.id, {intent:"decline"})`.
- `answerWaitingCall()`: `hangUpCall()`, wait up to 15 s for `call.id === waiting.id && phase ===
  "ringing"`, then `answerCall()`.
- `addToCall(mri)` and `transferCall(mri)`. Each failure goes through `reportCall` with the
  `call-failure.ts` sentences.
- Hold and Keypad are offered only where incoming-and-conversation's `callMediaPlacement(…) ===
  "here"`. Until that lands, only where `this.callMedia !== null`.

### `web/src/lib/call-failure.ts`

New sentences: `holdNotToldMessage(peer)`, `resumeNotToldMessage(peer)`,
`holdRefusedMessage(reason)`, `addFailedMessage(name, phrase)`, `addMayStillRingMessage(name)`,
`transferFailedMessage(target, phrase)`, `waitingMissedMessage(name)` and
`transferOfferedMessage(from, to)`. Tests in `call-failure.test.ts`.

### `web/src/lib/call-stage.ts`

- `CallStagePanel` (`:31`) becomes `"people" | "chat" | "keypad" | "add" | "transfer"`.
- `callStageSubtitle` (`:165`) states the hold.
- New pure `callControlRows(call, placementHere, tonesAvailable, flags)` returns the More rows with
  `{enabled, reason}`. Rows for features this install does not offer are absent.
- If media-robustness's `stageControlLayout` has landed, these rows join its `overflow`. Tests in
  `call-stage.test.ts`.

### `web/src/components/call-stage.tsx`

- **ONE More control** (`data-testid="call-more"`, `MoreHorizontalIcon`):
  - It is the same More that media-robustness M1 puts on a phone. On a desktop it is added to the
    header row (`:330-394`).
  - Its content takes `className="z-[100]"`: the primitive is `z-50` (`ui/dropdown-menu.tsx:54`)
    and the stage is `z-[96]` (`:194`).
  - Rows: `call-hold` / `call-resume` (`CallPausedIcon`), `call-keypad` (`DialpadSquare01Icon`),
    `call-add` (`CallAddIcon`), `call-transfer` (`ArrowDataTransferHorizontalIcon`).
- A held banner in `StageContent`/`AvatarCard` (`:458`, `:531`), with an inline `call-resume-inline`
  pill.
- `MiniStatus` (`:708`) says "On hold".
- `SidePanel` (`:423`) hosts the keypad and the picker.
- `[data-testid=call-stage]` gains `data-held="me|them|both|none"`,
  `data-conversation-id={call.conversation_id}` and `data-direction={call.direction}`, which the
  drivers prove their presses with.

### New components

- **`web/src/components/call-keypad.tsx`.** A 4×3 grid of 44 px keys (`call-keypad-key-<k>`) and a
  sent-tones echo. Keys arrive only from the keypad's own focus, so digits typed into the stage's
  chat never send a tone. Disabled, with its reason, when `canSendTones` is false or the call is
  held.
- **`web/src/components/call-people-picker.tsx`** (Add and Transfer).
  - Candidates: the peers of the user's one-to-one conversations and the members this machine
    already holds (store only, no directory read). Excluded: self, the peer, anyone already in the
    call.
  - A search field and 44 px rows (`call-picker-row`). **A row SELECTS; it never acts.**
  - The act is one explicit button at the panel's foot (`call-picker-confirm`), which names the
    consequence:
    - "Add {name}";
    - "Transfer {peer} to {name}", drawn destructive. For Transfer, the button is ARMED by the
      selection and disarmed by Escape, Cancel or another selection.

### `web/src/components/call-bar.tsx`

- A `WaitingCard`, rendered while `callStatus.waiting` is set, in its own stack at `z-[97]` (above
  the stage's `z-[96]`). It carries distinct test ids: `call-waiting`, `call-waiting-decline`,
  `call-waiting-answer`. It never reuses `call-hangup` or `call-answer`.
- `incoming-call-banner.tsx` (`:39-43`) also filters `callStatus.waiting.conversation_id`, so one
  call never draws two cards.

### `web/mock/server.ts`

Mirror every refusal:

- `call_hold` with the backend's reason codes. It broadcasts an inactive or sendrecv answer after
  300 ms.
- Hooks `{kind:"call_media", hold:true|false}` (held by them: broadcasts an inactive or sendrecv
  offer) and `{kind:"call_media", reject_hold:"glare"|"refuse"}` (a `refused` event).
- `injectMockCallInvite` (`:4966`) stores `waiting` under § 4.5's rules. `call_hangup` (`:10549`)
  declines a waiting id. `endMockCall` (`:4889`) promotes.
- `{kind:"call_waiting", expire:true}`.
- `{kind:"call_escalate", fail?:true}`.
- `call_add_participant` and `call_transfer`, with `{kind:"call_add", fail:"unreachable"|"timeout"}`
  and `{kind:"call_transfer", fail:true, incoming?:true}` (the last emits the § 4.10 frame).
- `call_invite {reset:true}` clears all of it.

### `web/scripts/join-live.ts` and `web/scripts/call-live.ts`

- **[M0] `hangUp(page, expect)` proves its target.**
  - It clicks only `[data-testid=call-stage][data-call-id=<the call this driver started>]
    [data-direction=outgoing] [data-testid=call-hangup]`. The stage must also carry the pinned
    `data-conversation-id`, or, for a join, the pinned meeting thread.
  - It never clicks a ringing card or a waiting card, and logs and skips otherwise.
  - The call id is read from the stage once the driver's own placed call first appears in
    `dialing`, before anything else can ring.
  - A test scans both files for a `call-hangup` locator that is not scoped this way.
- **[M0] `call-live` read-outs:**
  - a `data-held` timeline;
  - the backend's `media_content_shape` and section `dir=` lines, from the journal (the page never
    sees them);
  - per-frame LINK NAMES in `readCallSignals` (keys only, never a URL, because a URL carries the
    surl);
  - `__tlPc`'s audio sender `dtmf.canInsertDTMF`, and which `telephone-event` rtpmap the answer
    kept.
- **[M1–M3] New `call-live` flags,** named so they cannot be confused with `--hold N` (the
  duration, `:367`): `--press-hold-at <s>`, `--press-resume-at <s>`, `--press-keys <digits>`.
  - Each press opens `call-more` and re-proves the stage's `data-conversation-id ===
    AUTHORIZED_CALL_CONVERSATION` and `data-call-id` immediately before it.
  - The driver sets `teams-lite:call-exp:hold` only in its own fresh context (`addInitScript`).

### `web/scripts/preview.ts`

`--call-control` captures:

- the More menu in both themes;
- held by us and held by them, on the stage and in the mini window;
- the "wasn't told" state;
- the keypad at desktop and phone width;
- the waiting card over a live stage, in both themes;
- the add and transfer pickers, the armed transfer button, and every failure notice.

Every capture goes through `withPreview` and its MOCK sentinel.

## 6. Gating and consent

| RPC | List | Why |
| --- | --- | --- |
| `call_hold` | `OUTWARD_METHODS` | It tells the other party the call is held (their client shows it), and it carries an SDP. |
| `call_add_participant` | `OUTWARD_METHODS` | A person's devices ring, which cannot be taken back. |
| `call_transfer` | `OUTWARD_METHODS` | A third person's devices ring, and the user's caller is moved to them. |

All three:

- need the write token and are refused read-only at the dispatch gate;
- are named in BOTH alternations of the guard;
- go through `writeRequest`;
- are mirrored, refusal for refusal, by the mock.

**No new RPC, deliberately:**

- remote-hold detection and the waiting ring's display (reads of frames);
- the waiting Decline (the existing OUTWARD `call_hangup`, now matching a waiting id, behind its
  own branch);
- End-and-answer (the existing `call_hangup`, `call_prepare` [MACHINE] and `call_accept`
  [OUTWARD], each with the same gate as today);
- the transfer notice of § 4.10 (a read);
- **DTMF.** The tones ride the call's own audio exactly like the user's voice. The backend never
  sees them, and they leave only on a key press, on the page that reached its media through the
  gated `call_place`/`call_accept`, while the keypad holds focus.

**Nothing acts on a timer:**

- an expiring waiting ring posts nothing;
- an unconfirmed hold posts nothing;
- an incoming transfer is never accepted or refused;
- the only timer-driven POST is the hangup that follows a transfer the user made (§ 4.9), which
  completes the act they asked for.

**Every outward act is one press whose words state the consequence before it.**

- **Hold** is reversible (Resume) and is one press, like Mute.
- **Add** is one explicit button after a selection, which is the "a call is one click and the
  words carry the difference" rule (CLAUDE.md § Audio calls). The row tap never acts.
- **Transfer** cannot be undone, moves the user's caller and drops the user's own leg, so its
  button is ARMED by the selection and states both people.

**Privacy rule for hold.** The microphone AND the speaker are silenced on this page at the press,
before any signaling, and stay silenced until Resume, whatever the service answers. A hold the
service never confirms says so ("{peer} wasn't told, so they hear silence"); it never reverts to a
live microphone.

**The `hold` link is never posted.** A Rust test pins that.

**The verified audio path:**

- the incoming accept bodies change only behind `accept_links`;
- the declared modalities change only for an all-inactive answer on a one-to-one, which no
  verified call produces;
- `calling.spec.ts` stays green unchanged.

## 7. UI and copy (including mobile)

All copy is final English.

**The More menu** (`call-more`, "More call actions"). Its rows are 44 px through the shared
`DropdownMenuItem`, and its content sits at `z-[100]`.

| Row | Disabled reasons (shown in words) |
| --- | --- |
| "Put the call on hold" / "Resume the call" | "Hold works in one-to-one calls." · "Turn the camera and screen sharing off first." · "Hold from the window that has the call's audio." · "Putting the call on hold…" (pending) · "The call is becoming a group call." |
| "Keypad" | "This call doesn't carry keypad tones." · "The keypad works in one-to-one calls." · "Resume the call to use the keypad." |
| "Add someone" | "Adding people works in calls you placed." · "This call already has {MAX} people." |
| "Transfer the call" | "Transfer works in one-to-one calls." · "{peer} has the call on hold." |

A row whose feature this install does not offer is not drawn. A row that cannot work right now is
drawn disabled, with its reason under it (on a phone a tooltip is a sentence that does not exist).

**Held by us:**

- subtitle "On hold";
- over the person, a pause mark, the line "{peer} can't hear you, and you can't hear them.", and a
  primary "Resume" pill;
- while signaling: "Putting the call on hold…" / "Resuming…";
- unconfirmed after 35 s: the caption "On hold here only — {peer} wasn't told, so they hear
  silence." and the same sentence as a notice;
- a resume that is unconfirmed: the caption "Resumed here — {peer} may still hear silence.";
- the Resume pill is disabled while a hold is pending, with the reason "Putting the call on hold…".

**Held by them:**

- subtitle "On hold";
- caption "{peer} put the call on hold." (fallback "The call is on hold.");
- camera, share and keypad are disabled; Mute keeps working.

In both cases the mini window's status says "On hold".

**Keypad panel** (beside the stage; replaces it below `md`, like the other panels):

- heading "Keypad";
- hint "Tones go into the call, for menus that ask you to press a number.";
- a 1–9 * 0 # grid with letters under the digits;
- a visual echo "Sent 1 2 3 #". The echo's live region announces "4 tones sent", never the digits,
  because these are often a PIN.

**Waiting card** (bottom stack, `z-[97]`, full width on a phone):

- the caller's face and name, "Is calling you";
- "Decline", and "End call and answer" (drawn only while the current call is connected);
- on expiry, the notice "You missed a call from {name} while you were on another call.";
- when the current call ends on its own, the waiting call becomes the ordinary ringing card for
  its remaining time.

**People picker:**

- heading "Add someone to the call" or "Transfer {peer} to…";
- search field "Search people you chat with";
- rows select;
- foot button "Add {name}" with the hint "Their devices ring, and the call becomes a group call.";
- or "Transfer {peer} to {name}" with the hint "{peer} is put through to {name}, and you leave the
  call.";
- pending: "Calling {name}…" / "Transferring to {name}…";
- notices:
  - "{name} couldn't be added: {reason}."
  - "{name} wasn't added in time. Their devices may still be ringing."
  - "The transfer didn't go through: {reason}. You're still in the call."

**Escalation:**

- subtitle "Now a group call", with the People panel listing whoever the roster names;
- on failure: "The call couldn't become a group call, so it ended.";
- the § 4.10 notice: "{transferor} tried to transfer this call to {target}. teams-lite can't follow
  a transfer yet."

**Incoming PSTN caller:** a `4:+E.164` caller the store cannot name is drawn as the formatted
number.

**Mobile.** The only new header control is More, and only where media-robustness's phone layout has
not already put More in the bottom bar. Panels replace the stage below `md`. Every new target is at
least 44 px; the existing 36 px stage controls are media-robustness M1's.

## 8. Tests, mock and captures

**Rust (`cargo test`)**

- `calling::tests`:
  - `session_of`;
  - every link that a `callbacks.link(` builder publishes carries its session id (scan-driven);
  - `dir=` printed only when stated;
  - `sdp_hold_state` and `signaling_media_types` (§ 5);
  - escalation, retarget and `escalation_occurring` parsed;
  - `retarget_completed_from_frame`;
  - `add_participant_payload` in `vT`/`yV` with omitted keys;
  - `transfer_payload` (no `displayName`, no envelope);
  - `transfer_outcome_from_frame` and `transfer_request_from_frame` keyed on our paths (a
    transferee `callTransfer` is never read as an outcome);
  - `Links::transfer()` refuses our own callback;
  - `AcceptLinks::Measured` byte-identical to today.
- `server` tests:
  - write class, read-only and token refusals for the three methods (extend `:17138`);
  - the guard-name test;
  - the scans listed in § 5 (no `"hold"` link; modalities from the SDP; pending before POST in both
    handlers; detection after the glare return; the waiting decline never reaches
    `end_call_locally`);
  - behaviour: a meeting answer keeps its list; promotion conditions; the dedupe rules; a
    `Connected`-phase `addParticipantFailure` never sets `unreachable`; the `callEnd` mapping during
    a transfer.
- `python3 .claude/hooks/guard-live-automation.test.py` with the three names.

**Web unit (`bun run test` and `bun run typecheck`)**

- `call.test.ts`: hold labels, captions, the waiting label, the two new end reasons.
- `call-media.test.ts`: `holdDirections` round trip; `normalizeTones` strips and caps.
- `call-stage.test.ts`: `callControlRows`, every reason, flags and placement.
- `call-failure.test.ts`: the new sentences.

**Local WebRTC E2E** (no tenant, `about:blank`, the `webrtc-glare.spec.ts` pattern)

- `webrtc-hold.spec.ts`:
  - A↔B connected;
  - A's hold path (directions → inactive, the offer through `toMsSdp`/`fromMsSdp`) stops B's
    inbound-rtp packets from growing, and resume restores them;
  - B holding makes A's answer `a=inactive`;
  - `rollBackHold` returns A to `stable` with its track still disabled.
- `webrtc-dtmf.spec.ts`:
  - `canInsertDTMF` is true over the munged SDP;
  - with the answer keeping only `telephone-event/8000` beside `opus/48000`, it records whether
    `canInsertDTMF` stays true;
  - `ontonechange` fires once per tone with 200/100 ms timing.
- `webrtc-retarget.spec.ts`: measures the second-connection takeover (a new `RTCCertificate`, so a
  new fingerprint and ufrag) end to end. It also records, informationally, whether a single
  connection follows.

**Mock E2E** (`web/e2e/call-control.spec.ts`; every test resets with `call_invite {reset:true}`)

- Remote hold flips the caption and `data-held`; resume clears them.
- Local hold (with the flag) silences, shows Resume, and the glare retry and the refusal reach the
  "wasn't told" state.
- An unconfirmed resume shows its caption.
- The keypad sends and echoes, and never fires from the chat composer.
- A second invite during a call shows the waiting card; one during a ringing call does not.
- Decline clears the card and leaves the live call connected. End-and-answer ends the first call
  and answers the second; the spec asserts the RPC sequence, not only the screen.
- Waiting expiry shows the missed notice. The awareness card does not duplicate the waiting card.
- Escalation and its failure.
- Add: success, unreachable, timeout.
- Transfer: the armed button, success, failure, and the § 4.10 notice.
- Menu, waiting card and picker are hit-testable over a live stage (`elementFromPoint`), not merely
  present.

`calling.spec.ts` must stay green unchanged.

**Driver test:** a scan that every `call-hangup` locator in `join-live.ts` and `call-live.ts` is
scoped to the proven stage.

**Preview:** `cd web && bun run preview -- --out /tmp/cc --call-control`, in both themes and at a
phone's width, with `--dpr 3` for the keypad and the caption crops.

## 9. Live verification plan

Every call step involves the pinned colleague (Clément BOSLE, `AUTHORIZED_CALL_CONVERSATION`), who
must agree to THAT step beforehand. Any third colleague must agree beforehand too.

| # | Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- | --- |
| 1 | M0 regression and read-outs | `cd web && bun run call-live -- --hold 60`, plus `bun run join-live -- --from-chat --hold 60` and `bun run join-live -- --pair --share` (both installs up: the user's start) | the driver; the colleague answers | Audio verdicts are unchanged. The journal shows the `media_content_shape` lines (`mediaLegId`/`fromMixer` present or absent), the link names at connect (`transfer`, `addParticipantAndModality`, `addParticipant`), `canInsertDTMF`, the kept `telephone-event` rtpmap, and zero frames of another session touching the live call. |
| 2 | M0 incoming provenance | the user's own click: the colleague calls the user, the user answers in their own app | the user, the colleague | The audio works as on 2026-09-04. The journal shows the invite's thread shape and zero cross-session frames touching the live call. With step 1, this is the proof § 4.0.1 requires before M4's promotion is on. |
| 3 | M1 remote hold | `bun run call-live -- --hold 90` | the colleague presses Hold at about 20 s and Resume at about 50 s, in his own Teams | Either `the other side put the call on hold` then `… resumed it` in the journal, with `dir=inactive` then `dir=sendrecv`, `data-held` `them` then `none`, and inbound audio bytes growing again; OR no renegotiation at all. That records outcome 2 or 3 of § 4.3. Both are measurements. |
| 4 | M2 keypad | `bun run call-live -- --hold 40 --press-keys 123#` | the driver presses; the colleague, who agreed to hear four tones, stays on | `canInsertDTMF` true, `ontonechange` ×4, the echo, and audio unaffected. A real menu acting on the tones is the user's own click on a menu they choose. |
| 5 | M3 local hold | `bun run call-live -- --hold 60 --press-hold-at 15 --press-resume-at 35` | the driver presses, re-proving the stage's conversation and call ids before each press; the colleague reports what his Teams showed and heard | Within 35 s of each press, `a media answer arrived … dir=inactive` (then `sendrecv`) or `the offer was answered at once`. `outbound-rtp` packets stop and resume. The colleague confirms "on hold", then audio back both ways. **If nothing arrives**, the "wasn't told" state appears, the microphone stays silent, Resume restores audio, the answer to § 12 Q1 is recorded, and the flag stays off. **A glare refusal** is journaled with its code and the retry's outcome. |
| 6 | M3 incoming hold | the user's own click: the colleague calls, the user answers, holds and resumes, with `teams-lite:call-exp:hold-incoming` on in their browser | the user, the colleague | The same signals as step 5. Only then does incoming hold become a default. |
| 7 | M4 call waiting | the user's own windows only, never `call-live` | the user, the colleague, and a second colleague who agreed | The waiting card appears within a second. Decline removes it, the journal shows the reject POST, and the first call is untouched. On a repeat, End call and answer ends the first call, the second connects with audio both ways, and no frame of the first call touches the second. |
| 8 | M5 inbound escalation | `bun run call-live -- --hold 150` | the colleague adds a third colleague who agreed | `escalation=true`/`retarget=true` on the offer; the second connection takes over (ICE connected and inbound audio on it); `retargetCompleted` code 0 if delivered; "Now a group call". On a failure: the stated sentence and a clean hangup, never a silent call. |
| 9 | M5 `accept_links: client` | the user's own start: `TEAMS_LITE_CALL_ACCEPT_LINKS=client bin/teams-dev-server.sh` with `bun run dev` (19421/19441), or the `CallProtocol` setting if it has landed; the colleague calls, and the user answers ONLY in the 19441 window | the user, the colleague | It connects with audio both ways, exactly like 2026-09-04. Only then does `client` become the default. |
| 10 | M6 add someone | the user's own click, flag on | the user, the colleague, a third colleague who agreed | The third person rings, joins and appears in People; audio continues for all three. The third person checks their chat list: they gained no access to the user's 1:1 chat with the colleague. The journal names the door used and the conversation the call line lands in. |
| 11 | M7 blind transfer | the user's own click, flag on | the user, the colleague, a third colleague who agreed | The colleague is put through to the third person. The user's leg ends with "Transferred … to …". The journal names `transferAcceptance`, `transferCompletion` and the `callEnd` code/phrase, which go into NATIVE-CALLING.md. Separately, the colleague transfers the user to the third person: the § 4.10 notice appears and nothing is posted. |

## 10. Milestones

Each ships alone. S = under a day, M = a few days, L = a week or more.

| # | Milestone | Scope | Effort | Risk | Depends on |
| --- | --- | --- | --- | --- | --- |
| M0 | Groundwork: routing, read-outs, drivers that prove their hangup | `CallSession::new`; session routing (or incoming-and-conversation's M0); shape-only journaling (`media_content_shape`, `dir=`, link names at connect, invite thread shape, `callEnd` id match); `data-conversation-id`/`data-direction` on the stage; `hangUp` proving its target; `call-live` read-outs; steps 1–2. | S–M | Low: nothing changes on the wire. | incoming-and-conversation (M0 routing; soft, landed here under its name otherwise) |
| M1 | Show when the other side holds | `media_section_list`/`sdp_hold_state`/`signaling_media_types`; `held_by_them`; the `[]` answer on a one-to-one only; captions; mock and E2E; preview; step 3. | S | Low: read-only, and the one answer change is scoped to an all-inactive 1:1 answer. | video-send (`media_section_list`, `modalities-parity`; soft); video-receive M1 (owns the one `is_one_to_one()` predicate and the one answer-modalities function this milestone's `[]` is a flag of; README § 3 collisions 6 and 8) |
| M2 | Keypad | `canSendTones`/`sendTones`; the keypad panel; one-to-one only; `[0-9*#]`; `webrtc-dtmf.spec.ts`; `--press-keys`; `4:` callers named by number; step 4. | S | Low: no signaling. | none |
| M3 | Put a 1:1 on hold and resume it | `call_hold`; the shared door helper with the pending-before-POST fix; `settle_hold`; the per-request timer; the refusal purpose and glare retry; `setHeld`/`rollBackHold`; the Resume pill; page flags; `webrtc-hold.spec.ts`; steps 5–6, then outgoing default on, then incoming default on. | M | Medium: the first directions-only renegotiation on this tenant. Mitigated by local silence first, the 35 s bound, a rollback that keeps privacy, the flags, and refusing it while a capture is on. | media-robustness-devices (M6 refusal reader + offer purpose; M0 `call-experiments.ts`; soft); plane-reliability (M4 `Answer` bound; soft); incoming-and-conversation (M8 `callMediaPlacement`; soft); video-send (mediaContent parity; told about the order fix) |
| M4 | Call waiting: Decline, or End and answer | `CallingPlane.waiting`; the § 4.5 rules; the waiting decline branch with its own correlation; promotion with rebuilt callbacks; the per-invite expiry; `WaitingCard`; awareness-card filter; step 7. Promotion on only after § 4.0.1's proof (steps 1–2). | M | Medium: a promoted call must not be ended by the old call's frames (closed by routing), and End-and-answer can meet a caller who has gone (stated). | incoming-and-conversation (M0 routing, hard; `decline_payload`, soft); plane-reliability (M3 `intent`, M4 `HANGUP_WAIT`, hard for End-and-answer); ringing-alerts-history (whether the waiting card chimes; missed-call surface) |
| M5 | Survive escalation to a group | second-connection takeover; `retargetCompleted`; `escalated`; refusals while escalating; `accept_links` behind its switch; mock; steps 8–9. | L | High: a wrong answer drops our leg mid-conversation. The incoming body change touches a verified path (switch, then the user's own proof). | media-robustness-devices (the reconnect machinery and its second-session lessons, soft); incoming-and-conversation (`CallProtocol`; not in the same release as its attach change) |
| M6 | Add someone to a call you placed | `call_add_participant`; picker; one add in flight; the 130 s bound; phase-scoped `unreachable`; `MAX_GROUP_CALL_PEOPLE`; flag; step 10. | M | Medium: it rings a person irreversibly, and the privacy of the 1:1 thread is unmeasured (flag until measured). | M5 (hard); meeting-awareness (roster fidelity for "appears in the roster"; soft); incoming-and-conversation M4a (for incoming calls; deferred) |
| M7 | Blind transfer, and the transferee notice | `call_transfer`; armed button; the two callbacks; `CallEndReasonTransferred`; the 90 s bound; the `callEnd` mapping and the hangup fallback; the § 4.10 notice; step 11. | M | Medium: it moves a caller to a third person irreversibly, and the transferor-side end frame is unmeasured. | plane-reliability (M4 bounds; soft); incoming-and-conversation (frame readers per callback path; soft) |

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Hold implemented by POSTing to `hold` would PARK the call or put it on server hold, and the call would leave this endpoint. | Hold is a renegotiation. No `Links` accessor returns `hold`, and a scan pins `call_hold`. NATIVE-CALLING.md records that `/call/holdCompletion/` and `/call/resumeCompletion/` are park callbacks. |
| The service never answers a directions-only offer, leaving our offer pending and the far side unsure. | Silence first. The 35 s bound leads to `rollBackHold` with the silence kept and the "wasn't told" sentence. Resume always re-offers `sendrecv`. The flag stays off until step 5 says otherwise. |
| A hold press meets glare (a 1:1 renegotiates on its own, § 8b). | The pending state is recorded before the POST. The refusal reader clears it. One retry after the service's own offer is answered. Both glare directions are journaled with their codes. |
| An answer frame lands during the POST and a stale glare window refuses the service for 35 s. | Pending state before the POST, cleared by `settle_hold` from either path. A scan pins the order in `call_hold` and in `call_offer_media`. |
| The declared-modalities change regresses the receive path. | It applies only to an all-inactive answer on a one-to-one. A test pins a meeting answer and every other answer to today's list. `calling.spec.ts` stays unchanged. |
| `kind == Call` is true for incoming GROUP calls, so hold/transfer would be offered there. | `controls_eligible()` over video-receive's one `is_one_to_one()` predicate (§ 4.0.6), used by every control, the mock and `callControlRows`, with a Rust test that an incoming group invite is refused. |
| `call_hold` carries a camera section the backend never records. | The SDP is validated per direction (§ 4.1, § 4.2) and refused otherwise, in the backend and the mock. |
| Declining the waiting call ends the active one (the shared `call_hangup` tail runs `end_call_locally`). | A first, returning branch for a waiting id, with its own correlation id, and a test that it never reaches `end_call_locally`. |
| A promoted waiting call is answered with callbacks on a dead surl (a reconnect or calling turned off promotes inside `end_call_locally`). | Promote only with a connected channel. Rebuild the callbacks from it. Drop the waiting ring on `CallingTurnedOff`. Tests for both. |
| The ended call's late frames end the promoted call. | Routing by session (§ 4.0.1), with promotion switched on only after the three-provenance proof. |
| Phantom rings on the user's other installs, and re-delivered invites, turn into waiting cards and a REJECT posted to a call the user is in elsewhere. | Waiting only while the live call is `Connecting`/`Connected`; dedupe by call id; End-and-answer only while `Connected`. |
| A driver's `finally` clicks Decline on a promoted call, or hangs up a call the user answered. | M0's `hangUp` clicks only the proven stage of the call the driver started, never a ringing or waiting card. The M4 live step uses the user's own windows only. |
| End-and-answer stalls on a hung hangup POST and the user loses both calls. | It depends on plane-reliability's `HANGUP_WAIT`. The page waits 15 s for the promotion and then says what happened. |
| A Teams client's Hold reaches us as a local mute-on-hold, or through a mixer, and detection sees nothing. | M0 journals `fromMixer`. Step 3 records which outcome happened. The UI never implies "not held" from silence. |
| An escalation retargets to a new DTLS fingerprint and our single connection cannot follow, so our leg dies silently. | A second connection takes over, which is the client's own shape. A failure is stated and followed by a clean hangup. Controls are refused while escalating. |
| The incoming accept links (`qV`) change the verified incoming flow. | The `accept_links` switch, default `measured`, byte-identical by test, and not in the same release as incoming-and-conversation's attach change. |
| Add or transfer rings a third person by mistake. | OUTWARD methods named in the guard. Rows select and an explicit button acts. Transfer is armed and names both people. Candidates are people the user already chats with. Flags until each live step. |
| An unreachable added person relabels the call's whole ending. | `unreachable` only in `Dialing`/`Connecting`; one add in flight; `add_failed` reported against it. |
| Keypad digits leak: typed in the chat, announced aloud, or sent to a meeting. | Focus-scoped keys; the live region announces counts only; one-to-one only. |
| The new menu, card and panels render under the stage (`z-50` / `z-[95]` < `z-[96]`). | Menu content at `z-[100]`, waiting stack at `z-[97]`, with hit-tests in the E2E. |
| Two pages share one call, and the page without the media presses Hold or Keypad. | Offered only where `callMediaPlacement === "here"` (or where this page holds `callMedia` until that lands). |

## 12. Open questions and decisions the user must make

**Open questions** (each has the step that answers it)

1. Does the service answer a directions-only renegotiation on a 1:1, and how does the far Teams
   client react? An offer that adds a section was answered on 2026-08-06; this kind never has.
   (Step 5.)
2. When a Teams client presses Hold, what reaches us: an all-inactive renegotiation, nothing (local
   mute-on-hold), or nothing because the call goes through a mixer? Does the held party hear music?
   The UI caller of `hold()` is not in the fetched chunks. (Step 3.)
3. Does the service's answer keep `telephone-event`, at which clock rate, and does anything on a
   Teams call act on RFC 4733 events from a WebRTC endpoint? How is an auto attendant addressed from
   Teams? (Step 4; the bundle does not say.)
4. Is a teams-lite 1:1 peer to peer or through a media processor (`fromMixer`)? (Step 1.)
5. Does an incoming 1:1 invite name a thread, and in which shape? Until it does, incoming calls with
   no thread are not "one-to-one" for these controls. (Step 2.)
6. Does an outgoing 1:1 carry `transfer`, `addParticipantAndModality` and `addParticipant` once
   connected? (Step 1.)
7. Does the service echo a `mediaLegId` this app never sends, and does sending one change whether a
   renegotiation is answered? (Steps 1 and 5; parity belongs to video-send.)
8. Does declining a never-attached waiting invite stop the user's other devices ringing, and is the
   caller sent to voicemail? (Step 7; shared with incoming-and-conversation's
   `decline-uses-client-shape`.)
9. After a blind transfer, which `callEnd` does the transferor get, and how long after
   `transferCompletion`? What does the transferor's client do when the transferee never accepts?
   (Step 11.)
10. Adding a third person under a 1:1 thread: does the service create a new group context, and does
    the third person gain anything in the 1:1 chat? (Step 10.)
11. Does the account hold a Teams Phone (PSTN) licence, and does the tenant have a Call Park
    policy? No read-only probe is known.

**Decisions for the user**

- **D1. Ship hold?** Ship hold (M3) as the first live outgoing directions-only renegotiation, by
  appointment with the pinned colleague, knowing it may meet glare and may go unanswered? Proposed:
  yes, behind the flag, steps 5–6 in that order.
- **D2. How to turn on experiments.** How should the user turn on `hold-incoming`, `add` and
  `transfer` for their own click? Proposed: an "Experimental call controls" group in Settings ›
  Calls (media-robustness M4 creates that pane), off by default and stored per browser. The
  alternative is setting the `teams-lite:call-exp:*` key by hand.
- **D3. Waiting card alerts.** Should the waiting card chime or vibrate, given the user is already
  talking? Proposed: one short cue, no repeating ring (ringing-alerts-history decides the sound).
- **D4. Second confirmation for Add.** Should Add also be armed, like Transfer? Proposed: no. A
  placed call is one click with words, and Add rings one person.
- **D5. Following a transfer.** Should following somebody else's transfer (§ 4.10) ever become a
  feature? It means placing a call to a third person on a press of the user's, while the first call
  is still up. Proposed: only after hold-and-answer exists.

## 13. Review notes

**My own re-check, which neither review raised:**

- **The "never answered" premise was too strong, and the risk and value statements changed with
  it.** NATIVE-CALLING.md § 10.8 records outgoing renegotiations ANSWERED on 2026-08-06: in a
  meeting on a frame (audio accepted, video rejected), and on a real 1:1 (the new section zeroed;
  the answer's place inferred from timing). What has never been sent is a directions-only offer.
  The summary, § 4.0.3, M3's risk and Q1 now say exactly that.
- **The client sets `RemoteHold` from a negotiation only on a non-mixer 1:1**
  (`if(o&&!this.callUsesMixer)`, @1946847). This is the third reason remote hold may be invisible,
  so M0 journals `fromMixer`.
- **Several sibling designs now own pieces this one had invented copies of.** It consumes them by
  name instead:
  - routing: incoming-and-conversation § 4.0, `session_of`, instead of a separate
    observe/enforce switch;
  - the refusal reader and offer purpose: media-robustness § 4.1;
  - the page experiment flags: `call-experiments.ts`, instead of a new localStorage key;
  - the stage's More control: media-robustness M1;
  - which page holds the media: `callMediaPlacement`, instead of `callMediaHere`;
  - bounded hangups and `intent`: plane-reliability § 4.6 / § 4.9;
  - `media_section_list` and the modality derivation: video-send.
- **Following a transfer and "take the call here" were deferred TO this workstream** by
  incoming-and-conversation, while the draft deferred the first back to it. Nothing may be
  orphaned, so § 3 now owns both decisions and M7 builds the read-only notice.
- **More detail checked against the bundle:**
  - `participantInvitationData` is `{}`, not `null` (`XP`);
  - `transferor.details` carries no `displayName` (`lG`);
  - `yV`'s `debugContent` is `{}`;
  - the waiting decline needs its own correlation id (`post_call_signal` reads `plane.call`).

**Accepted from the code and protocol review (all verified):**

- Stale surl on promotion (`:14491-14500`, `:14576-14578`): rebuilt callbacks, and no promotion
  without a connected channel.
- The pending state set after the POST (`:8262-8275`): set before it, rolled back on failure, with
  one settle helper; the same fix goes into `call_offer_media`.
- `/call/rejection/` is unread and glare does apply to hold: the refusal reader, one retry, and the
  "no glare" claim corrected.
- `mediaLegId` and the other `mediaContent` fields: stated as a divergence, journaled, and parity
  left to video-send.
- An unconfirmed RESUME and the Resume pill while a hold is pending: both defined, with reasons.
- The client never answers a retarget on its existing connection (`processOfferAsync` →
  `acceptReconnectStartAsync` → `initReconnect`): the second connection is the plan,
  `escalationOccurring` is parsed, controls are refused while escalating, and M5 is L.
- Enforce proven by an outgoing run alone: the proof is now three provenances.
- `InviteFailed` carries no mri: phase scoping plus one add in flight.
- Duplicate invites, and a waiting call during a Ringing call: dedupe, and waiting only while
  `Connecting`/`Connected`.
- Explicit `null`s for `undefined` keys: omitted.
- The stage had no `data-conversation-id`: added, together with `data-direction`.
- The `onCallMedia` failure path: both failure paths route by `pendingOffer`.
- Remote-hold detection while we hold: skipped.
- The missed test literals (`:17246`, `:17302`) and `:7594`: corrected.
- The 20 s ticker: per-request timers.
- The fallback door's envelope: `offer_payload` returns what the door requires.
- The `callEnd`/`transferCompletion` order and a fallback leaving a phantom leg: the mapping and a
  hangup POST before the local end.
- The threaded add's privacy: an open question, a flag, and a check in the live step.
- Two cards for one call: the awareness filter.
- DTMF: one-to-one only, `[0-9*#]`, the clock-rate read-out, the colleague's consent for the
  keypad run, and the live region announcing counts.

**Accepted from the project-rules and regressions review (all verified):**

- **The blocker:** the drivers' unproven `hangUp` (`join-live.ts:1253`, the Decline testid at
  `call-bar.tsx:152`). It is fixed in M0, the waiting card has its own test ids, and the M4 step
  uses the user's own windows.
- Hold default-on per direction.
- The `is_one_to_one()` predicate (incoming group invites are `CallKind::Call`).
- `call_hold` validating its SDP.
- A separate, returning branch for the waiting decline.
- The phantom and re-delivered invite rules.
- Transfer armed and confirmed; Add acting only through an explicit button.
- The mute-on-hold path as an outcome.
- Hold detection and `[]` scoped to one-to-one.
- The link read-out moved to "names at connect" (`transfer` arrives on `callAcceptance`).
- `MAX_GROUP_CALL_PEOPLE` for add, and "may still ring" for a timed-out add.
- A bounded hangup for End-and-answer (via plane-reliability).
- The switch procedure spelled out, and the new driver flags renamed (`--press-*`).
- The docs list completed (CLAUDE.md § Audio calls' "left for the user's other devices" sentence,
  the server comments, NATIVE-CALLING § 7).
- The session-id test made scan-driven.
- `kind` kept, with an `escalated` flag beside it.
- The transfer success sentence made explicit.

**Rejected or changed from what was suggested, and why:**

- **"Prefer the unthreaded `addParticipant` on a 1:1"** (project-rules review). Rejected as the
  default. The client's own rule picks `addParticipantsAndModality` whenever the session holds a
  thread (`o.threadId||o.groupId?…`), and the captured client invitation carries `groupChat`
  (NATIVE-CALLING.md § 2.3). A body that diverges from the client is how this plane earns silent
  failures. Kept instead: the client's rule, the privacy check in step 10, the flag until it passes,
  and the unthreaded door tried once only after a named 4xx.
- **"Narrow enforce to ended or waiting session ids"** (code review). Not adopted. A second routing
  rule beside incoming-and-conversation's would be two answers to "which call is this frame
  for". Its rule (ignore any segment that names no live session) already covers both, and the
  narrowing is replaced by the three-provenance proof before M4 relies on it.
- **"Two-step confirm for Add"** (project-rules review, offered as "consider"). Changed: the row
  tap no longer acts and an explicit button does, but Add is not armed. CLAUDE.md states that a
  placed call, a group call to twenty people included, is one click with the words carrying the
  difference. Add rings one person. Transfer IS armed, because it also drops the user's own leg
  and moves the caller.
- **"Mask the keypad echo"** (code review). Changed: the VISUAL echo stays, because it is how a
  person checks what they pressed, as on any phone; only the live region is reduced to a count.
- **A separate observe-mode routing switch** (the draft's own M0). Dropped in favour of the
  sibling rule plus the proof runs, for the same one-rule reason.
- **"A single-PC retarget experiment decides the M5 shape"** (the draft). Reversed on the code
  review's evidence. The experiment is kept as information only, because a browser that tolerates a
  fingerprint change says nothing about what the service expects of the answer.

**Documentation to update when each milestone lands:**

- **NATIVE-CALLING.md:**
  - a new numbered section "Call control — hold, waiting, keypad, add, transfer, park", after
    meeting-participation's § 11, with § 4's table per action and each measurement as it lands;
  - the correction that `/call/holdCompletion/` and `/call/resumeCompletion/` are park callbacks
    and `hold` is the park door;
  - § 6's RPC table (`call_hold`, `call_add_participant`, `call_transfer`) and its "deliberately
    NOT built" line;
  - § 7 (Consent);
  - § 8's open items (directions-only renegotiation, how a remote hold reaches us,
    `telephone-event`);
  - § 9: the names (`hold`, `setHold`, `getMediaModalitiesForHold`, `lG`, `vT`, `yV`, `iG`) are the
    stable search keys, not the 2026-11-05 offsets.
- **AGENTS.md (CLAUDE.md) § Audio calls:**
  - "Four methods reach a person" grows by three, one sentence each;
  - the rules on silence-first hold, the keypad having no RPC by design, the waiting ring and its
    60 s expiry, the one-to-one predicate, and never posting `hold`;
  - "An invite that arrives while a call is up is left for the user's other devices to ring" is
    rewritten.
- **AGENTS.md § A call is a PAGE:** the More rows, the held captions, the keypad and picker panels,
  and the z-index rule.
- **AGENTS.md § Automation safety:** the three names in the guard list, the scoped `hangUp`, and
  the `--press-*` flags, which aim at the same pinned conversation.
- **AGENTS.md § Video in a meeting:** a pointer that M3's run measures directions-only
  renegotiation, which video-send's 1:1 activation relies on.
- **Code comments:** `src/bin/server.rs:2134-2139`, `:14678-14680`, `:15028-15030`.
- **Script usage blocks:** `web/scripts/call-live.ts` and `web/scripts/preview.ts` (`--call-control`).
