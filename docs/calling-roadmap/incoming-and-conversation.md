# Incoming calls and the conversation leg, done the way the Teams web client does them

**Workstream:** `incoming-and-conversation` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

The goal is to take an incoming call the way the real client does, and give every call the doors
of its CONVERSATION leg: `leave`, `updateEndpointState`, `notificationLinks`, `unmute`. Three
things follow:
- hanging up an incoming call tells the caller;
- the user's mute is published the way meetings read it;
- long calls are kept alive the way the service expects.

The client attaches to the forked leg AT RING TIME and joins the call's conversation in the same
POST (`WV`). That is what lets it decline with a `DELETE {callEnd}` on the leg, and be told when a
ring ends elsewhere: the caller cancelled, the call was answered on another device, forwarded, or
sent to voicemail.

This design gets there in steps. Each step can ship alone, and none regresses the verified audio
path.
1. **M0** makes every calling frame belong to the call whose callbacks it arrived on. This is the
   prerequisite: today a frame from any conversation can rewrite or end the live call.
2. **M1–M3** make the microphone follow the call across pages, read what the service already
   says, and publish the mute the client's way. The mute publish sits behind a switch.
3. **M4a** joins the conversation at ANSWER time, on the user's own click. It fixes the incoming
   hangup with nothing automatic.
4. **M4b** moves the attach to RING time. It is opt-in, gated per kind, and is the one step that
   needs the user's explicit approval of a second automatic calling act.
5. **M5** promotes what passed. **M6–M9** add the keep-alives, real group and meeting invites,
   several windows, and the self-unmute while soft-muted.

Every protocol change sits behind a per-backend setting the user flips in Settings
(`call_protocol_set`, a write-token RPC). No environment variable switches any of it.

## 2. Where we are today

All references were re-checked against this worktree (HEAD `c941850`) and against
`calling-pluginless.js` of 2026-11-05. `@N` is a character offset in that file.

| Area | Today | Where |
| --- | --- | --- |
| Frame routing | After the invite and sharing branches, EVERY frame's links are merged into the one live call (`call.links.merge`). ANY `callEnd`/`conversationEnd` ends it. The session id every callback carries (`{surl}callAgent/{sessionId}/{causeId}{path}`) is never compared. | `src/bin/server.rs:14682-14692`, `:14715-14722`; `src/calling.rs:283-291` |
| Invite parse | Reads `callNotification` only: no `conversationInvitation`, no kind, no subject, no `meetingInfo`. | `src/calling.rs:481-566` |
| Ringing | `kind: CallKind::Call` always. Nothing is attached or joined while ringing. | `src/bin/server.rs:14946-15040` |
| Answer | Measured 2026-09-04 (NATIVE-CALLING.md § 8a): `attach_payload` (with SDP, `sender`, `acceptedCallModalities`) to `links.attach`, then `acceptance_payload` to the returned `acceptance`. Does not re-check `phase == Ringing`. | `src/bin/server.rs:7815-7958`; `src/calling.rs:1303-1349` |
| Decline | POST `{callRejection:{sender,code:603,phrase:"Decline"}}` to the invite's `reject`. Never measured. | `src/bin/server.rs:8369-8416`; `src/calling.rs:1544-1552` |
| Hangup (incoming) | `Links::hangup` falls through to `callLeg`. Whether the caller is told is UNMEASURED (§ 8a "Still open"). | `src/calling.rs:390-392` |
| Hangup (outgoing, meeting) | `Links::hangup` picks `hangup`/`end`/`leave`/…; the measured outgoing door is "a hangup link of its own" (§ 8b names no link). On a meeting `leave` takes `hangup_payload`, measured (§ 8). | same |
| Self-mute publish | `{muteUnmute:{sender,muted,scope:"Myself"}}` to `links.mute`/`unmute`. Unmeasured, and not the client's toggle. An incoming 1:1 has no such link, so it answers `told_service:false`. | `src/bin/server.rs:8423-8457`; `src/calling.rs:1559-1567` |
| Mute on the page | `setCallMuted` disables only the PRESSING page's track. `onCallState` never applies `call.muted`, so Mute on the phone does not silence the laptop that holds the microphone. A failed `call_mute` keeps the local mute. | `web/src/lib/store.ts:3340-3351`, `:2623-2643` |
| Keep-alive | Every 20 s while Connected, `{}` to `links.keep_alive()` = `"keepAlive"`, a name no measured link set holds, so nothing is ever sent. No conversation keep-alive. | `src/bin/server.rs:757`, `:14536-14547`, `:15194-15210`; `src/calling.rs:419-421` |
| Ring end | Only a local 60 s timeout, applied to `Ringing` AND `Dialing`. `CallEndReasonUnanswered` has no label, so it reads "The call ended." | `src/bin/server.rs:773`, `:15168-15186`; `web/src/lib/call.ts:341-372` |
| End parse | Requires a `callEnd`/`conversationEnd` KEY. A bare body on `/call/end/` is missed. | `src/calling.rs:585-608` |
| Ringing guards | `call_accepted_in_frame` moves `Ringing → Connecting`. A media answer sets `Connected` for any call that has not ended. Harmless today only because an unattached ring receives no frames. | `src/bin/server.rs:14788-14805`, `:14891-14907` |
| Several pages | Every page draws a connected call. Any page may press Answer. A failed answer runs `hangUpCall`, which DECLINES a call that is still ringing. | `web/src/lib/call.ts:175-177`; `web/src/lib/store.ts:3098-3133`; `src/bin/server.rs:8378-8385` |
| HTTP | The client has no timeout. `post_signal` can only POST. | `src/bin/server.rs:4818`; `src/calling.rs:2188-2276` |
| Guard | `call_*` methods are named one by one in both alternations. No rule covers a cargo example on the calling plane. | `.claude/hooks/guard-live-automation.sh:981`, `:753-800` |

## 3. Gaps covered, deferred and rejected

**Covered:**
- `incoming-attach-at-ring-and-join`: M4a at answer time, then M4b at ring time.
- `incoming-hangup-reaches-caller`: M4a.
- `decline-uses-client-shape`: M4b.
- `ring-cancelled-or-answered-elsewhere` and `answered-elsewhere`: M2 for the labels, M4b for delivery.
- `mute-enforced-on-media-page`: M1.
- `self-mute-not-visible-in-meetings`: M3, plus M9 for the self-unmute body.
- `call-keepalive-on-callleg` and `meeting-keepalive-long-meetings`: M6.
- `rung-into-meeting`, and the incoming half of `group-call-ring`: M7.
- `multi-page-call-ownership`: M8.
- Three map open issues:
  - "Frames are not matched to the call by id": M0.
  - "call_accept does not re-check phase==Ringing": M4a, with a typed outcome.
  - "CallEndReasonUnanswered has no label": M2.

**Deferred or rejected:**

| Item | Decision | Why |
| --- | --- | --- |
| Inbound transfer / redirect / replacement | Deferred to `advanced-call-control` | Accepting a transfer (`transferAcceptance` to `links.TRANSFER_ACCEPTANCE`) rings a third party with no click. M2 only journals the key names of a frame on `/call/transfer/` or `/call/replacement/`. |
| `callProgress {status:"ringing"}` (`zV`) | Deferred, and stated as a divergence | The client either sets `X-CacheSkypeToken: 1` (when `sendProgressFromCC` is on) or POSTs `callProgress` itself. teams-lite does neither, a combination no client emits (@1294864, @1378737). The flag has no bundle default. The caller already saw "Ringing" from the user's other devices on 2026-09-04. Revisit only if M4b case (g) shows "Calling…". |
| Outgoing group invitation properties | Deferred | ECS-driven. The bundle defaults contradict the captured `enableGroupCallEventMessages`, so nothing changes without a capture. |
| Auto-unmute when the organizer releases a mute the user did NOT ask to lift (client mute state 1) | Rejected | The client re-opens the microphone itself (`handleServerMutedState`: state 1 → unmute, @1934339). A microphone opened by somebody else's action is the one direction this app never takes. State 3 (the user pressed Unmute while soft-muted) IS honoured; see § 4.9. |
| Unmute APPROVAL (`unmuteApprovalLinks`, `rejectUnmuteRequestAsync`, `confirmUnmute`) | Deferred to `meeting-participation` | The round-1 design also deferred the SELF unmute (`NV` to `links.unmute`) as "an approval flow". That was a misreading: it is self-service (§ 4.9) and is in scope here as M9. |
| Acceptance body = client `qV` | Rejected for now | `taking_the_call` is measured to establish a call. Changing it in the same release as the attach would change two variables at once. |
| Hangup/leave phrase drift | Deferred to `plane-reliability` › `protocol-drift-hygiene` | The phrases are accepted on `leave` (measured on meetings). The DOOR is what matters here. |
| Take the call here / move it to this device | Deferred to `advanced-call-control` | It needs transfer-to-self and its own gate. M8 only states where the microphone is. |
| Hang up automatically when the media page is gone | Open decision (§ 12) | An outward act with no click. |
| `one_to_one` on the answer branch (codec filter) | The PREDICATE is owned by `video-receive` M1 (`is_one_to_one()` over `isMultiParty`; README § 3, collision 6); its use is `video-send`'s | M7 reads that predicate for its copy and returns no `kind` for this. `video-send` makes the answering branch return `one_to_one` and updates `call_prepare_says_whether_the_call_is_one_to_one` (`src/bin/server.rs:17584`). |
| General signaling timeouts (place, join, accept) and the transport API | Owned by `plane-reliability` M4 (README § 3, collision 7) | This workstream adds NO transport of its own. It uses plane-reliability's one API (method plus class, `SignalRefused`, `delete_signal`): the attach takes the `Create` class, and the keep-alives take the `End` class. |
| `answerCall` declining on ANY failure | Owned by `plane-reliability` › `answer-failure-declines` | This design only makes the double-answer refusal a typed outcome that never reaches that path (§ 4.4). |
| Missed-call notification, counting, history | Owned by `ringing-alerts-history` › `missed-call-notifications` | This workstream supplies the NAMED end reasons (§ 4.12) and a one-line notice. That workstream may replace the notice with its own surface. |
| Server-mute DETECTION, display and track enforcement | Owned by `meeting-awareness` § 4.6 (M5 display, M6 enforcement) | This workstream owns the self-unmute body and the ONE server-muted `call_mute` branch (§ 4.9; README § 3, collision 10), the `server_mute` value of `call_protocol_set` that meeting-awareness M6's enforcement is, and the shared conversation links (§ 4.3). |

## 4. Protocol

Status vocabulary: **measured** (seen on this tenant), **read-from-bundle** (2026-11-05 client
code), **inferred** (neither).

### 4.0 Frame routing: a frame acts only on the call whose callbacks it arrived on (M0, local)

- **Rule:** after the relay-config branch and the invite branch, read the session segment of
  `frame.url` (`…/callAgent/<sessionId>/<causeId>/<path>`).
  - **Segment present, matching the live call's `callbacks.session_id`:** today's handling.
  - **Segment present, matching nothing live:** journal it ONCE per `(sessionId, path)`
    (`[calling] ignored a frame on <callback_path>: it is addressed to a call session that is not
    the live one here`, as built) and return. That
    covers a call that ended, a declined ring whose conversation keeps posting, and an
    unrelated session.
  - **No segment:** today's handling, plus one journal line naming the path. The rule tightens
    only once the journal shows such frames never act on a call.
- **The content-sharing branches** (`server.rs:14633-14679`) move BELOW the check. They act on the
  live call too.
- **Why the client is safe and we are not:** the client routes each callback to the signaling
  session that owns its path, and a session that ended is IDLE.
  - `rejectIncomingCall` sets `this.fsmState=I.SIGNALING_FSM_STATE.IDLE` (@1357890).
  - `handleCallEnd` returns early in that state: `not handling incoming callEnd since fsmstate =
    SIGNALING_FSM_STATE.IDLE` (@1389185).
- **Status:** inferred from our own link builder (`CallbackBase::link`) plus `callback_path`'s
  measured URL shape (`src/calling.rs:798-812`). Pinned by unit tests.

### 4.1 Attach and join at ANSWER time (M4a, `incoming = join-at-answer`)

- **Method:** POST. **Link:** the invite's `callNotification.links.attach` (measured:
  `https://api.flightproxy…/cc/v1/forked/<guid>/…/attach?i=<ip>`).
- **When:** inside `call_accept`, on the user's Answer press, in place of today's `attach_payload`.
  Nothing happens at ring time.
- **Body:** the client's `WV`, identical to § 4.2, with `requireMediaContent:false`. M4a runs only
  when the invite carried an offer, which the measured 1:1 invites do.
- **Then:** POST the unchanged, measured `acceptance_payload` to the attach answer's
  `callInvitation.links.acceptance` (§ 4.4).
- **Fallback:** on a DEFINITIVE refusal of the attach (an HTTP 4xx, whatever its body), post
  today's measured `attach_payload` on the same link in the same handler. A 4xx created nothing,
  so that is not a second attach. A timeout, a 5xx or a lost connection is NOT a refusal:
  `call_accept` ends the call locally exactly as today (`CallEndReasonAcceptFailed`) and never
  re-attaches.
- **What it buys, with nothing automatic:**
  - the conversation's `leave`, so the hangup reaches the caller (§ 4.6);
  - `updateEndpointState`, so an incoming 1:1 can publish its mute (§ 4.7);
  - the acceptance acknowledgement's keep-alive door (§ 4.10).
- **Status:** the door is measured. The body is read from the bundle. teams-lite has never posted it.

### 4.2 Attach and join at RING time (M4b, `incoming = ring`)

- **Method:** POST. **Link:** the invite's `callNotification.links.attach`. **Join target:**
  `conversationInvitation.conversationController`, a BARE field outside any `links`
  (NATIVE-CALLING.md:651). Read it at `/_decoded/conversationInvitation/conversationController`,
  falling back to `/conversationInvitation/conversationController`.
- **When:** once the ringing session is stored and emitted, in a task of its own (never awaited by
  the frame loop). Only when ALL of these hold:
  - the backend is not read-only;
  - the invite named both a controller and `attach`;
  - the call's KIND is enabled for ring-time attach. M4b enables kind `Call` (1:1) only; Group and
    Meeting invites stay on the answer-time flow until M7's matrix passes;
  - the slot was free. A busy machine attaches nothing, as today.
- **Timeout:** the request goes through plane-reliability M4's one transport in its `Create` class
  (65 s), in a DETACHED task (README § 3, collision 7). `ATTACH_TIMEOUT = 15 s` is a local WAIT on
  that task, never a transport abort: when it ends first, `attach` becomes `Unknown` locally, and the
  task's late answer still lands, as the claim if the ring still stands, or through `hang_up_orphan`
  if the call has gone. Aborting the request would throw away the links that are the only address
  of a leg the service may already have created, which is what plane-reliability's "never cancel one
  whose answer matters" rule exists to prevent.

```json
{
  "attach": {
    "requireMediaContent": "<true iff the invite carried no callNotification.mediaContent>",
    "links": { "end": "<callbacks.link(paths::CALL_END)>" },
    "locationContent": null, "networkContent": null, "areaContent": null
  },
  "capabilities": null,
  "endpointCapabilities": "<RING_ATTACH_ENDPOINT_CAPABILITIES = CAPTURED_ENDPOINT_CAPABILITIES & !16 = 73447>",
  "additionalActions": [{
    "input": {
      "capabilities": null,
      "endpointCapabilities": "<same>",
      "conversationRequest": {
        "roster": { "type": "Delta", "rosterUpdate": "<callbacks.link(CONVERSATION_ROSTER_UPDATE)>" },
        "links": "<conversation_callback_links(callbacks)>"
      },
      "endpointMetadata": { "holographicCapabilities": 3 },
      "participants": { "from": { "id", "displayName", "endpointId",
                                  "participantId": "<invite to.participantId>", "languageId": "en-us" } }
    },
    "name": "join", "url": "<conversationController>", "waitForResponse": true
  }]
}
```

- **Explicit omissions:**
  - no `sender`, no `acceptedCallModalities`, no `mediaContent`, no SDP;
  - no `X-CacheSkypeToken` header (that header belongs to `sendProgressFromCC`);
  - no `payload` envelope (NATIVE-CALLING.md § 2.3).
- **Stated divergences from `WV`:**
  1. **Bit 16 (`brokerEnabledIncoming`) is cleared.** The client sets it only when its broker is on
     (`getEndpointCapabilities`, @1302650), and follows the attach with
     `handleBrokerSubscription` (@1295516). teams-lite implements no broker channel, so it must
     not claim one on the incoming path. The measured answer-time attach declared no capabilities
     at all. **`73447` is therefore a COMPUTED value**, and an explicit exception to the code's own
     rule for these masks ("Not computed … Revisit them together with the join payload, never
     alone", `src/calling.rs:35-42`): it is the captured mask minus one named bit, read from the one
     constant set in `calling.rs` that video-send's ladder reads too (README § 3, collision 9).
     This workstream decides bit 16; video-send decides bit 512. If video-send's R1 (`72951`, bit
     512 clear) is promoted for joins, this attach drops bit 512 too, or this section says why it
     keeps it.
  2. **The join publishes the shared 9-link `conversation_callback_links`** (`src/calling.rs:1221`).
     `WV` names 6. The same service accepts those names on place and join, and a link missing from
     one body fails invisibly; that is why the function is shared.
- **Callbacks:**
  - `/call/end/` (`attach.links.end`) is where the ring's own `callEnd` (487 / 450 / 451 / 452)
    now lands;
  - the 9 conversation callbacks, plus `rosterUpdate`.
  All are already in `paths::`.
- **Bundle evidence:**

```text
@1294788 sendAttachToCallInternal: …this.signalingAgentConfig.sendProgressFromCC&&o.set(I.HEADERS.CACHE_SKYPE_TOKEN,"1"),
  this.http.sendPostRequest({url:this.incomingCallPayload.callNotification.links.attach,customHeaders:o,
  requestName:dt.SEND_CALL_ATTACH.name,payload:WV(this,this.incomingCallPayload.conversationInvitation.conversationController,
  !this.getInitialMediaOfferFromIncomingCallPayload())…
@1167888 function WV(e,t,i=!0){st(e,"signalingSession cannot be null"),st(t,"joinUrl cannot be null");const r={type:"Delta",
  rosterUpdate:lt(e,We.CONV_ROSTER_UPDATE)},n=e.getEndpointCapabilities(),s={payload:{attach:{requireMediaContent:i,
  links:{end:lt(e,We.END)},locationContent:e.getLocationContent(),…},capabilities:null…
@1295516 …c&&c.length>0&&this.processConversationServiceResponse(c[0].output,a,0),this.onCallStatusChanged(I.CALL_STATUS.RINGING,a),
  …this.handleBrokerSubscription(l.response.callInvitation,a)}}).catch(l=>{…this.onCallStatusChanged(I.CALL_STATUS.LOCAL_TERMINATED,a,c.error),
  this.dispose(c.error,a)…
```

- **The join's REQUEST NAME is `SEND_CALL_ATTACH`.** The round-1 consent argument called it
  `JOIN_CONVERSATION_WITHOUT_CALL_MODALITY`, which is wrong. That name belongs to the SUBSCRIBE
  path (`gM(…{subscribe:!0,…})`, @1259555). The attach's join is an `additionalActions` entry with
  neither `stream` nor `callInvitation`: a third shape, whose visibility to other participants is
  UNMEASURED.
- **Outcomes** (`AttachState`):

| Outcome | Next step |
| --- | --- |
| `Attached(leg)` | § 4.3 reads it. |
| `Refused { status }` (HTTP 4xx) | Nothing was attached. Answer uses the measured answer-time flow (`attach_payload`, then acceptance). |
| `Unknown { reason }` (timeout, 5xx, connection) | The leg MAY be attached. Answer must not attach again: `can_accept` turns false with the reason on the card ("This call could not be claimed here. Answer it on another device."). The ring is left to end on its own. A Decline posts the legacy rejection to the invite's `reject`. |

  The client disposes the call on any attach failure. Falling back on a definitive refusal is a
  deliberate divergence, because the fallback is the measured flow.
- **Decline while the attach is in flight:** `CallingPlane.declined_while_attaching = Some(…)`
  records the call's id, `local` and invite `reject`, and the call ends locally at once. When the
  attach answer lands, the task sends:
  - on `Attached`, the DELETE decline of § 4.5 on the fresh `callLeg`;
  - on `Refused` or `Unknown`, the legacy rejection.
- **Status:** read-from-bundle. Never posted by teams-lite. The visibility of the join is unmeasured.

### 4.3 Read the attach answer and every conversation object (explicit pointers)

- **Leg doors:** read from `/callInvitation/links/{acceptance, callLeg, mediaAnswer, progress,
  redirection, newOffer}`, plus `/callInvitation/mediaContent` (the offer, when one was required),
  `/callInvitation/callModalities` and `/participants/from`.
- **Conversation doors:** read from `/additionalActionResponses/0/output`, and with the same reader
  from:
  - the join answer and the place answer;
  - every `/conversation/conversationUpdate/` frame;
  - every conversation keep-alive response.
- **The conversation reader:** `ConversationLeg { controller, links, sequence_number, state }`.
  `links` is built from the TOP-LEVEL `links` object only and REPLACES the previous set
  wholesale. It runs under the client's sequence gate: an object whose `sequenceNumber` is below
  the last one applied is ignored for these fields. Accessors: `leave()`,
  `update_endpoint_state()`, `notification_links()`, `mute()`, `unmute()`.
- **Shared with meeting-awareness:** this is the structure `meeting-awareness` § 4.1 rule 4 calls
  `conversation_links`. Whichever workstream lands first builds it, and both read it. It is learned
  for EVERY call kind, because an incoming 1:1 needs its `leave`. Their "meetings only" rule keeps
  applying to the chat-thread and history fields.
- **Our own callbacks are filtered out:** any link whose URL starts with this call's own
  `callbacks.surl` is dropped (`Links::without_prefix`). Our own callbacks echoed back must never
  be read as a door. `CallSession.links` (the flat merge everything else posts from) is unchanged.
- **Bundle evidence:**

```text
@1378500 this.links[I.LINKS.MEDIA_ANSWER]=t.response.callInvitation.links.mediaAnswer,this.links[I.LINKS.ACCEPT]=t.response.callInvitation.links.acceptance,
  this.links[I.LINKS.REJECT]=t.response.callInvitation.links.callLeg,this.links[I.LINKS.REDIRECT]=t.response.callInvitation.links.redirection,
  this.links[I.LINKS.NEW_OFFER]=t.response.callInvitation.links.newOffer,this.signalingAgentConfig.sendProgressFromCC||this.sendProgress(t),
  …const r=this.getInitialMediaOfferFromIncomingCallPayload()||t.response.callInvitation.mediaContent
@1344429 processConversationServiceUpdate(e,t,i){…this.convJoined=!0;…a=e.sequenceNumber>=0?e.sequenceNumber:-1;if(s&&a<this.updateSequenceNumber){…return}
  …this.links[I.LINKS.CONVERSATION_CONTROLLER]=e.conversationController,…
```

- **Status:** the leg link NAMES are measured: the answer-time attach answered `[acceptance,
  callController, callLeg, mediaAnswer, newOffer, progress, redirection]` (§ 8a). Their nesting
  under `callInvitation.links`, and the join output, are read-from-bundle. M2/M4a journal the key
  names before anything relies on them. Broker-shaped keys are journaled too (`links.subscribe`,
  or anything naming `broker`).

### 4.4 Accept (unchanged body; the claim; the inline acknowledgement)

- **Method:** POST. **Link:** on an attached leg, the attach answer's `acceptance`. On the legacy
  flow, today's pair (`server.rs:7837-7930`).
- **Body:** unchanged and measured (`acceptance_payload`). No second attach on a leg already attached.
- **The claim:** `call_accept` first takes the call atomically: `phase == Ringing && !accepting`
  sets `accepting = true`. Otherwise it answers **`Ok({call_id, outcome: "answered_elsewhere" |
  "not_ringing"})`** and posts nothing.
  - **It is an `Ok`, not an `Err`.** `answerCall`'s catch runs `hangUpCall`
    (`store.ts:3112-3117`), and on a call that is still `Ringing` that is a DECLINE
    (`server.rs:8378-8385`). A refusal-as-error would make the second press decline the user's own
    call.
  - `media_owner` (§ 4.15) is recorded on success. It never decides the claim.
- **Waiting for an in-flight attach:** if `attach == InFlight`, poll for up to
  `ATTACH_SETTLE_WAIT = 20 s`, in 100 ms steps (the `await_sharing_session` pattern). That is more
  than `ATTACH_TIMEOUT` (the local wait of § 4.2), so InFlight always resolves to an outcome or to
  `Unknown` first; the detached request itself may run on to its `Create` bound, and a late answer
  then lands as § 4.2 says. If it somehow has not resolved, answer
  `Ok({outcome:"claiming"})`. The page shows "Still claiming this call — press Answer again in a
  moment." and keeps ringing. It never falls back.
- **Read from the acceptance response**, in BOTH flows (the legacy flow's second POST is an
  acceptance too):
  - `/callAcceptanceAcknowledgement/links/callLeg`, the keep-alive door;
  - `/callAcceptanceAcknowledgement/callKeepAliveInterval`, in seconds.
  The same object may also arrive as a FRAME (`handleCallAcceptanceAck` from a frame body, @1371762).
  `handle_call_frame` reads it there too.
- **At accept, with `mute = endpoint-state`:** publish `{state:{isMuted:<current>}}` once (§ 4.7).
  It is the client's `evaluateEndpointStatesForAccept` / `handleEndpointStatesForAccept`.
- **Bundle evidence:**

```text
@1284070 acceptAsync: …payload:qV(this,a,o,l,u),causeId:c}).then(m=>{…m.response.callAcceptanceAcknowledgement&&(this.logger.info(
  "acceptAsync, handling inlined callAcceptanceAcknowledgement"),this.internalHandleCallAcceptanceAck(m.response,c))
@1371347 this.links[I.LINKS.HANGUP]=e.callAcceptanceAcknowledgement.links.callLeg,this.links[I.LINKS.KEEPALIVE]=e.callAcceptanceAcknowledgement.links.callLeg,
  …this.scheduleKeepAlives(e.callAcceptanceAcknowledgement.callKeepAliveInterval)
@1831542 …v=await s.execute("evaluateEndpointStatesForAccept",()=>this.evaluateEndpointStatesForAccept(this._muteState===2,…));…
  const _=s.execute("handleEndpointStatesForAccept",()=>this.handleEndpointStatesForAccept(v,i))
```

- **Status:** the body and door are measured. The ack fields are read-from-bundle, and the
  interval VALUE is unmeasured (M2 journals it).

### 4.5 Decline a ring attached at ring time (M4b)

- **Method:** DELETE. **Link:** the attach answer's `callInvitation.links.callLeg`, which is the
  client's `LINKS.REJECT`, assigned in exactly one place (@1378500).
- **When it does not apply** (no attach, `Refused`, `Unknown`, or `incoming != ring`): today's POST
  `{callRejection}` to the invite's `reject` stays.

```json
{ "callEnd": { "code": 603, "subCode": 0, "phrase": "CallEndReasonLocalUserInitiated", "resultCategories": ["Success"] } }
```

- **Bundle evidence:**

```text
@1357890 rejectIncomingCall(e,t){…this.fsmState=I.SIGNALING_FSM_STATE.IDLE;const r={code:isNaN(e?.code)?I.CALL_END_CODE.REJECT:e.code,
  subCode:e?.subCode||I.CALL_END_SUB_CODE.SUCCESS,phrase:e?.phrase||I.CALL_END_PHRASE.LOCAL_USER_INITIATED,…};
  r.resultCategories=r.resultCategories||[hr.Success];…this.http.sendDeleteRequest({url:this.links[I.LINKS.REJECT],…payload:mM(this,r)…
@917324 CALL_END_CODE:{REJECT:603,SUCCESS:0,CALL_REDIRECTED:302,CANCEL:487,…CALL_ACCEPTED:450,CALL_FORWARDED:451,CALL_FORWARDED_TO_VOICEMAIL:452,…}
```

- **The client posts no `leave` after a decline.** It goes IDLE and ignores the conversation from
  then on. teams-lite gets the same effect from § 4.0. Whether the service keeps posting to a
  declined ring's conversation is journaled in M4b. Whether we should also LEAVE it is a user
  decision (§ 12).
- **Status:** read-from-bundle. Never pressed live. DELETE is a new verb here (`delete_signal`).

### 4.6 Hang up an answered INCOMING call so the caller is told (M4a)

- **Method:** POST. **Link:** the conversation's `leave` (§ 4.3), and ONLY for a call whose leg
  this machine attached with the WV body (`attach == Attached`).
  - Outgoing calls and meetings keep today's `Links::hangup` chain unchanged. The verified
    outgoing door is "a hangup link of its own" (§ 8b); it is NOT measured to be `leave`, which is
    the correction the round-1 design needed.
  - If an attached call's conversation named no `leave`, fall back to `Links::hangup`.
- **Body:** unchanged `hangup_payload` (`src/calling.rs:1522-1539`, the `fM` shape), measured on a
  meeting's `leave` (§ 8).
- **`hang_up_orphan` on the attached path:** the user hung up while the acceptance travelled. It
  posts to the `leave` of the `AttachedLeg` copy the handler took before the await.
- **Bundle evidence:**

```text
@1354034 terminateEstablishedCall: …const n=this.convJoined?dt.LEAVE_CONVERSATION.name:dt.END_CALL.name…return this.leaveConversation(a,t)
@1357018 leaveConversation: if(this.convJoined){…url:this.links[I.LINKS.LEAVE],requestName:dt.LEAVE_CONVERSATION.name,payload:fM(this,e,r,t.scope)…
```

  `LINKS.HANGUP` is assigned at @1335775, @1371775 and @1386788, and never posted to.
- **Status:** for an incoming call, read-from-bundle. M4a's live check is the user's own End press.

### 4.7 Publish the user's own mute (M3, `mute = endpoint-state`)

- **Method:** POST. **Link:** the conversation's `updateEndpointState` (§ 4.3), or the merged
  `updateEndpointState` where no conversation was read. With neither, nothing is posted and
  `told_service:false`, which is today's outcome on an incoming 1:1.
- **`links.mute` and `links.unmute` are never posted for a self toggle** in this mode. `mute` is a
  SERVER-side mute of the participants it names (`IV`, `MUTE_SCOPE.MYSELF`), a different operation.

```json
{ "from": { "id", "displayName", "endpointId", "participantId", "languageId" },
  "endpointState": { "...": "<the last endpointState this leg published, initially {}>",
                     "state": { "isMuted": "<bool>" },
                     "endpointStateSequenceNumber": "<++seq, the first update is 1>" } }
```

- **Body notes:**
  - It matches the client exactly: `m={...E,...a,endpointStateSequenceNumber:++this.endpointStateSequenceNumber}`,
    where `E` is `latestEndpointState`, which the start body never sets. The round-1 carry-over of
    the join's `endpointProperties` is dropped.
  - A state equal to the last one published is not re-sent (the client's `shouldUpdateEndpointState`).
  - The sequence is per conversation leg.
- **Bundle evidence:**

```text
@1937795 muteUnmute: const n=!t&&(!this.isServerMuted||this._callInLobby);…if(t)this.setMuted(2,i),r.info("is updating endpointState for mute"),
  this.updateEndpointStateForMute(!0,i);else if(n)…this.setMuted(0,i),this.updateEndpointStateForMute(!1,i);else return r.info("is unmuting on server")…
@1228224 updateEndpointState=(a,o=Ht(),l,c)=>{…const u=this.links[I.LINKS.UPDATE_ENDPOINT_STATE],E=this.getEndpointState();…
  const m={...E,...a,endpointStateSequenceNumber:++this.endpointStateSequenceNumber};…
@1287947 getEndpointState=()=>{let a;return this.startOrJoinConvResponseReceived?a={...this.latestEndpointState}:a={...this.requestedEndpointStateWhileConnecting},a}
@1240009 muteAsync=(a,o,l=Ht())=>(…this.participantManager.muteAsync(this.links[I.LINKS.MUTE],a,o,l))   // a SERVER mute, not the toggle
```

- **Status:** the roster field it fills (`endpoints[<id>].endpointState.state.isMuted`) is measured
  (NATIVE-CALLING.md:1052-1068). The publish itself is unmeasured.

### 4.8 Server mute: read by meeting-awareness, honoured here only by narrowing

- **Detection, display and enforcement** belong to `meeting-awareness` § 4.6 (M5 display, M6
  enforcement as the `server_mute = enforce` value of this workstream's `call_protocol_set`; README
  § 3, collision 10).
- **The rule for SELF is `handleSelfServerMutedState` (@1954977), not `isParticipantServerMuted`.**
  It reads our endpoint's `stream || streamLobby` `.serverMuted` FIRST, then the `audio` and
  `audio-teleconferencing` media streams, and never `endpointState.isMuted`. meeting-awareness's
  rule 6 is the remote-participant test, and the self test should be this one. That is the
  coordination note in § 13.
- **What this workstream relies on:** `call.server_muted`, once it exists. M1's rule treats it as a
  reason to keep the track closed, and M9's unmute branch reads it.
- **What this workstream will NOT do:** refuse an Unmute because of a server mute. The round-1
  refusal would have trapped a soft-muted attendee, where today they can at least unmute locally.

```text
@1954977 handleSelfServerMutedState(t){const i=jn.filterVirtualEndpoints(this.localSignalingParticipant.endpointDetails),
  r=(0,ti.find)(i,o=>this._endpointId===o.endpointId),n=r&&r.mediaStreams,s=n&&n.filter(o=>o.type==="audio"||o.type==="audio-teleconferencing"),
  a=r&&(r.stream||r.streamLobby);if(a){const o=a.serverMuted??!1;…}else if(s&&s.length>0){const o=s.some(l=>l.serverMuted);…this.handleServerMutedState(t)}}
```

### 4.9 Unmute oneself while soft-muted (M9, `mute = endpoint-state`)

- **Method:** POST. **Link:** the conversation's `unmute` (§ 4.3).
- **When:** `call_mute(false)` arrives while `call.server_muted` is true.
  - **No `unmute` link:** refused before the network with "call_mute: the organizer muted you and
    this meeting offers no way to unmute yourself."
  - **Otherwise:** post the body below, and leave `call.muted` true.

```json
{ "from": { "id", "displayName", "endpointId", "participantId", "languageId" }, "mediaTypes": ["audio"] }
```

- **The local track stays closed** (the client's "is unmuting on server", state 3), and the page
  records this window's own pending unmute.
- **When the roster clears `server_muted`:** the page that holds media and holds that pending unmute
  opens the track, and § 4.7 publishes `isMuted:false`. A release the user did not ask for (state 1)
  opens nothing.
- **The asynchronous form** (`unmuteAsyncEnabled`, which adds `links.clientCallback` and an
  `operationId`) is ECS-only and is not sent.
- **Bundle evidence:**

```text
@1240009 unmuteAsync=(a=Ht())=>(…Ni(this.fsmState===I.SIGNALING_FSM_STATE.CONNECTED,"unmute operation only allowed in a connected call"),
  this.participantManager.unmuteAsync(this.links[I.LINKS.UNMUTE],a))
@1099404 function NV(e,t=!1,i=null){…const r={from:{id:…,displayName:…,endpointId:…,participantId:…,languageId:…},mediaTypes:["audio"]};
  return t&&(r.links={clientCallback:lt(e,We.MUTE_UNMUTE_ASYNC,i)},r.operationId=i),{payload:r}}
@1936223 updateCapabilities(…){…this.isServerMuted&&!this._callInLobby?this._commandUrlPresence&Dm.Unmute&&(r=!0):this.isMuted&&(r=!0)…   // canUnmuteSelf
@1934339 handleServerMutedState(t){…if(!this.isServerMuted){const r=this._muteState===3;if(this._muteState===1||r)…"is unmuting, as we are no longer server muted"…
```

- **Status:** read-from-bundle. A server mute of SELF has never been measured, so M9 waits for
  meeting-awareness § 9 step 6.
- **This is the ONE server-muted branch of `call_mute`** (README § 3, collision 10). This
  workstream owns its body and its door. meeting-awareness M6 does not add a second branch: its
  enforcement is the `server_mute = "display" | "enforce"` value of this workstream's stored
  `call_protocol_set` (default `display`), and it adds only the page's track rules (meeting-awareness
  § 4.6). meeting-awareness M6 lands after this M9, so its POST is this body from the start.

### 4.10 Call keep-alive on the call leg (M6, `keepalive = client`)

- **Method:** POST. **Link:**
  - **incoming:** `/callAcceptanceAcknowledgement/links/callLeg` (§ 4.4);
  - **outgoing and meeting:** `callAcceptance.links.callLeg` on the `callAcceptance` frame.
  The dead `keepAlive` accessor (`src/calling.rs:419`) is removed.
- **Body:** `{"callParticipantUpdate":{}}` (the client's `pM(this,!1)`).
- **Interval:** every `max(floor(0.9 × callKeepAliveInterval × 1000), 15 000)` ms, re-armed after
  each attempt. The 15 s floor is ours, against a misread interval. Sending stops for good on 404.
  With no interval named, nothing is sent and one journal line says so, which is today's effective
  behaviour.
- **Gates:** only while `Connected`; refused before the network on a read-only backend.

```text
@1400547 scheduleKeepAlives(e){const t=e*.9*1e3;…this.keepAliveTimer=window.setInterval(this.sendKeepAlive.bind(this),this.keepAliveInterval)}
@1402881 …url:this.links[I.LINKS.KEEPALIVE],payload:e,requestName:dt.SEND_KEEP_ALIVE.name…restartKeepAliveInterval… keepAlive stopped since last request returned NOT_FOUND.
@1387285 this.links[I.LINKS.KEEPALIVE]=e.callAcceptance.links.callLeg,…this.scheduleKeepAlives(e.callAcceptance.callKeepAliveInterval)
```

- **Status:** read-from-bundle. The interval value is unmeasured, and so is whether the service
  drops a call without it.

### 4.11 Conversation keep-alive (M6, `keepalive = client`)

- **Method:** POST. **Link:** the conversation's `notificationLinks` (§ 4.3).

```json
{ "participants": { "from": { "...": "local" } },
  "roster": { "type": "Delta", "rosterUpdate": "<callbacks.link(CONVERSATION_ROSTER_UPDATE)>" },
  "links": "<conversation_callback_links(callbacks)>" }
```

- **Interval:** `floor(2_700_000 × (1 − r/100))` ms, with `r` drawn uniformly from 10..=20 ONCE
  per call (the client caches it), i.e. 36:00 to 40:30.
- **Response handling:** the answer goes through the § 4.3 reader. A retryable failure (5xx or
  timeout) re-arms the timer. Any other failure drops `notificationLinks` and stops, as the
  client does. Timeout: plane-reliability M4's `End` class (README § 3, collision 7), so the
  keep-alive needs no bound of its own.
- **Stated divergence:** the client's `vG` publishes `VP(e,lT)`, every entry of its `lT` table
  (about 35, @927761). This sends the same 9-link set the join published, so the set the service
  holds never changes under it.

```text
@1400768 scheduleConversationKeepAlive(){…if(!this.conversationKeepAliveInterval){const e=…conversationKeepAliveTimeoutSec,t=OH(e*1e3);…
  this.conversationKeepAliveInterval=t}…}
@979187 function OH(e){const t=Math.floor(Math.random()*11)+10;return Math.floor(e*(1-t/100))}
@982507 conversationKeepAliveTimeoutSec:2700
```

- **Status:** read-from-bundle. Whether a meeting drops at about 45 minutes is unmeasured (the
  longest recorded hold is 42 s, NATIVE-CALLING.md § 8).

### 4.12 A ring that ended elsewhere (parse in M2, delivered in M4b)

- **Frame:** our `/call/end/` (or `/conversation/conversationEnd/`), routed by § 4.0.
- **Read:**
  - `callEnd`/`conversationEnd` at `/_decoded/…` or the root, OR the bare body when
    `frame.url.contains(paths::CALL_END)` or `paths::CONVERSATION_END`. The bare body sits at
    `/_decoded` when the frame was wrapped and at the root otherwise, because `CallFrame.body` is
    the envelope;
  - `callControllerTransactionEnd` overrides the outer end;
  - fields `code`, `subCode`, `phrase`, `acceptedElsewhereBy{id,displayName}`, at either level.
- **Named endings** (Rust constants, pinned against `web/src/lib/call.ts` by a test):

| Code | While | Name | Sentence |
| --- | --- | --- | --- |
| 450 | ringing | `CallEndReasonAnsweredElsewhere` | `ended_elsewhere: {by_self: id == our mri, name}`; the name is resolved through `display_name_for_mri` |
| 451 | ringing | `CallEndReasonForwarded` | |
| 452 | ringing | `CallEndReasonForwardedToVoicemail` | |
| 487 | ringing | `CallEndReasonCallerCancelled` | |

```text
@1389185 handleCallEnd(e,t){let i=e.body.callEnd||e.body;…if(this.fsmState===I.SIGNALING_FSM_STATE.IDLE){…return}…
  (later) i.callControllerTransactionEnd&&(…i=i.callControllerTransactionEnd)…i.acceptedElsewhereBy&&(this.acceptedElsewhereBy={id:…,displayName:…})
```

- **The local 60 s expiry stays:** a backstop that sends nothing, matching the client's
  establishment timeout, which disposes and sends nothing (@1403459). Its label now branches on
  DIRECTION:
  - incoming: "Missed call from {Name}.";
  - outgoing: "{Name} did not answer."
  `expire_unanswered_call` covers `Dialing` too (`server.rs:15175-15177`).
- **Status:** read-from-bundle. Measured only in the negative today (an unattached endpoint is told
  nothing).
- **As implemented (M2, IC2):**
  - `ringing` is `CallSession::is_being_offered`: incoming, `connected_at_ms` unset, and the
    phase `Ringing` or `Connecting` (a `callAcceptance` frame moves a ring to `Connecting`
    without anybody answering here).
  - An `acceptedElsewhereBy` is a 450 whatever the code: naming who took the call is a statement
    only an answer elsewhere makes. A 450 that names nobody carries `ended_elsewhere: null`, and
    the page says "The call was answered elsewhere." rather than guessing who.
  - The override is read FIELD BY FIELD (the override first, the outer object where it is
    silent), so a field the service put on only one of the two is never lost.
  - `by_self` compares OIDS (`calling::same_person`: case and the `8:orgid:` prefix), because
    the two ids come from different services. The name is the store's
    (`display_name_for_mri`) before the frame's `displayName`.
  - Outgoing with no one name ("Design crew", or a callee with no name): "Nobody answered."
  - The local bound says "did not answer" only once the outgoing invite is out
    (`CallSession::invite_sent`, set when `call_place` is answered). It counts from
    `call_prepare`, so a reservation that expires before its invite left (the user sat on the
    microphone prompt) rang nobody, and ends as `CallEndReasonPlaceFailed` (§ 13 note 21).
  - Every ending writes `[calling] the service ended the call: code … sub-code … "…" — named …`.

### 4.13 The invite's conversation, kind and subject (parse in M2, used in M7)

- **New `IncomingCall` fields:**
  - `conversation_controller`: `/_decoded/conversationInvitation/conversationController`;
  - `multi_party` and `hostless`: `…/isMultiParty` and `…/isHostless`;
  - `subject`: `…/subject`;
  - `meeting`: `/_decoded/meetingInfo` or `/_decoded/meetingData` present, or a
    `groupChat.threadId` starting `19:meeting_`;
  - `evt`: the frame's own `evt`.
  Each is read with a root fallback.
- **Kind:** meeting → `Meeting`; `multi_party` → `Group`; otherwise `Call`. An unknown shape
  stays `Call`, which is today.

```text
@1376555 processIncomingCallPayload: …e.conversationInvitation&&(this.links[I.LINKS.CONVERSATION_CONTROLLER]=e.conversationInvitation.conversationController,
  this.setMultiParty(e.conversationInvitation.isMultiParty,t),this.setIsHostless(!!e.conversationInvitation.isHostless,t)),…
  e.meetingData&&(this.meetingData=e.meetingData),e.meetingInfo&&(this.meetingInfo=e.meetingInfo)
@822573 Am={INCOMING_SKYPE_NGC_CALL:107,INCOMING_SKYPE_NGC_GVC_CALL:109,…}
```

- **Status:** `conversationInvitation` being present is measured. The field names are
  read-from-bundle. A 109 invite has never been captured.
- **As implemented (M2, IC2):** the fields live in ONE struct, `IncomingCall.conversation:
  InviteConversation` (`evt`, `invitation_keys`, `controller`, `multi_party`, `hostless`,
  `subject`, `meeting`), and every invite journals its `shape()` — key names and yes/no, never the
  subject or the controller URL. The kind is unchanged (still `Call`); M7 reads the struct. A
  meeting is also recognised by a `groupChat.threadId` beside the notification, not only inside it.

### 4.14 `callProgress` "ringing": deliberately NOT sent

- **Method:** POST (not implemented). **Link:** the attach answer's `callInvitation.links.progress`.

```json
{ "callProgress": { "sender": { "...": "local" }, "status": "ringing", "phrase": "ringing" } }
```

- **Bundle evidence:** `@1378737 this.signalingAgentConfig.sendProgressFromCC||this.sendProgress(t)`.
- **Status:** read-from-bundle. Deferred (§ 3), and a stated divergence.

### 4.15 Which page holds the call (M8, local)

- **The page id:** each tab mints one with `crypto.getRandomValues`. `crypto.randomUUID` is
  secure-context-only, and this app is reached over plain http on overlay networks. The id is kept
  in `sessionStorage`.
- **Duplicate tabs:** at load the tab posts `claim <id>` on `BroadcastChannel("teams-lite-pages")`.
  A live tab holding the same id (a duplicated tab copies `sessionStorage`) answers `taken`, and the
  newcomer mints a fresh id. Until the check resolves, placement is `unknown`. Where
  `BroadcastChannel` does not exist, placement is never `lost`.
- **Where the id travels:** `call_prepare` (place and join) and `call_accept` carry it, checked
  `^[0-9a-f]{32}$`. It is recorded on the reservation, or at a successful accept. It is NOT a
  credential and never decides anything outward: the double-answer refusal keys on
  `phase`/`accepting` (§ 4.4).
- **One mechanism, owned by media-robustness M7** (README § 3, collision 2). The param is named
  `page` (on `call_prepare`, `call_accept` and media-robustness' `call_media_state`), and the
  backend records it as the `media_owner` state field, beside media-robustness' `media`
  (`held | released | lost | unknown`), `PageLease` and `MEDIA_OWNER_GRACE`. This workstream adds no
  param of its own: M8 contributes the id's minting (above, `getRandomValues`, `sessionStorage`, the
  duplicate-tab check) and consumes that state in `callMediaPlacement`. Wherever this design says
  "carry `media_owner`", read "carry `page`, recorded as `media_owner`".

## 5. Changes by file

### `src/calling.rs`

- `IncomingCall` (`:481`) gains `conversation_controller`, `multi_party`, `hostless`, `subject`,
  `meeting` and `evt`. `incoming_call_from_frame` (`:514`) reads them as in § 4.13.
- **New `pub fn session_of(url: &str) -> Option<&str>`:** the segment after `/callAgent/`, which
  is `CALL_AGENT` (`:72`). Used by § 4.0.
- **New payloads:**
  - `pub fn ring_attach_payload(local, controller, callbacks, require_media: bool) -> Value`, the
    WV of § 4.2, with `RING_ATTACH_ENDPOINT_CAPABILITIES`;
  - `pub fn decline_payload() -> Value`, the `mM` of § 4.5;
  - `pub fn endpoint_state_payload(local, endpoint_state: &Value) -> Value`, the `uG` of § 4.7;
  - `pub fn self_unmute_payload(local) -> Value`, the `NV` of § 4.9;
  - `pub fn call_keep_alive_payload() -> Value`;
  - `pub fn conversation_keep_alive_payload(local, callbacks) -> Value`;
  - `pub fn keep_alive_interval_ms(&Value) -> Option<i64>`, reading both locations with the 15 s floor;
  - `pub fn conversation_keep_alive_interval_ms(r: u8) -> i64`, with `r` clamped to 10..=20.
  `attach_payload` and `mute_payload` STAY: they are the legacy flows' bodies.
- **New `pub struct AttachedLeg`** with `acceptance`, `decline` (= `callInvitation.links.callLeg`),
  `media_answer`, `progress`, `redirection`, `new_offer`, `offer: Option<MediaContent>`,
  `conversation: Option<ConversationLeg>` and `raw`. Read by `pub fn attached_leg(response,
  own_surl) -> AttachedLeg`, with explicit pointers.
- **New `pub struct ConversationLeg`** with `controller`, `links: Links` (top-level only, replaced
  wholesale), `sequence_number: i64` and `state`. Read by `pub fn
  ConversationLeg::from_response(&Value, own_surl) -> Option<Self>` and `pub fn apply(&mut self,
  newer: ConversationLeg) -> bool`, the sequence gate. Accessors are those of § 4.3.
  `JoinedConversation` (`:1848`) gains `conversation: Option<ConversationLeg>`; its existing flat
  `links` is unchanged, so `call_join`'s merge does not move. `PlacedCall` gains the same field.
- **`Links`:** add `without_prefix(&str)` and `update_endpoint_state()`. Remove `keep_alive()` in M6.
- **`CallEnded`** (`:573`) gains `sub_code: i64` and `accepted_elsewhere_by: Option<(String,
  String)>`. `call_ended_from_frame(url, frame)` implements § 4.12. New constants beside
  `END_REASON_UNREACHABLE` (`:614`): `END_REASON_ANSWERED_ELSEWHERE`, `END_REASON_FORWARDED`,
  `END_REASON_VOICEMAIL`, `END_REASON_CALLER_CANCELLED`. New `pub fn end_reason_for(&CallEnded,
  ringing: bool) -> Option<&'static str>`.
- **As implemented (M2, IC2):** `CallEnded` gained `sub_code` and `accepted_elsewhere_by`;
  `call_ended_from_frame(url, frame)`; the five `END_REASON_*` constants (the four above plus
  `END_REASON_UNANSWERED`, which was a literal in server.rs); `end_reason_for`; `same_person`. And
  three journal readers that decide nothing: `keep_alive_offer` (§ 4.4's interval and `callLeg`,
  off an acceptance answer or frame), `conversation_doors` (§ 4.3's four doors and any
  broker-shaped key, off the place, join and accept answers and every `conversationUpdate`), and
  `body_key_names` (a `transfer` or `replacement` frame). In server.rs the frame half is
  `note_frame_shapes`, step 4 of the handler's documented order, once per distinct shape through
  `FrameNotes`; the answer half is `note_answer_shapes`, once per request.
- **New typed refusal:** `pub struct SignalRefused { pub status: u16 }`, carried in the `anyhow`
  chain so `e.downcast_ref::<SignalRefused>()` tells a definitive 4xx from a lost answer.
- **The transport is plane-reliability M4's, not a second one** (README § 3, collision 7). This
  design originally added `send_signal(method, …, timeout: Option<Duration>)` and `delete_signal`
  here; both are folded into plane-reliability's one API (`post_signal`, `put_signal` and
  `delete_signal`, each taking a `SignalClass`, over the header builder and reply reader extracted
  once), which lands before M4a. `SignalRefused` (above) is that API's typed refusal. This workstream calls it: the ring-time
  and answer-time attach in the `Create` class (with § 4.2's local wait on a detached task), the
  DELETE decline through `delete_signal`, and the keep-alives in the `End` class.
  `a_calling_request_carries_one_credential_and_no_api_version` (`:3312`) scans the shared header
  builder.
- **New pure enums**, parsed from the stored setting (§ 6): `CallProtocol { incoming: Answer |
  JoinAtAnswer | Ring, ring_kinds: Vec<Kind>, keepalive: Off | Client, mute: Legacy |
  EndpointState, server_mute: Display | Enforce }`. An unknown or absent value reads as the
  default. `server_mute` is added by meeting-awareness M6 (README § 3, collision 10).

### `src/bin/server.rs`

- **`CallingPlane`** (`:2140`) gains:
  - `declined_while_attaching: Option<PendingDecline { call_id, local, reject }>`;
  - `protocol: calling::CallProtocol`, loaded at `start_calling` and updated by the RPC;
  - `ignored_sessions: HashSet<(String, String)>`, for § 4.0's journal dedup.
- **`CallSession`** (`:2229`) gains:
  - `protocol`, a snapshot taken when the call starts: a call keeps the protocol it started with;
  - `attach: AttachState { NotAttempted, InFlight, Attached(Box<AttachedLeg>), Refused(u16),
    Unknown(String) }`;
  - `conversation: Option<ConversationLeg>`;
  - `endpoint_state: Value` and `endpoint_state_sequence: u64`;
  - `mute_seq: u64`;
  - `keep_alive: Option<KeepAlive { url, interval_ms, due_at_ms, stopped }>` and
    `conversation_keep_alive: Option<(i64 interval_ms, i64 due_at_ms)>`;
  - `ended_elsewhere: Option<(bool, String)>`, `subject: Option<String>`, `media_owner:
    Option<String>` and `accepting: bool`.
  `CallSession::json` (`:2341`) adds `subject`, `mute_seq`, `ended_elsewhere {by_self,name}`,
  `media_owner`, `can_accept` (now also false while `attach` is `Unknown`, with
  `accept_blocked_reason`) and `protocol`. Constructors at `:7436`, `:7594`, `:14975` and the test
  constructors (`:17246`, `:17302`) are updated.
- **`handle_call_frame`** (`:14601`):
  - **M0 owns the handler's branch ORDER** (README § 3, collision 4). At least eight workstreams
    insert a branch here, each with a constraint (this routing first; meeting-participation's
    abandoned-cause drop and reactions before the link merge; video-receive's acknowledgement after
    it; advanced-call-control's hold detection below the glare return; the content-sharing branches
    below this routing; M2's end parse). M0 writes that order as a comment block at the top of the
    handler and pins it with a source-scan test; every later branch adds itself to the list in its
    own change.
  - § 4.0 routing, before the sharing branches.
  - **Ringing guards for `direction == Incoming && phase == Ringing`.** Only these four act:
    `callEnd`/`conversationEnd` (with § 4.12 naming), the roster fold, the `conversationUpdate`
    refresh of `conversation`, and `invite_failed`. Excluded: `call_accepted_in_frame` (`:14788`),
    the media-answer transition (`:14891`), `media_renegotiation_from_frame` (`:14811`), the
    acceptance-acknowledgement POST (`:14775`) and `lobby_state_in_frame` (`:14746`). Each
    excluded body is journaled by its key names.
  - A `callAcceptance` frame captures `callKeepAliveInterval` and `callLeg`. A
    `callAcceptanceAcknowledgement` frame does the same.
  - M2: frames on `/call/transfer/` and `/call/replacement/` are journaled by key names.
- **`handle_incoming_call`** (`:14946`):
  - **M7:** kind and subject come from the invite. The subject falls back to
    `store.conversation_context`.
  - **M4b:** spawns `attach_at_ring(call_id)` under the § 4.2 conditions.
- **New `async fn attach_at_ring(&self, call_id)`**, as specified in § 4.2:
  1. refuse on `read_only()` before the network;
  2. set `InFlight`, then spawn the POST through plane-reliability's transport in the `Create`
     class and wait on it for at most `ATTACH_TIMEOUT` (a local wait, § 4.2; the late answer still
     lands);
  3. re-read the plane: carry out a `declined_while_attaching`, or store the outcome;
  4. fill `offer` from the attach answer; fold `output.roster`; emit;
  5. journal the shapes by name.
- **`call_accept`** (`:7815`):
  - the claim, and the typed outcomes of § 4.4;
  - the InFlight wait;
  - **M4a:** the WV attach with the 4xx-only fallback;
  - attached: acceptance only;
  - read the inline acknowledgement in both flows;
  - publish `endpointState` at accept when `mute = endpoint-state`;
  - `hang_up_orphan` posts to the attached leg's `leave`.
- **`call_hangup`** (`:8369`), for an attached call:
  - declining → `delete_signal(decline_payload)` on `leg.decline`;
  - `InFlight` → record `declined_while_attaching`, end locally, post nothing;
  - established → POST `hangup_payload` to `conversation.leave()`.
  Every other call keeps today's choice. The journal names the door's ROLE (`decline` / `leave` /
  `legacy`), never its URL.
- **`call_mute`** (`:8423`) bumps `mute_seq` on every accepted request.
  - With `mute = legacy` (the default) it is unchanged.
  - With `mute = endpoint-state` it POSTs `endpoint_state_payload`, skipping a state equal to the
    last one published, and never posts to `links.mute`.
  - **M9:** while `server_muted`, an unmute goes through § 4.9. This is the ONE server-muted branch
    (README § 3, collision 10); meeting-awareness M6 adds no second one.
- **The ticker** (`:14536-14547`): `CALL_KEEPALIVE` (20 s, `:757`) becomes `CALL_TICK = 5 s`. It
  drives `expire_unanswered_call` and two due-time checks, active only when the call's snapshot says
  `keepalive = client`:
  - `keep_call_alive` (`:15194`);
  - new `keep_conversation_alive`.
  Both refuse on `read_only()` and only run while `Connected`.
  **The 60 s ring-expiry sweep moves to this 5 s tick with it**, so once M6 lands, ringing-alerts-history,
  advanced-call-control and plane-reliability M10 cite `CALL_TICK`, not the 20 s ticker (README
  § 3, collision 4). The sweep's 60 s bound is unchanged; it is only checked more often.
  media-robustness M8's pin asserts the behaviour (a relink is one-shot, arms no timer, and the
  ticker posts nothing new after it), not the contents of the `keep_alive()` accessor this milestone
  removes. Note that after this milestone the call keep-alive's door IS `callLeg`, which a relink
  also posts to once.
- **`call_prepare`** (`:7401`) accepts the page id as `page` (§ 4.15; media-robustness M7's
  mechanism). The answering branch (`:7487`) returns `one_to_one` from video-receive's one
  `is_one_to_one()` predicate (README § 3, collision 6), as the placing branch does (`:7645`); M7
  reads that predicate for its copy and returns no `kind` for this.
- **New RPC `call_protocol_set`** (`MACHINE_METHODS`). It takes `{incoming?, ring_kinds?,
  keepalive?, mute?}`, plus `server_mute?` (`display | enforce`, default `display`) once
  meeting-awareness M6 adds it as its enforcement (README § 3, collision 10), validates every value,
  stores JSON under
  `calling_protocol:<own_port()>`, updates `CallingPlane.protocol`, journals one line naming the
  new values, emits `call_state`, and answers the effective protocol. Refusal words: "decides how
  this machine's calls speak to Teams". `call_status` (`:7392`, open) reports `protocol`.
  `start_calling` (`:14454`) journals the effective protocol once.
- **New tests:**
  - `a_frame_acts_only_on_the_call_whose_callbacks_it_arrived_on`: another session's
    `conversationEnd` must not end the live call, and its roster must not fold.
  - `attaching_at_ring_time_is_refused_read_only_and_answers_nothing`.
  - `a_ringing_incoming_call_is_moved_by_nothing_but_its_end`.
  - `a_second_answer_is_an_outcome_and_never_a_decline`.
  - `an_attached_incoming_call_is_left_on_its_conversation` and `an_outgoing_call_keeps_its_own_hangup_door`.
  - `a_declined_ring_is_deleted_on_the_attach_answers_leg`.
  - `a_decline_during_the_attach_is_carried_out_when_it_lands`.
  - `an_attach_that_timed_out_is_never_attached_again`.
  - `the_self_mute_publishes_endpoint_state_only_when_asked`.
  - `the_page_knows_the_names_of_a_ring_that_ended_elsewhere`: `include_str!` of `web/src/lib/call.ts`.
  - `the_keep_alives_are_off_until_asked_for` and `every_keep_alive_refuses_read_only_before_the_network`.
  - `the_calling_protocol_is_per_backend_and_gated`.
  `every_calling_method_that_reaches_a_person_is_outward_facing` (`:17137`) adds `call_protocol_set`
  to the machine half. `the_call_keepalive_is_frequent_enough_to_hold_a_call` (`:17725`) and
  `a_ring_nobody_answered_frees_the_call_slot_and_a_live_call_is_untouched` (`:17526`) are updated
  for `CALL_TICK`.

### `.claude/hooks/guard-live-automation.sh` and its test

- Add `call_protocol_set` to BOTH alternations (`:981`). `every_gated_method_is_named_in_the_automation_guard`
  (`server.rs:17954`) pins it.
- **New example category `examples_reaching_a_call`:** an example whose source names
  `calling::(post_signal|put_signal|delete_signal|place_call|join_meeting|ring_attach_payload|decline_payload|endpoint_state_payload|self_unmute_payload|hangup_payload|acceptance_payload|attach_payload|mute_payload|invitation_payload|call_keep_alive_payload|conversation_keep_alive_payload)`
  is refused unless it is on the tracked allowlist `SANCTIONED_CALL_EXAMPLES="examples/meeting_join_probe.rs"`.
  - This is the live-driver precedent: a constant target, then named in the allowlist.
  - Pinning a thread id instead would break the sanctioned probe, which pins its meeting by
    calendar subject (`examples/meeting_join_probe.rs:39`).
  - `guard-live-automation.test.py` gains a passing fixture (the probe) and a failing one (an
    untracked example calling `calling::delete_signal`).

### `web/src/lib/call.ts`

- `ActiveCall` gains `subject`, `mute_seq`, `ended_elsewhere`, `media_owner`,
  `accept_blocked_reason` and `protocol`.
- Constants `CALL_END_ANSWERED_ELSEWHERE`, `CALL_END_FORWARDED`, `CALL_END_VOICEMAIL` and
  `CALL_END_CALLER_CANCELLED`, pinned from Rust.
- `callEndLabel` (`:341`) gains those cases plus `CallEndReasonUnanswered`, branching on direction.
- `callPhaseLabel` (`:258`) gives a ringing Group and a ringing Meeting their own words.
- **New pure `microphoneShouldBeOpen({trackOpen, backendMuted, backendSeq, serverMuted, pending})
  -> boolean`.** The rules:
  1. `serverMuted` → closed. This composes with meeting-awareness M6.
  2. `pending && backendSeq <= pending.afterSeq` → follow `pending.muted`. The frame predates our
     own press.
  3. A newer frame saying muted → closed.
  4. A newer frame saying unmuted → open ONLY if `pending.muted === false`, which is this
     window's own press.
  5. With no pending press, a frame never opens a microphone.

  **As implemented (M1, IC1):** `microphoneShouldBeOpen({trackOpen, backendMuted, backendSeq,
  pending})` holds rules 2, 3 and 5. Rule 4 is gone (§ 13 note 19): a frame never opens a
  microphone at all, and this window's own Unmute is confirmed by the backend's ANSWER to it
  instead (`followMuteAnswer`). `serverMuted` is not an input yet, because nothing feeds it until
  meeting-awareness M6 adds `call.server_muted`, which then adds rule 1 above the rest. Beside it:
  `followMuteFrame` (one frame applied, including the stale-frame and older-backend cases),
  `pendingMuteAfterFrame` (when a press is forgotten, § 13 note 18), `muteToResend` (only a failed
  Mute, once) and `callShowsMuted` / `unmuteRefusal` (what a window draws).
- **New `callMediaPlacement(call, pageId, holdsMediaHere, duplicateChecked) -> 'here' |
  'elsewhere' | 'lost' | 'unknown'`.**
- Fix the stale header (`:11-13`): "Calling is off until the user turns it on".

### `web/src/lib/store.ts`

- `pageId` per § 4.15.
- `callPrepare` and `callAccept` carry `media_owner`.
- **`answerCall` (`:3098`):** an `outcome` other than `accepted` stops this page's own media, does
  NOT call `hangUpCall`, and reports "This call was answered in another window." for
  `answered_elsewhere`. For `claiming` it reports the § 4.4 sentence.
- **`setCallMuted` (`:3340`):**
  - record `pendingMute = {muted, afterSeq: call.mute_seq}`;
  - apply it to the track at once (unchanged), call the RPC, and keep `pendingMute` on failure;
  - on reconnect, re-send that one press once, if the call is still live and this page holds it.
    **As implemented:** only a failed MUTE, and only while the FRESH `call_status` read's
    `mute_seq` is still at or below `afterSeq` — never from the page's copy from before the drop
    — and the press carries `if_seq`, so the backend refuses it if another was recorded since
    (§ 13 notes 18 and 19). A failed Unmute takes itself back instead: the track closes again and
    it is never re-sent.
- **`onCallState` (`:2623`):** applies `microphoneShouldBeOpen` when this page holds media, and
  clears `pendingMute` once `backendSeq > afterSeq && backendMuted === pending.muted`.
  **As implemented:** it clears `pendingMute` at the FIRST frame with `backendSeq > afterSeq`,
  agreeing or not (§ 13 note 18). A `call_status` read is applied the same way, and a frame
  older than one already applied is ignored for the microphone (frames are built under the
  backend's lock and sent after it, by whichever task changed the call). The answer to this
  page's newest press is applied too (`followMuteAnswer`, § 13 note 19).
- New reactive `callMicrophone: 'live' | 'muted' | null`, from the TRACK.
- Non-owner pages: `setCallMuted(false)` is never offered (§ 7). **As implemented in M1:** it is
  drawn disabled at its size, and the reason is said in flow on a line of its own under the
  stage's header that wraps ("This window does not carry the call's audio, so it cannot unmute
  you.", `call-unmute-refusal`), in the folded window's status line, and in the control's
  accessible name. The sentence names no other window, because M1 cannot tell a window that holds
  the call from a reload that lost it; M8 refines it. A page still OPENING its microphone for the
  call it started (`callStarting`) is not a non-owner: its press is kept and applied at adoption.

### Other web files

| File | Change |
| --- | --- |
| `web/src/lib/call-media.ts` | `CallMedia` gains `readonly muted: boolean`. `simulatedCallMedia.setMuted` (`:1284`, a no-op today) disables the simulated tracks and records the state. |
| `web/src/lib/ws-client.ts` | `callPrepare` (`:1039`) and `callAccept` (`:1077`) carry `media_owner`. `callAccept` returns `{call_id, outcome}`. New `callProtocolSet` goes through `writeRequest`. |
| `web/src/lib/call-stage.ts` | `callStageTitle` (`:154`) prefers `call.subject` for a group or meeting. |
| `web/src/components/call-bar.tsx` | `RingingCard` (`:99`) copy per § 7. Answer is hidden while `!can_accept`, and shows `accept_blocked_reason`. |
| `web/src/components/call-stage.tsx` | `data-microphone` on the root, and the `elsewhere` / `lost` states per § 7. |
| `web/src/components/calling-protocol-settings.tsx` (new) | The three rows, in Settings › This app. |
| `web/mock/server.ts` | See § 8. |
| `web/scripts/join-live.ts` | `--pair --toggle-mute`: a MODE of the existing driver, with the same pinned meeting and proofs. (It was `--mute`; renamed because media-robustness' `--muted`, one letter away, means "join muted", README § 3 collision 4.) The sender presses Mute 10 s after connecting and Unmute 10 s later. The receiver's digest prints, per remote endpoint, `endpointState.state.isMuted` (booleans only). `--hold` is honoured under `--pair` as well. |
| `web/scripts/preview.ts` | The `--call` and `--maintenance` captures of § 8. |

### Docs

`NATIVE-CALLING.md` and `AGENTS.md` are covered in § 12 under docs to update. `CLAUDE.md` is a
symlink to `AGENTS.md`.

## 6. Gating and consent

### Methods

| Method | Class | What changes |
| --- | --- | --- |
| `call_accept` | OUTWARD (unchanged) | Posts the WV attach and acceptance (M4a), or only the acceptance on an attached leg (M4b). A second press is an outcome that posts nothing. |
| `call_hangup` | OUTWARD (unchanged) | For an attached call, the door is DELETE `callLeg` (decline) or `leave` (hangup). Changing the door of the user's own press needs no new consent. |
| `call_mute` | OUTWARD (unchanged) | Behind `mute = endpoint-state`, it publishes the same fact (whether the user can be heard) to the client's door. M9's self-unmute asks the meeting to let the user be heard, on their own press. |
| `call_prepare` | MACHINE (unchanged) | Accepts `media_owner`, a non-secret label. |
| `call_protocol_set` | **MACHINE (new)** | Decides how this machine's calls speak to Teams, so it needs the write token and is refused read-only. Named in both guard alternations. The mock mirrors its validation. |

### No environment switch

The round-1 env switches are replaced by the stored, per-backend `calling_protocol:<port>`.
- An agent cannot write a stored setting: it needs the write token.
- A systemd drop-in is different. The PostToolUse auto-update re-stages the service and
  `try-restart`s it (`bin/teams-lite-service.sh:885-886`), which would pick a drop-in up with no
  user step.
- It keeps the A/B: 19422 can ring-attach while 19420 stays on the measured flow.
- It applies with no restart, from the next call.

### Automatic acts

Automatic acts are POSTs with no click behind them. Each refuses on `read_only()` BEFORE the
network, the rule `deliver_push` and `publish_presence` follow. A read-only backend also never
registers (`calling_available() = !read_only()`, `server.rs:14415`; `spawn_calling` returns early,
`:15662`), so it never receives an invite.

- **The ring-time attach and join (M4b only).** This is the first calling POST no click triggers.
  It needs the USER's explicit approval before it can become a default (§ 12). Until then it runs
  only where the user turned it on, for the kinds they turned it on for. The argument for
  approving it:
  1. It is the request every Teams client makes on every ring. The service already receives one
     per ringing device.
  2. It claims this endpoint's forked leg and accepts NOTHING. Measured 2026-09-04: even an attach
     carrying an SDP left the caller ringing, because the acceptance is what answers.
  3. It carries no SDP, no `sender` and no `acceptedCallModalities`, and opens no microphone.
  4. It publishes nothing the registration did not: the service forked the invite here because
     this endpoint is registered.
  5. No `callProgress` is sent.

  Its UNKNOWNS are stated, not argued away. Whether the join shows the user in a conversation or
  meeting roster before they answer is unmeasured: the round-1 "roster-only join" reassurance
  rested on a misattributed request name (§ 4.2). Whether the attach silences the user's other
  devices is also unmeasured. M4b cases (c) and (h) measure both, and the setting turns it off.
- **The keep-alives (M6).** They belong to a call the user already placed, answered or joined. They
  publish nothing new, and run only while `Connected` and only with `keepalive = client`.
- **The mute re-send on reconnect (M1).** It delivers the user's own press that failed to arrive,
  once, for the same call. It is not a decision.
- **Deliberately NOT automatic:**
  - a decline or a leave when a ring ends elsewhere, or when the local ring timer fires;
  - hanging up when the owner page is gone;
  - re-opening a microphone when an organizer releases a mute.

### Microphone rule (M1, M8, M9)

A microphone opens only from the window that holds it, on that window's own press. A frame can
close a track, never open one. Other windows may MUTE; they may not UNMUTE (§ 7).

### Live verification

Live verification uses only:
- `join-live` (including `--from-chat`, `--pair`, the new `--pair --toggle-mute` MODE, and `--hold`);
- `call-live`, with the colleague's agreement for that session;
- `examples/meeting_join_probe.rs`;
- the user's own click with a consenting colleague.

No script is hand-rolled, and the browser MCP tools only look.

## 7. UI and copy (including mobile)

**Ringing card** (`call-bar.tsx`):

| Call | Title | Subtitle | Accept button |
| --- | --- | --- | --- |
| 1:1 | the caller | "Incoming call" (unchanged) | **Answer** |
| Group | the caller | "Group call in {subject}", or "Group call" | **Answer**, aria-label "Answer the group call in {subject}" |
| Meeting invitation | the caller | "Asking you to join {subject}", or "Asking you to join a meeting" | **Join**, aria-label "Join {subject} with your microphone"; the click opens the microphone to everybody in that meeting |

- Decline is always drawn.
- While `!can_accept`, Answer is replaced by its reason (`accept_blocked_reason`), e.g. "This call
  could not be claimed here. Answer it on another device."
- After answering a group or meeting, the stage title is the subject, falling back to the caller.

**A ring that ended elsewhere** (M4b delivers it; M2 makes the copy reachable): the card leaves at
once and ONE notice says why.

| Ending | Notice |
| --- | --- |
| 450 by self | "You answered on another device." |
| 450 by someone else (a delegate, a pickup) | "{Name} answered this call." |
| 451 | "The call was forwarded." |
| 452 | "The call went to voicemail." |
| 487, and the local timeout on an INCOMING ring | "Missed call from {Name}." |
| the local timeout on an OUTGOING ring | "{Name} did not answer." |

The user's own Decline still says nothing. `ringing-alerts-history` may replace these notices with
its missed-call surface.

**Answer pressed in a second window:** "This call was answered in another window." Nothing is
declined.

**Mute follows the call** (M1):
- every window's Mute silences the microphone that is live;
- the owner window's button follows its TRACK;
- a failed mute stays muted;
- a window that does not carry the audio may mute and may not unmute: its Unmute is disabled and
  the stage's subtitle says why (§ 5, `store.ts`);
- server-muted presentation is `meeting-awareness` § 4.6.

**Several windows** (M8):

| State | What the window shows |
| --- | --- |
| Holds the media | The stage as today. |
| Elsewhere | Subtitle "In this call in another window". Controls: **Mute** (title "Mutes the microphone in the window that holds it"), People, Chat, Fold and Leave. Camera, Share and Record are hidden, because they need the local capture. Unmute is drawn disabled with "Unmute in the window that holds the call". |
| Lost (proven by § 4.15) | "This window lost the call's audio when it reloaded. Leave the call, then call back." with Leave. |
| Unknown (an older backend, or the duplicate check unresolved) | Today's rendering. |

**Settings › This app › Calling protocol (experimental)** (`calling-protocol-settings.tsx`):
- **Incoming calls:**
  - "Measured (default)"
  - "Join the call's conversation when you answer"
  - "Claim the call when it starts ringing". Note under it: "This window's backend makes the request
    every Teams client makes on each ring: it claims the call for this device, answers nothing and
    opens no microphone. A ring that ends elsewhere then goes away at once, and Decline uses
    Teams' own door." It applies to the kinds ticked (one-to-one only at first).
- **Keep-alives:** "Off (default)" or "Send them the way Teams does".
- **Mute:** "Measured (default)" or "Publish it the way Teams does".
- Under the rows: "Applies to calls this window's backend starts from now on. Other windows on this
  machine keep their own." Each row clears the 44 px floor.

**Mobile:**
- The ringing card keeps its full-width bottom layout.
- The elsewhere and lost states REMOVE three header controls, which relieves the known phone-header
  overflow.
- Notices use the existing `--notice-inset-bottom` reservation, so they never cover Answer.
- The disabled Unmute keeps its size, and no new control is added below 44 px.

## 8. Tests, mock and captures

**Rust unit tests (`src/calling.rs`):**
- `session_of` on callback URLs, with a query, and with no segment.
- `ring_attach_payload` key for key: no `mediaContent`/`sender`/`acceptedCallModalities`;
  `requireMediaContent == !offer_present`; capabilities `73447` (bit 16 clear).
- `attached_leg` reads `callInvitation.links.*`, the offer and the join output, and drops a link on
  our own surl.
- `ConversationLeg` reads top-level links only, replaces wholesale, and applies the sequence gate.
- Every new body equals its client shape (`mM`, `uG` with the sequence from 1, `NV` without async
  links, `pM(this,!1)`, `vG` with the 9 links).
- `keep_alive_interval_ms` for both locations, the floor, and absence.
  `conversation_keep_alive_interval_ms` stays within [2 160 000, 2 430 000].
- `call_ended_from_frame`:
  - a bare body only on our end paths, at `/_decoded` and at the root;
  - the `callControllerTransactionEnd` override;
  - `acceptedElsewhereBy` at both levels.
- `end_reason_for`, the invite's `_decoded` siblings, and `CallProtocol` parsing (an unknown value
  reads as the default).
- `no_body_carries_the_sdk_request_envelope` (`:3362`) gains `ring_attach_payload`,
  `decline_payload`, `endpoint_state_payload`, `self_unmute_payload` and both keep-alive payloads.

**Rust source scans (`src/bin/server.rs`):** as listed in § 5, including:
- `read_only()` precedes the transport call (`post_signal` / `delete_signal`, plane-reliability
  M4's one API) in `attach_at_ring`, its deferred-decline branch and both keep-alives;
- `call_mute` never names `links.mute()` under `EndpointState`.

**Web unit tests** (`call.test.ts`, `call-stage.test.ts`):
- the `microphoneShouldBeOpen` truth table, including:
  - "a failed mute stays muted when a later frame says unmuted";
  - "a stale frame does not re-mute an unmute just pressed";
  - "a frame never opens a microphone with no own press";
- `callMediaPlacement` for all four states, including a duplicate tab;
- `callEndLabel` for every new reason, by self and other, incoming and outgoing;
- `callPhaseLabel` for ringing Group and Meeting;
- `callStageTitle` preferring the subject.

**Mock (`web/mock/server.ts`), mirroring every refusal and state:**
- `call_accept` (`:10432`) returns `outcome: "answered_elsewhere"` when the call is no longer
  ringing or is being accepted; it never throws for that.
- `call_prepare` (`:10213`) validates and records `media_owner`. `MockCall` carries `media_owner`,
  `subject`, `mute_seq`, `ended_elsewhere`, `accept_blocked_reason` and `protocol`.
- `call_mute` (`:10557`) bumps `mute_seq`.
- `call_protocol_set` validates like the backend, and `call_status` reports `protocol`.
- Hooks:
  - `{kind:"call_end", code, accepted_elsewhere_by}` (extending `:13348`) maps codes exactly as
    `end_reason_for` does;
  - `{kind:"call_invite", call_kind:"group"|"meeting", subject, no_offer:true}` (extending `:13420`);
  - `{kind:"call_invite", busy:true}` for a second invite while one is live;
  - `{kind:"call_mute", hold_ms, fail:true}` holds or fails the next `call_mute`;
  - `{kind:"call_attach", outcome:"unknown"}` puts the ringing call into the blocked state;
  - `{kind:"call_rpcs"}` returns the call RPCs received since the last reset.
- `call_invite {reset:true}` clears all of it. A spec MUST call it.

**E2E** (new `web/e2e/call-answering.spec.ts`, two pages in one context where needed; every test
ends with the reset):
1. A ring answered on another device goes at once and says who, by self and by another.
2. A ring the caller gave up on says it was missed, and an outgoing call nobody answered says they
   did not.
3. A second window mutes the microphone the first one holds: page A answers, page B presses Mute,
   and A reads `data-microphone="muted"`.
4. A failed mute stays muted (`call_mute fail`, then a roster-shaped `call_state`). The window that
   holds the call does not unmute on a stale frame either (`hold_ms`).
5. A second window shows what it can do from there: no camera, share or record; Mute and Leave
   work; Unmute is disabled.
6. Two windows cannot answer one call twice. Count `call_hangup` RPCs (expect 0 from page B), and
   page A stays connected.
7. A reloaded window says it lost the audio. A duplicated tab does not.
8. A group call and a meeting invitation ring as what they are; the meeting button says Join.
9. A ring this window could not claim offers no Answer and says why.

The existing "answers a ringing call, mutes, and hangs up" and "declines…" specs in
`calling.spec.ts` stay green unchanged.

**As implemented (M1):** specs 3 and 4 live in `web/e2e/call-windows.spec.ts` — the file for the
several-windows specs (3–7) — beside the ones that pin § 13 notes 18 and 19: a refused and a
half-answered Mute in a second window, Mute then Unmute inside one round trip, a failed Unmute
taking itself back, a Mute pressed while the holder could not hear, a lost Mute sent again once
and never twice and never over a press made since, and the window still opening its microphone.
Every spec about something that must NOT move the microphone records each value `data-microphone`
takes, because a microphone opened for one frame and closed again passes a final-state assertion.
The mock records presses one at a time; its hooks are `{kind:"call_mute", fail|hold_ms|lose_answer}`,
`{kind:"call_mute", offline}`, `{kind:"call_state"}` (a frame changed in nothing) and `mutes` on
`{kind:"call_record"}`.

**As implemented (M2):** specs 1 and 2 live in `web/e2e/call-answering.spec.ts`, with a third
pinning that a call answered HERE is never said to have ended elsewhere, and a fourth that a
meeting being JOINED (outgoing, `connecting`, held on the wire) is never said to be missed. The
mock's hooks are
`{kind:"call_end", code, accepted_elsewhere_by}` (read by `mockServiceEnding`, the backend's
`end_reason_for` re-spelled for the other install the mock stands for) and `{kind:"call_place",
nobody_answers:true}`, which ends the next placed call as `CallEndReasonUnanswered` once its
`call_place` has answered. The `--call` capture gained
`-notice-{answered-elsewhere,missed}-{light,card-light,card-dark}`; it is what found § 13 note 20.

**Captures:**
- `cd web && bun run preview -- --out /tmp/call --call` gains `-ring-group`, `-ring-meeting`,
  `-ring-unclaimable`, `-notice-answered-elsewhere`, `-notice-missed`, `-stage-elsewhere-{light,dark}`
  (second page) and `-stage-lost`.
- `--maintenance` gains `-calling-protocol-{light,dark}`.

**Gates:**
- `cargo test`.
- `cd web && bun run test && bun run typecheck && bun run test:e2e`, with explicit free ports
  (`E2E_MOCK_PORT=19467 E2E_WEB_PORT=19468`) and freed listeners.
- `python3 .claude/hooks/guard-live-automation.test.py`, because the guard changes.

## 9. Live verification plan

**Step 0 for anything run on 19442:** the user presses the in-app update to a release carrying the
milestone, and Settings › This app shows the expected build. 19442 is the RELEASED binary. 19420
re-stages from master by itself.

| # | Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- | --- |
| M0 | Regression on the verified paths, after M0 merges. | `cd web && bun run join-live -- --from-chat --hold 60` (the pinned meeting). `bun run call-live` only with the colleague's agreement for that session. | The agent runs them. | Connected, held, hung up cleanly. No `[calling] ignored a frame on` line is journaled while the call is up (the line names no session, so this is the check that can be run). Both calls end on the service's own end frame, as before. |
| M1 | During one of the user's own calls, with the laptop (holding the microphone) and the phone on the same backend, press Mute on the PHONE, then Unmute on the laptop. | The user's own click. E2E specs 3–5 are the automated proof. | The user, with the colleague. | The colleague hears silence the moment the phone shows Muted. The laptop's Mute turns on by itself. The phone offers no Unmute. The laptop's Unmute is heard. |
| M2 | Instrumentation: one `call-live` (outgoing) and one incoming call from the colleague. | `call-live` with agreement; the colleague places the incoming call. Journals are read with `journalctl --user -u teams-lite-backend.service`. | The agent; the colleague. | Shape lines: the invite's `conversationInvitation` keys and `evt`; `callKeepAliveInterval` from `callAcceptance` (outgoing) or the acceptance's inline acknowledgement (incoming legacy); whether `leave`/`updateEndpointState`/`notificationLinks`/`unmute` are in each conversation object. No behaviour changes. |
| M3 | The user sets Mute = "Publish it the way Teams does" on 19442. Both installs join the pinned meeting; the sender mutes, then unmutes. | `cd web && bun run join-live -- --pair --toggle-mute` (19442 sends, 19440 watches). | The agent, after the USER has started both installs. | The receiver's digest shows the sender endpoint's `isMuted` going false → true → false in step. The sender's journal shows `published the mute on updateEndpointState → 200` twice, and no `mute`-link POST. |
| M4a | The user sets Incoming = "Join the call's conversation when you answer" (and Mute as in M3) on 19442. The colleague calls the user from real Teams; the user answers on 19442, mutes, unmutes, then presses End. | The user's own click and the consenting colleague. No driver can RECEIVE a call. | The user and the colleague. | The journal shows `attached (WV) → links=[…acceptance…]; joined the conversation: links=[…leave…updateEndpointState…]`, then `accepted`, then `endpoint state isMuted=true → 200`, and finally `left the conversation → 200`. The colleague sees the user muted, then not, and their call ENDS when End is pressed. That closes § 8a "HANGING UP MAY NOT REACH THE CALLER". A decline on the legacy flow is pressed once too, and its answer journaled. |
| M4b | The user sets Incoming = "Claim the call when it starts ringing", one-to-one only, on 19442; 19420 stays measured. The colleague calls the user from real Teams. Cases: **(a)** answer on 19442, talk, End; **(b)** Decline on 19442; **(c)** answer on the Teams phone app while 19440 and 19442 both ring; **(d)** the colleague hangs up before anyone answers; **(e)** after (b) and (d), the agent runs `join-live --from-chat --hold 120`; **(f)** optional: during (c)'s phone call the agent runs `join-live --from-chat --hold 120` and the user hangs up the phone call meanwhile; **(g)** optional, the user's own setup: Teams quit on the phone and 19420 stopped, for one ring; **(h)** the colleague watches their own client during every ring. | The user's own click and the colleague; `join-live` for (e) and (f). | The user, the colleague, the agent for `join-live`. | **(a)** `attached at ring time … joined: links=[…]` BEFORE Answer, then `accepted`, then `left → 200`; the colleague's call ends on End. **(b)** `declined on the leg (DELETE) → 2xx`; the colleague's ring stops at once. **(c)** 19442's card vanishes within about 2 s with "You answered on another device."; 19440 rings to its 60 s backstop: the A/B in one call. **(d)** 19442's card vanishes within about 2 s with "Missed call from {Name}."; 19440 at 60 s. **(e)/(f)** the meeting is untouched; any frame from the earlier ring's conversation is journaled as `ignored`. **(g)** the caller's client says "Ringing" (not "Calling…"), or callProgress is revisited. **(h)** the colleague sees nothing unusual during the ring: no "joined", ringing as always. |
| M5 | Promotion: repeat M4a on the default flow after `join-at-answer` becomes the default. Promote ring-time for 1:1 only after the user's decision (§ 12), then repeat (a)–(d). | The user's own click. | The user and the colleague. | The same signals with no setting changed, and `call-live` still rings, connects and hangs up for both sides. |
| M6 | Baseline, then the user sets Keep-alives = "Send them the way Teams does" and the hold is repeated. Then a 30-minute call with the colleague. | `cd web && bun run join-live -- --from-chat --hold 3000` (baseline and after); `cd web && bun run call-live -- --hold 1800` (the colleague agrees to 30 minutes in advance). | The agent runs the drivers; the user flips the setting and asks the colleague. | Baseline: the journal shows whether and when the service ends the meeting, with its phrase. After: `call keep-alive → 2xx` every 0.9× the interval, `conversation keep-alive → 2xx` at 36–40 min, and no service-initiated end. A solo meeting may be ended for being empty: the phrase says which. If so, repeat with `--pair --hold 3000`. |
| M7 | A colleague, inside a meeting, calls the user in ("Request to join"); separately, a colleague calls a group chat the user is in. M4b's per-kind setting stays one-to-one only, so these exercise the answer-time join. | The user's own click and consenting colleagues. | The user and the colleagues. | The card reads "Asking you to join {subject}" with **Join**, or "Group call in {subject}". The stage title is the subject, and the roster names the others. Leave removes the user (the colleagues confirm). The journal records `evt` 107/109 and the `conversationInvitation` keys. Only after this passes may the user tick Group or Meeting for ring-time attach. |
| M8 | During one of the user's own calls, open the app in a second window on the laptop, then reload the window that holds the call. | The user's own click. E2E specs 5–7. | The user. | The second window says "In this call in another window" and offers Mute but not Unmute; its Mute silences the first window. After the reload, the reloaded window says it lost the audio, and Leave ends the call for the colleague. |
| M9 | An organizer (a consenting colleague) soft-mutes the user in a meeting, then the user presses Unmute. Needs meeting-awareness M5. | The user's own click. | The user and the organizer. | The journal shows `self-unmute requested on unmute → 2xx` and then a roster clearing `serverMuted`. The track opens only then. A mute the organizer lifts without being asked leaves the user muted with the meeting-awareness notice. |

## 10. Milestones

Each milestone ships alone.

| ID | Milestone | Effort | Risk | Depends on |
| --- | --- | --- | --- | --- |
| M0 | **A frame acts only on its own call**, plus the calling-plane example guard: § 4.0, the guard category, the documented `handle_call_frame` branch order with its source-scan test (README § 3, collision 4), tests, M0 regression run. | S | Low. A frame for another session is ignored and journaled. Frames with no session segment keep today's handling until the journal proves them inert. | — (`plane-reliability` and `video-receive` benefit; every workstream that adds a branch adds it to M0's order) |
| M1 | **The microphone obeys the call**: `mute_seq`, `pendingMute`, `microphoneShouldBeOpen`, the reconnect re-send, simulated mute, E2E 3–4. | S–M | Low. A frame only ever closes a track; a failed mute stays muted. | — (`meeting-awareness` M6 composes with rule 1) |
| M2 | **Read what the service already says**: invite fields journaled, the § 4.12 end parse and the named endings with labels (direction-aware), the ack, conversation-link and transfer/replacement shape journals, mock `call_end` codes, E2E 1–2. | S | Very low. Reads and labels only. | — (`ringing-alerts-history` consumes the named endings) |
| M3 | **Calling protocol switches and the client's self-mute publish**: `call_protocol_set` (guard, mock, Settings section), `ConversationLeg` with the sequence gate, the `updateEndpointState` publish under `mute = endpoint-state`, `join-live --pair --toggle-mute`. | S–M | Low to medium. Off by default; the local mute never depends on the publish. | `meeting-awareness` M1 (`conversation_links`; whoever lands first builds it) |
| M4a | **Incoming calls join their conversation when answered** (M4a, `incoming = join-at-answer`): the use of plane-reliability's transport and its `SignalRefused`, the WV attach at answer with the 4xx-only fallback, the claim and typed outcomes, the inline ack, `leave` for attached calls, endpoint state at accept. | M | Medium. It changes the measured attach body, so it is opt-in, falls back only on a definitive refusal, and keeps the acceptance unchanged. | M0, M3; `plane-reliability` M4 (the one transport API, README § 3 collision 7; it lands first); `plane-reliability` › `answer-failure-declines` (the general catch; not blocking) |
| M4b | **Incoming calls are claimed when they ring** (M4b, `incoming = ring`, one-to-one only): `attach_at_ring` with its local wait on a detached `Create`-class request and its tri-state, ringing guards, DELETE decline, decline-while-attaching, ring-ended-elsewhere delivery, the blocked-Answer state, E2E 9. | M | Medium-high. It is the second automatic calling act, and the roster visibility and other-device effects are unknown. Opt-in, per kind; the legacy flow stays. | M0, M2, M4a (and so `plane-reliability` M4); the user's approval (§ 12) before it becomes a default |
| M5 | **Promote what passed**: `join-at-answer` becomes the default after M4a; ring-time per kind only after the user's decision and each kind's matrix. The legacy attach and the `callLeg` fallback stay until every kind ring-time covers has passed. A controller-less invite always keeps the measured answer-time flow. | S | Low once measured. | M4a, M4b, M7; docs |
| M6 | **Long calls and meetings stay up**: `CALL_TICK` (which also carries the 60 s ring-expiry sweep; ringing, advanced and plane-reliability M10 then cite it), the call and conversation keep-alives under `keepalive = client`, the dead `keepAlive` link removed, and the baseline before promotion. The keep-alives are an automatic act the user approves in README § 5, D2. | M | Medium. It adds periodic POSTs to verified calls, so it is opt-in and promoted only on data. | M3 (`ConversationLeg`), M4a (the incoming interval) |
| M7 | **Group calls and meeting invitations ring as what they are**: kind and subject, copy including Join, the stage title, mock and E2E 8. The copy reads video-receive's one `is_one_to_one()` predicate; the answering branch's `one_to_one` comes from that predicate, not from a `kind` returned here. | M | Medium-low. An unknown invite shape stays a 1:1 in the copy, and is not one-to-one for the predicate. | M2, M4a; `video-receive` M1 (the predicate, README § 3 collision 6); `video-send` (`one_to_one` use) |
| M8 | **One call, several windows**: `pageId` with the duplicate check (sent as `page`), `callMediaPlacement` over media-robustness M7's `media_owner` / `media` state, the elsewhere and lost states, the owner-only Unmute, mock, E2E 5–7. | M | Low. Local rendering and a non-secret label; an older backend renders as today. | M1, M4a (typed outcomes); `media-robustness-devices` M7 (the one ownership mechanism, README § 3 collision 2) |
| M9 | **Unmute yourself when an organizer soft-muted you**: `self_unmute_payload` and the ONE server-muted `call_mute` branch, under `mute = endpoint-state`. | S | Medium. Unmeasured, and waits for a real capture. | `meeting-awareness` M5 (`server_muted`) and § 9 step 6; M3. `meeting-awareness` M6 (enforcement) lands after this |

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| The verified incoming answer (2026-09-04) regresses. | Every incoming change is opt-in per backend. The acceptance body is unchanged. M4a falls back to the measured attach only on a definitive 4xx; a timeout ends the call as today and never double-attaches. The two installs give an A/B in one call. |
| A frame of another conversation rewrites or ends the live call. This is new exposure once rings join conversations, and it is latent today. | M0 routes by session id first, with a test that another session's `conversationEnd` cannot end a live call, plus M4b cases (e) and (f). |
| A second window's Answer DECLINES the user's own call. | The claim refusal is an `Ok` outcome; the page releases only its own media. Spec 6 counts `call_hangup` RPCs. |
| A failed or stale mute unmutes a user who pressed Mute: the privacy failure. | `pendingMute` with `mute_seq`; a frame never opens a microphone; a failed RPC keeps the mute and re-sends once on reconnect. The truth table and spec 4 pin it. |
| Another window opens a microphone on an unattended laptop. | Non-owner windows may only mute. The rule is written into AGENTS.md § Audio calls. |
| A slow ring-time attach races the user's Answer into a double attach. | `ATTACH_SETTLE_WAIT (20 s) > ATTACH_TIMEOUT (15 s, a local wait)`. `Unknown` never re-attaches: Answer is withdrawn with a reason and the ring is left alone, until the detached request's late answer lands as the claim. |
| A local give-up throws away a leg the service created. | The request is never aborted at 15 s: it runs on in plane-reliability's `Create` class (65 s), detached, and a late answer reaches the claim or `hang_up_orphan` (README § 3, collision 7). |
| The ring-time join makes the user visible in a meeting roster before they answer, or silences their other devices. | Ring-time is one-to-one only until M7. M4b cases (c) and (h) measure both, and the setting turns it off. Promotion is the user's decision. |
| Moving the outgoing or meeting hangup off its measured door. | `leave` is used only for attached incoming calls. Outgoing calls and meetings keep `Links::hangup`; `an_outgoing_call_keeps_its_own_hangup_door` pins it. |
| A blanket signaling timeout ends calls the service accepted. | The policy belongs to `plane-reliability` M4, and its rule is the mitigation: a bound aborts only the HTTP request, and every request whose answer matters runs detached, so a late answer still lands. This workstream adds no transport of its own (README § 3, collision 7). |
| Tooling switches an unproven flow on. | A stored per-backend setting behind the write token, never an environment variable. The RPC is named in the guard. |
| A cargo example drives the new outward calling functions directly. | The new guard category refuses any example naming them unless allowlisted. |
| Keep-alives disturb verified calls (a wrong body, a flood). | Opt-in; the interval comes from the service with a 15 s floor; nothing is sent when absent; sending stops on 404. |
| The incoming broker capability is claimed but not served. | Bit 16 is cleared on the incoming attach, and broker-shaped answer keys are journaled. |
| The server mute is misdetected. | No refusal is built on it. Detection belongs to meeting-awareness (behind its switch), and this workstream acts on it only in M9 (§ 4.9), after a capture. |
| `crypto.randomUUID` breaks the app on insecure origins; a duplicated tab reads as lost. | `getRandomValues` plus the BroadcastChannel duplicate check. "Lost" is drawn only when proven. |

## 12. Open questions and decisions the user must make

**Decisions:**
1. **Approve the ring-time attach and join as a second automatic calling act** (beside the
   registration), before M4b can become a default for any kind. The opt-in itself is the user's
   click in Settings. If approved, AGENTS.md § Audio calls' "nothing in this feature ever acts on
   its own" is amended to name it. If declined, M4a still fixes the hangup, mute and keep-alive,
   and only the decline door and the ring-ended-elsewhere notice are lost.
2. **A stored per-backend setting** in Settings › This app (chosen here), against environment
   switches in a unit drop-in. `meeting-awareness` M6's server-mute enforcement now uses the same
   stored setting (`server_mute` in `call_protocol_set`) instead of `TEAMS_LITE_SERVER_MUTE=enforce`
   (README § 3, collision 10); the rest of the choice is README § 5, D9.
3. When a ring that joined its conversation ends without our answer (declined, expired, or answered
   elsewhere), should teams-lite also POST `leave` on that conversation? The client does not.
4. Should the backend hang up a call whose media-owning page has been gone for N seconds? It is an
   outward act with no click, but it avoids a phantom participant.
5. Confirm that other windows may only MUTE. The alternative is a remote Unmute with a confirmation
   naming the device whose microphone opens.
6. Confirm that an organizer lifting a mute the user did not ask to lift leaves the user muted. The
   client unmutes them automatically.
7. If the M6 baseline shows meetings and calls survive without keep-alives, keep them opt-in or
   promote them anyway? Either way the keep-alives are periodic POSTs to a live call that nobody
   pressed, so they are listed for approval with the other automatic acts in README § 5, D2: they
   publish nothing new (an empty `callParticipantUpdate`, and the 9-link set the join already
   published) and only keep a call the user is in from being dropped.
8. Who is the consenting colleague for M4a, M4b and M7, and do they agree, per session, to
   `call-live` runs?

**Open questions:**
- What does `/additionalActionResponses/0/output` look like on this tenant? (M2/M4a)
- Is the ring-time join visible to others in a 1:1, a group, or a meeting? (M4b cases c/h, M7)
- What is the `callKeepAliveInterval` value? Does the service drop a call without the call
  keep-alive, the conversation keep-alive, or both? (M2, M6)
- Which code does an attached endpoint receive when the caller cancels (487 per the table), and
  when the user declines on another device (603, or 452 when voicemail picks up)?
- Is prod's `sendProgressFromCC` on? (M4b case g)
- Does `/conversation/localParticipantUpdate/` carry the self server-mute, or only the roster?
  (meeting-awareness § 9 step 6)
- Does the service keep posting to a declined or expired ring's conversation? (M4b journal)
- Is the invite's own `callNotification.links.reject` a valid pre-attach decline door? It matters
  only for the legacy and `Unknown` paths.
- What does the service do with a transfer the transferee never answers? (for `advanced-call-control`)

**Docs to update when each milestone lands:**
- **AGENTS.md § Audio calls:**
  - the automatic-acts rule (decision 1);
  - the attached-call decline door (DELETE `callLeg`) and hangup door (`leave`);
  - "a microphone opens only from the window that holds it";
  - "mute follows the call";
  - a new subsection "One call, several windows";
  - the protocol setting.
  And replace two bullets that become false: "Taking an INCOMING call is THREE POSTS on this
  tenant" (it becomes attach-with-join, then accept) and "A ring nobody answers is never closed by
  the service" (true only of an unattached ring).
- **AGENTS.md § A call is a PAGE:** the elsewhere and lost states.
- **AGENTS.md § Video in a meeting:** one line, that an incoming 1:1 now carries a conversation
  roster like an outgoing one.
- **AGENTS.md § Running the released build BESIDE the staged one:** the protocol setting is per
  backend, so the two installs may differ.
- **NATIVE-CALLING.md:**
  - § 2.4 rewritten (attach-with-join, then accept);
  - § 6: the keep-alive paragraph and the RPC table (`call_protocol_set`);
  - § 7: consent amended;
  - § 8a "Still open": moved to measured after M4a/M4b;
  - a new subsection "The conversation leg" (doors, the endpointState mute, the self-unmute);
  - the round-1 corrections recorded.
- **Doc comments in `src/bin/server.rs`:**
  - `expire_unanswered_call`, which says "holds no leg of ours";
  - `CALL_RING_TIMEOUT`;
  - `keep_call_alive`, which says "only where it asked for one";
  - `CallSession.muted`.
- **Doc comments in `src/calling.rs`:**
  - `Links::hangup`, where the `callLeg` fallback is legacy;
  - `attach_payload`, legacy flow only;
  - `mute_payload`, legacy flow only.
- **`web/src/lib/call.ts`:** the header comment.

## 13. Review notes

**What the reviewers changed** (each claim re-checked in the code or the bundle first):

1. **Blocker: a refused second Answer declined the call.** Confirmed: `answerCall`'s catch runs
   `hangUpCall` (`store.ts:3112-3117`), and `call_hangup` declines a `Ringing` incoming call
   (`server.rs:8378-8385`). The refusal is now an `Ok` outcome (§ 4.4), and spec 6 counts
   `call_hangup` RPCs.
2. **Blocker: frames are not matched to the call.** Confirmed (`server.rs:14682-14722`). The ring-time
   join would make it a live hazard. M0 adds session routing as a prerequisite.
3. **Failed and stale mutes.** Confirmed: a failed `call_mute` leaves backend `muted:false`, and the
   round-1 "widen when nothing is in flight" rule would have reopened the microphone. Replaced by
   `mute_seq` plus `pendingMute`: a frame never opens a microphone.
4. **Remote unmute.** Accepted: non-owner windows may only mute.
5. **The server-unmute deferral was a misreading.** Confirmed: the client posts `NV` to `links.unmute`
   (@1937795, @1240009), and `canUnmuteSelf` follows the link's presence (@1936223). The refusal is
   dropped and the self-unmute is M9.
6. **The self server-mute rule.** The citation is corrected to `handleSelfServerMutedState`
   (@1954977), and detection belongs to meeting-awareness. One reviewer said porting the cited
   remote test would "lock the user muted". The round-1 RULE text never read `isMuted`, only its
   citation did. The rule is corrected anyway.
7. **Ringing guards.** Confirmed: `call_accepted_in_frame` and the media answer move a ringing call
   (`server.rs:14788-14805`, `:14891-14907`). Added (§ 5).
8. **Timeouts.** Confirmed that no request has a timeout today (`server.rs:4818`). The timeout is
   now limited to the new automatic requests, and the general policy belongs to `plane-reliability`.
9. **The settle wait was shorter than the attach timeout, and the fallback ran after a timeout.**
   Accepted. 20 s > 15 s; the fallback runs only after a 4xx; `Unknown` withdraws Answer.
10. **The mute publish had no switch, and env switches can be flipped by tooling.** Confirmed (the
    auto-update `try-restart`, `bin/teams-lite-service.sh:885-886`). Both are resolved with one
    stored per-backend setting behind the write token.
11. **The consent citation was wrong.** Confirmed: `JOIN_CONVERSATION_WITHOUT_CALL_MODALITY` is the
    subscribe path (@1259555); the attach's join rides `SEND_CALL_ATTACH`. The citation is
    corrected, the "roster-only" claim is withdrawn, and the user must approve.
12. **Split M4.** Adopted: M4a at answer time fixes the hangup with nothing automatic. Ring-time is
    gated per kind, and the legacy flow is kept until every kind passes.
13. **`leave` was not measured on outgoing calls.** Confirmed: § 8b names no link. `leave` is
    scoped to attached incoming calls.
14. **`crypto.randomUUID` and duplicated tabs.** Accepted: `getRandomValues` plus the
    BroadcastChannel check.
15. **A claim at prepare could lock other windows out.** Accepted: the claim is at accept only, and
    `media_owner` is a label.
16. **Example guard.** Accepted with a change: an allowlist rather than target pinning (see below).
17. **Smaller fixes:**
    - the direction-aware missed-call label;
    - Join on meeting invitations;
    - the client's exact endpointState shape (sequence from 1), published at accept too;
    - the 9-link keep-alive divergence stated, and the jitter drawn once per call;
    - bit 16 cleared;
    - the callProgress divergence stated;
    - the bare-body read at `/_decoded` and the root with `contains`;
    - `CallingPlane.declined_while_attaching`;
    - `ended_elsewhere {by_self,name}`;
    - explicit-pointer `ConversationLeg`, shared with meeting-awareness;
    - `conversation` filled on place and join;
    - the acknowledgement read in both flows and as a frame;
    - broker keys journaled;
    - the mock hooks;
    - the envelope and read-only scans;
    - the AGENTS.md bullets that become false;
    - step 0 for 19442, and the colleague's per-session agreement.

18. **Implementing M1 found two holes in the pending-mute rule, both in the direction the design
    calls its top risk.** Clearing `pendingMute` only on the frame that AGREES with it kept a
    failed Unmute alive past a later Mute from another window: the next Unmute from anywhere was
    then read against it by rule 4 and opened this window's microphone. And re-sending on reconnect
    "if the call is still live and this page holds it" would have sent that lost Unmute over a
    Mute pressed on the phone during the outage. So a press is forgotten at the first NEWER frame,
    agreeing or not, and the re-send is conditioned on the fresh read's `mute_seq` not having moved
    past it. The cost is the safe direction only: this window's own press, racing another window's,
    can be lost and leave the microphone closed. Pinned by `call.test.ts` and
    `call-windows.spec.ts`.
19. **An independent review of the M1 implementation (four reviewers, a skeptic per dimension)
    confirmed sixteen findings, none of them a microphone opened; all are fixed.** The ones that
    changed the design: Mute then Unmute inside one round trip landed muted, because the frame
    for this window's own earlier press forgot the newer one — settled by applying the backend's
    answer to the newest press when nothing newer has been decided (`followMuteAnswer`); a failed
    Unmute left the holder's microphone live under every other window's "muted", with nothing
    there able to close it — so a failed Unmute takes itself back and only a failed Mute is ever
    re-sent; the re-send was decided off a read a round trip old — so it carries `if_seq` and the
    backend refuses it when another press was recorded since; rule 4 was never decisive, since
    the press itself opens the track — so it is gone, and a frame never opens a microphone at
    all. The rest: a second window's "nothing was muted" after a LOST answer (now said only on a
    known refusal), the refusal clipped to nothing in a phone's header (now its own wrapping
    line, plus the folded window and the accessible name), the window still opening its own
    microphone told it "does not carry the call's audio" (`callStarting`), the doc comment on
    `CallSession.muted`, a source scan matching one spelling, and four test or doc claims that
    nothing pinned (a press made while the holder could not hear, the once-only re-send, the
    stale-frame guard, the truth table's own comments) — each now pinned or corrected.

20. **Implementing M2 found a notice drawn in the wrong ink.** The capture of "You answered on
    another device." came out RED: sonner replaces a toast under the same id by merging into it,
    and a report was sent untyped, so it kept the `type: "error"` of the refused camera said a
    moment before. Every call notice shares one id (`CALL_NOTICE`), so any report following a
    failure inside its nine seconds was drawn as a failure. A report is now sent as `toast.info`
    (`web/src/lib/notice.ts`), pinned by `notice.test.ts`.

21. **An independent review of the M2 implementation (three reviewers, a skeptic per
    dimension) confirmed three findings; all are fixed.** No test pinned the DIRECTION half of
    `is_being_offered`: the only outgoing case was `Dialing`, which the phase already excludes,
    while a placed call after `callAcceptance` and a meeting join in its lobby are outgoing and
    `Connecting` with nothing connected — a 487 there would have read "Missed call". It is
    pinned now in Rust and against the mock (a join held on the wire). "{Name} did not answer."
    could be said about a call whose invite never left, because the bound counts from the
    reservation — fixed with `invite_sent`. And one E2E step asserted a "Missed call" notice
    while the identical one from the step before still stood, so it could not fail; the steps
    are reordered so a different sentence stands between them. Refused: canonicalising the
    acceptedElsewhereBy id before the store lookup (the app looks every frame's mri up as
    spelled, and nothing measured a different spelling).

**What was rejected or changed, and why:**
- **Hard-coding a target in every calling example.** Rejected: the sanctioned
  `meeting_join_probe.rs` pins its meeting by calendar SUBJECT
  (`examples/meeting_join_probe.rs:39`), so a thread-id rule would refuse it. An allowlist of
  tracked examples is the live-driver precedent and catches the same hazard.
- **`media_owner` "is in effect a weak authorization token".** The design keeps no authorization
  role for it. The double-answer refusal keys on `phase`/`accepting`, never on the id.
- **"End the ring with an honest sentence" after an attach timeout.** Changed. Ending the ring
  locally would hide a call the user can still take on their phone, so Answer is withdrawn with the
  reason and the ring is left to end on its own.
- **"Send callProgress, the client's likeliest behaviour".** Not adopted. Publishing "this device
  is ringing" has no measured need, because the user's other devices already report it. It is
  stated as a divergence, with case (g) to revisit.
- **A guard rule against `systemctl set-environment` and drop-ins.** Superseded: there is no
  environment switch left to protect.
- **Coordination note for meeting-awareness.** Their § 4.5 rule 6 is the client's REMOTE-participant
  test. For SELF the client uses `handleSelfServerMutedState`, which reads `stream`/`streamLobby`
  first and the `audio` and `audio-teleconferencing` streams after. That is worth adopting in their
  M5. Their § 4.6 rule 2 (the `call_mute` branch while server-muted) is implemented here as M9,
  over `ConversationLeg::unmute()`, and is the one such branch (README § 3, collision 10).
