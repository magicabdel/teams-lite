# Video receive: a colleague's screen and camera in a meeting, who is speaking, pin, zoom

**Workstream:** `video-receive` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

Today a meeting joined from teams-lite draws the tile for a colleague's shared screen and fills it
with nothing. `call_subscribe` is answered 200, and `getStats` reports `bytesReceived: 0`
(NATIVE-CALLING.md:1672-1687). This design brings the receive request into line with the real web
client, **one measured variable at a time**:

- the SOURCE REQUEST itself: what `streamMsid` and `fmtParams` carry, and in what order;
- WHEN it is sent: after the service acknowledges our media answer, and never while an offer of
  ours is pending;
- HOW the service's inbound description is rewritten: the SSRC binding. This one is conditional,
  scoped to one call site, and promoted only if a measurement shows it is needed.

Only once a picture really decodes do we build the parts that depend on one. Those are the
subscription lifecycle (switch, release with `-1`), a gallery of several cameras with a local
"Pin for me", and zoom, pan and full screen on a shared screen. Who is speaking (CSRC polling)
depends only on audio, so it can ship without the video fixes.

Five rules hold the whole design:

- it adds no new outward action;
- it changes no request body on the verified audio path, except one: dropping an unbacked
  `"Video"` from the renegotiation answer, which restores the exact body that was measured;
- every inbound-SDP rewrite sits behind a switch, applies only to a conference's renegotiation
  offers, and is proven in a real browser before any live run;
- every tile claims only what has really arrived;
- every person is keyed by the ENDPOINT that publishes, never by an mri.

## 2. Where we are today

All references were re-checked against this worktree (HEAD `c941850`) and the 2026-11-05 bundle.

| Area | Today | Where |
| --- | --- | --- |
| Source request body | `SourceRequest{mid, source_id, stream_msid: String, fmt_params: String}`. The modern body puts `{sourceId, streamMsid, fmtParams}` into `json!` and then `to_string()`. serde_json has only the `default` and `std` features (verified with `cargo tree -e features`), so the inner keys are serialized SORTED: `controlInfo` before `sequenceNumber`, and `fmtParams`, `sourceId`, `streamMsid`. | `src/calling.rs:1064-1113` |
| fmtParams | `DEFAULT_VIDEO_FMTP = "max-fs=8160;max-mbps=245000;max-fps=3000;profile-level-id=42C02A;packetization-mode=1"`, a STRING. The page never passes `fmt_params` (`ws-client.ts` sends it only if given, and `store.ts` never gives it). | `src/calling.rs:1056-1057`; `web/src/lib/ws-client.ts:1109-1123`; `web/src/lib/store.ts:2780-2785` |
| streamMsid | `RemoteVideo.streamMsid = stream.id` of `event.streams[0]`. | `web/src/lib/call-media.ts:925-940` |
| When it is sent | Straight after `call_answer_media` returns, which is before the service's `mediaAcknowledgement` and with no check on our own pending offer. | `web/src/lib/store.ts:2720-2742` |
| What it is sent for | Everything in `publishing` that is a screen or a camera, matched to an unused section by its sharing label. It is RE-SENT after every answered renegotiation, and is never sent on a roster change (`onCallState` sets state and nothing else). It is never released. | `web/src/lib/store.ts:2757-2800`, `2623-2637` |
| `call_subscribe` | A `MACHINE_METHODS` entry (write token, refused read-only). It validates no shape and does not check the source against the roster. Journal: `subscribed: source= on mid= seq= (modern=)`. | `src/bin/server.rs:8313-8360`, `456-520` |
| Answer modalities | The page always declares `["audio","Video","ScreenViewer"]`. `"Video"` was added in `5b355f0` (2026-09-04 21:45) and has never been measured. The 1:1 verified in `25989b0` (09:40 that day) and the 2026-08-06 meeting run both answered with `["audio","ScreenViewer"]`. The backend checks the NAMES only, although the comment above it says the check is about the body. | `web/src/lib/store.ts:2726-2737`; `src/bin/server.rs:7951-7966`, `10182-10199`; NATIVE-CALLING.md:630-633, 1623 |
| mediaAcknowledgement | Published on every `mediaAnswer` body (three call sites). Ack frames arrive (measured: `/call/mediaAcknowledgement/:2`). Their links are merged like every other frame's, and nothing waits on them. | `src/calling.rs:1357-1376`; `src/bin/server.rs:7846`, `7914`, `7981`, `14682-14690`; NATIVE-CALLING.md:744 |
| Which kind a call is | `CallKind::Call` covers EVERY incoming call, including an incoming GROUP call ("An invite that reaches us from a GROUP call is still named after its CALLER"). Only an outgoing call becomes `Group` (`ring.len() == 1`). Nothing reads the service's own `isMultiParty`. | `src/bin/server.rs:14984-14995`, `7559-7560`; `src/calling.rs:479-560` |
| Who is published | `publishing` groups by mri and filters STREAMS by endpoint (`s.endpoint_id != self.local.endpoint_id`), so the user's other device is in it. The page is given no endpoint id and no "this is you" flag. `others()` drops the user's whole mri. | `src/bin/server.rs:2328-2330`, `2364-2386`; `src/calling.rs:1938-1985` |
| Inbound SDP transform | `fromMsSdp` rewrites the profile, the candidates, `x-candidate-ipv6` and the extmap ids (made consistent across the bundle). It adds no `a=ssrc` and no `msid`. It is called at THREE sites: the incoming call's first offer, every answer to our own offers, and every service renegotiation. | `web/src/lib/ms-sdp.ts:566-627`; `web/src/lib/call-media.ts:342`, `994`, `1071` |
| Labels on the first offer | `learnLabels` runs only on the renegotiation path. The incoming call's first offer is applied at 342 with no labels learned, and its `track` events fire inside that `setRemoteDescription`. | `web/src/lib/call-media.ts:342`, `921-923`, `1069-1071` |
| Track handler | `const [remote] = event.streams; if (!remote) return;`. Audio goes to `RemoteAudio`, keyed by stream id; video goes to `RemoteVideoTracks`, keyed by mid. | `web/src/lib/call-media.ts:325-333`, `434-470`, `914-966` |
| Tiles | `callStageLayout` draws every remote video section that has a track. A tile goes only on the track's `ended`. The name comes from `callVideoNames[mid]`, set AFTER the subscribe RPC resolves. There is no picture state. | `web/src/lib/call-stage.ts:342-361`; `web/src/lib/call-media.ts:941-947`; `web/src/lib/store.ts:2785-2789` |
| People rows | Built from `call.others` plus a self row with `mri: ""`. The user's other endpoint has no row. | `web/src/lib/call-stage.ts:222-250` |
| Driver digest | `PEER_CONNECTION_PROBE` keeps only LOCAL descriptions. `SIGNAL_PROBE` keeps raw inbound strings with no timestamp. `readMediaStats` sums `inbound-rtp`, with no per-mid figure and no transport total. `summariseSdp` prints mid, label, x-ssrc-range and direction. Every calling frame reaches the page as `call_signal` (`handle_call_frame` emits each one), so the driver can see acks, csrcInfo and invites without any backend change. | `web/scripts/join-live.ts:197-249`, `859-922`, `1053-1069`; `src/bin/server.rs:14601-14608` |
| Mock | The offer has audio at mid 0, main-video at mid 1 and sharing at mid 3, with no x-ssrc-range and no x-source-streamid. `call_subscribe` checks almost nothing and records nothing. `simulatedCallMedia.connectionState` is already `"connected"`. | `web/mock/server.ts:4708-4780`, `10451-10466`, `10513-10521`; `web/src/lib/call-media.ts:1320-1322` |

## 3. Gaps covered, deferred and rejected

**Covered**

| Gap | How | Milestone |
| --- | --- | --- |
| `meeting-video-receive-source-request` | `streamMsid` becomes the section's xSourceStreamId. `fmtParams` becomes an array holding one object of the client's own default caps. The inner string is built in the client's key order. The request waits for the ack and for no pending offer of ours (bounded at 15 s). A release is `-1`. A source switch is the client's `-1` then the new source. | M1, M4 |
| `receive-sdp-ssrc-binding` | A cname-only `a=ssrc` per service-sending VIDEO section. It applies at the renegotiation-offer call site only, in a conference only, and sits behind a switch. A browser spec proves it first, parameterised by the shapes LV0 records. It is promoted only if the measurement shows RTP arriving and being dropped. Each tile also gets a stream of its own, per mid. | M2 |
| `video-measurement-blind-spots` | A side-effect-free `web/scripts/live-digest.ts` shared by both drivers, plus remote descriptions, track events, timestamped frames, an allow-listed capture of what is sent, per-mid inbound stats, transport totals, CSRC/SSRC, an audio-only baseline phase, and `--expect-rev`. | M0 |
| `gallery-multi-camera` | A pure planner, caps scaled by tile count, avatar tiles, overflow, speaker-follow. Code is proven on the mock; going live waits on a SEEN verdict and on main-video sections being offered (video-send). | M4, M6 |
| `active-speaker` (both entries) | A 1 s CSRC poll with the client's own new-or-changed rule, mapped to the main-audio source of a published ENDPOINT. The user's own account is never named. | M5 |
| `pin-and-spotlight` (pin half) | A local "Pin for me", keyed by endpoint, per call, never published. | M6 |
| `shared-screen-zoom-fullscreen` | Zoom, pan, pinch, double-tap and full screen, with an iPhone fallback and an Escape ladder that respects full screen. | M7 |
| Map: "subscription accepted but zero RTP" | M0 to M2 | — |
| Map: "subscriptions are never asserted" | The mock records every request and has a read-back hook. | M1 |
| Map: "the endpoint filter on publishing is untested" | A Rust test. | M1 |
| Map: "video UI shows states the tenant does not back" (receive half) | A picture state that depends on the subscription. | M3 |
| Map: "incoming call's first offer: labels not learned" (label half) | `learnSections` before `setRemoteDescription` on the first offer. | M1 |
| Map: "remote media element robustness" (video half) | `paused` exists only after `live`. A muted or frozen track says so. | M3 |
| NEW | The renegotiation answer declares `"Video"` with no camera sent. The page now derives the list from its answer SDP, and the backend NARROWS the list to what the posted SDP backs. | M1 |
| NEW | An incoming group call is read as one-to-one. A conference is now decided by the service's own `isMultiParty`. | M1 |

**Deferred or rejected**

| Item | Verdict | Why |
| --- | --- | --- |
| Setting a spotlight | REJECTED here | `publishStateAsync` posts to `links.publishState` (calling-pluginless.js @1327423) and rearranges every attendee's stage. That needs its own OUTWARD entry, a guard name, and a harmless target, and there is no sandbox meeting. It belongs to meeting-participation / advanced-call-control. |
| Reading a spotlight | DEFERRED | The published-state type exists (`case"spotlight":return"spotlight"`, @940992), but no fetched chunk shows where a roster frame carries it. It needs one organizer action during a join-live hold, then a parser. |
| `dominantSpeakerInfo` link | DEFERRED | `isDominantSpeakerInfoLinkEnabled=!1` by default. The client's default is the CSRC poll, which needs no link, and `src/calling.rs:1176-1180` already refuses a link nothing reads. |
| MSI `-2` (subscribeAny) | DEFERRED | It only pays once cameras outnumber offered slots on a real tenant, and that has never been seen. M6 does speaker-follow with explicit ids. |
| Media control plane over a data channel | REJECTED | The sender falls back to signaling (`Sending SR by signaling`) while the MCP is not Active. A new transport is not worth a latency gain nobody would notice (NATIVE-CALLING.md § 10.2). |
| Audio SSRC binding, msid lines, msid-semantic | DEFERRED | Any msid signalling could move the verified audio receiver's stream association (`RemoteAudio` keyed by stream id, `if (!remote) return`). Only cname-only video binding is in scope. |
| A defensive audio fallback for an empty `event.streams` | REJECTED | Both reviewers flagged it. It would change the verified audio handler with no measurement behind it. It could double a voice, and a page-built stream never receives `removetrack`, so it leaks. LV0's track digest decides whether such tracks even exist. |
| Legacy `controlVideoStreaming` `globalTimeStamp` | DEFERRED | It is reached only when `applyChannelParameters` is never named, and on this tenant it always is (measured). Its `fmtParams` string is built from the same caps, in `VD()`'s spelling. |
| AV1/H.265 extra caps | DEFERRED | `isAv1Allowed:!1`, `enableH265VideoSourceRequest:!1`. |
| Renderer-pixel caps (`setMaxFSCapabilities`) | DEFERRED | Tile-count sizing comes first. Pixel-exact re-requests wait until a live run shows what the service honours. |
| Together mode, large gallery | REJECTED | They are composited by the service or by stage chunks that were not fetched, and are not a receive primitive. |
| 1:1 camera receive | OUT OF SCOPE (incoming-and-conversation) | A one-to-one has no roster sources and sends no source request (`if(!this.multiParty)return;`). Only the label half of the first-offer bug is folded in. |
| Camera slots from a reinviteless join | DEPENDENCY (video-send) | An audio-only join was offered only the sharing section (NATIVE-CALLING.md:1585-1592), and a three-section reservation was rejected (`web/src/lib/call-media.ts:348-358`). The planner consumes whatever main-video sections exist. |
| Showing the user's other device as a People row | DEPENDENCY (meeting-awareness, roster fidelity) | This design only labels that device's TILES and never names it as a speaker. |

## 4. Protocol

Status vocabulary: **measured** means seen on this tenant; **read-from-bundle** means read from
the 2026-11-05 client; **inferred** means neither.

### 4.1 Subscribe a section to a colleague's SHARED SCREEN

- **Method / link:** `POST` to `applyChannelParameters`. The link is named on the `callAcceptance`
  frame (measured: 13 acceptance links, NATIVE-CALLING.md:1121-1130) and re-read at post time from
  the merged set (`src/bin/server.rs:14682-14690`). The legacy `controlVideoStreaming` is used only
  if `applyChannelParameters` was never named.
- **Body** (the outer keys are serialized by serde in sorted order; the inner `mediaParameter`
  string is BUILT BY HAND in the client's own key order):

```json
{
  "applyChannelParameters": {
    "multiChannelParameter": {
      "mediaParameter": "{\"controlVideoStreaming\":{\"sequenceNumber\":2,\"controlInfo\":{\"sourceId\":212,\"streamMsid\":11,\"fmtParams\":[{\"max-fs\":8160,\"max-mbps\":135000,\"max-fps\":1500,\"profile-level-id\":\"64001f\"}]}}}",
      "mids": ["3"]
    }
  }
}
```

  - `streamMsid` is the section's xSourceStreamId (§ 4.7), as a JSON number when it is numeric.
    In the measured offer (audio, mid 3, x-data) mid 3 is at index 1, so the value is `11`.
  - `subStreamIndex` is ALWAYS omitted. The client sets it only through its multistream modifiers,
    which return early under the default config (`multiStreamSupported:!1`,
    `multiStreamSupportedVBSS:!1`), and `JSON.stringify` drops an undefined value.
  - `sequenceNumber` stays our per-call counter. The client's counter is class-static
    (`++eg.sequence`) and never resets between calls. A per-call counter starting at 1 was
    measured accepted.
- **Today's inner string, for comparison** (three differences: `streamMsid`, the type and values
  of `fmtParams`, and the key order):

```text
{"controlVideoStreaming":{"controlInfo":{"fmtParams":"max-fs=8160;max-mbps=245000;max-fps=3000;profile-level-id=42C02A;packetization-mode=1","sourceId":212,"streamMsid":"<MediaStream.id>"},"sequenceNumber":1}}
```

- **Bundle evidence** (calling-pluginless.js):
  - @2779490 `getSignalingSourceRequestMessage(t,i,r,n,s){if(!n)throw new Error("Failed to construct SourceRequestMessage, missing fmtParams");const a={sourceId:i,streamMsid:t,fmtParams:n};return a.subStreamIndex=s,{applyChannelParameters:{multiChannelParameter:{mids:[r],mediaParameter:JSON.stringify({controlVideoStreaming:{sequenceNumber:++eg.sequence,controlInfo:a}})}}}}`
  - @2836806 `const a=i===-1?r:this.extendReceiveCapabilitiesWithAdditionalCodecs(s,r);return n?.("awaitingSourceRequestDelivery"),this.mediaMessageController.sendSourceRequest(s.getXSourceStreamId(),i,s?.getMid(),a,s.getSubstreamIndex(),n)`.
    The `MediaStream` id the renderer passes is only the controller's LOOKUP key
    (`getMediaEntityByRemoteStreamId(t)`).
  - @2375130 `static getVideoCababilities(t,i,r){...i===B.MODALITY.sharing?(n=r.config.webrtcScreensharingCapabilityMaxFS,s=r.config.webrtcScreensharingCapabilityMaxFPS):...,new M6(n,s,r.config.h264SubscribeProfile)}`
  - @2373692 `P6=[2812,4218,8437,16875,33750,67500,108e3,135e3,198e3,244800,27e4,352500,52e4]`.
    @2374089 `calculateMbps(e,t){const i=e*t/100;return WI(P6,i,!0)}`.
    @761307 `function WI(e,t,i,r=0){for(let n=1;n<e.length;n++)if(e[n-1]+r<t&&t<=e[n]+r)return i?e[n]:e[n-1];return e[0]}`.
  - Config DEFAULTS: `webrtcScreensharingCapabilityMaxFS:8160,webrtcScreensharingCapabilityMaxFPS:1500`, `h264SubscribeProfile:"64001f"`, `useApplyChannelParametersForSourceRequests:!0`, `isAv1Allowed:!1`, `enableH265VideoSourceRequest:!1`.
    Several pass through ECS constraint transforms (`tP={…multiviewResolutionLimits:({current:e,constraints:t,logger:i})=>qB(e,t,i)…}` @766807), so the tenant's real values may differ.
- **Callbacks:** none new. `clientContentForMediaController {controlVideoStreaming, csrcInfo}`
  already rides the invitation, the join, the acceptance and every mediaAnswer
  (`src/calling.rs:1181-1186`).
- **Status:** shape read-from-bundle. A 200 for the OLD shape was measured on 2026-08-06
  (`subscribed: source=212 on mid=3 seq=1 (modern=true)`). Whether the new shape delivers RTP is
  unmeasured (LV1). Nothing has been re-measured since `clientContentForMediaController` was added
  on 2026-09-05 (`a357c18`), so the LV0 baseline is informative on its own.

### 4.2 Subscribe a section to a colleague's CAMERA, sized by tile count

- **Method / link:** as § 4.1.
- **Body:** the same envelope with the main-video mid and
  `{"sourceId":<roster main-video id>,"streamMsid":<xSourceStreamId>,"fmtParams":[{"max-fs":F,"max-mbps":M,"max-fps":3000,"profile-level-id":"64001f"}]}`.
- **Caps table**, built from RECV `fs = ceil(w/16)*ceil(h/16)` and `M = WI(P6, F*3000/100, true)`:

| Height | F | M |
| --- | --- | --- |
| 1080 | 8160 | 244800 |
| 720 | 3600 | 108000 |
| 540 | 2040 | 67500 |
| 360 | 920 | 33750 |
| 240 | 405 | 16875 |
| 180 | 240 | 8437 |

  - Tile count follows `multiviewResolutionLimits {1:1080,2:720,3:540,4:360,more:360}`.
  - A pinned tile among several asks for `maxSpotlightResolution` (1080).
  - Camera tiles in the strip under a share ask for 360. That is INFERRED (it is the `more` row),
    not read.
- **Bundle evidence:**
  - @2375130: the initial camera request is `webrtcVideoCapabilityMaxFS` (3600) unless
    `useMultiviewLimitsOnInitialRequest`, in which case
    `getVideoSendLadder().getMaxFsForHeight(multiviewResolutionLimits[1])`, which is 8160 at 1080.
  - @3065267 `multiviewResolutionLimits:{1:1080,2:720,3:540,4:360,more:360},maxSpotlightResolution:1080,…multiviewResolutionLimitUpdateThrottleDelay:300,useMultiviewLimitsOnInitialRequest:!0`.
  - @2452481 `limitMaxFS(e){return this.multiParty?Math.min(e,this.configProvider.config.webrtcMultiPartyRecvVideoMaxFS):…}` with `webrtcMultiPartyRecvVideoMaxFS:8160`.
  - The ladder (@764886 area): `this.fs=Math.ceil(e/B.MACROBLOCK_SIZE)*Math.ceil(t/B.MACROBLOCK_SIZE)`.
- **Status:** shape and numbers read-from-bundle, as defaults. Delivery is unmeasured: no
  main-video section has ever been offered to teams-lite (NATIVE-CALLING.md:1585-1592).

### 4.3 Re-request a different size on a subscribed section

- **Body:** the same mid and source with a new caps object. No `-1` in between.
- **Bundle evidence:** `handleStreamUpdateRequired(t,i,r){if(this.multiParty&&this.configProvider.config.webrtcMultiPartyRecvVideoSignaling){this.requestSource(t,i,r)…` with `webrtcMultiPartyRecvVideoSignaling:!0`.
- **Status:** read-from-bundle. Throttled at the client's 300 ms.

### 4.4 Release a section, and switch a section to another source

- **Release body:** `{"sourceId":-1,"streamMsid":<xSourceStreamId>,"fmtParams":[<that kind's INITIAL caps>]}`.
- **When:** 2000 ms after the source stopped being wanted AND no other wanted source takes the
  section (camera off, participant gone).
- **Never released:**
  - a section whose transceiver is stopped or whose port the service zeroed: the service has
    already taken it down;
  - a section whose current source is still wanted.
- **Switch from A to B on one mid** follows the client: an immediate `-1` on that mid, then
  B, serialized on that mid's queue. A single request carrying B is NOT what the client sends.
  LV4b records which shape the service honours when a real switch happens.
- **Bundle evidence:**
  - @2349945 `set stream(t){try{if(t===this.subStream)return;if(this.subStream&&(this.unsubscribe(Ie(),!1),this.notifyStreamChanged()),this.subStream=t,t){…const i=t.requestSource(this.msi);…`
  - @2352155 `unsubscribe(t=Ie(),i=!0){this.subStream&&(this.subStream.requestSource(-1).catch(()=>{}),this.streamReady=!1,this.subStream=null,…)}`
  - `webrtcVideoSubscriptionDisposeTimeout:2e3` (@3061596). `MSI:{unsubscribe:-1,subscribeAny:-2}`.
  - The `-1` carries the caps rebuilt after `resetToInitial()`, and the controller skips the codec
    extension for it (`i===-1?r:…`).
- **Status:** read-from-bundle. This app has never sent a `-1`.

### 4.5 Gate: the service's acknowledgement, no pending offer of ours, a connected transport

- **Inbound:** a `POST` to our `<surl>/call/mediaAcknowledgement/` (`src/calling.rs:101`),
  published on every `mediaAnswer` body. Its links are merged as on every frame. The body shape
  is unmeasured; the client reads only `.links`.
- **Backend gate**, per call:
  - `media_answers_posted` is incremented UNDER THE LOCK, BEFORE the POST, at all three
    `media_answer_payload` call sites (`src/bin/server.rs:7846`, `7914`, `7981`). It is
    decremented if that POST fails.
  - Each ack frame sets `acked = min(acked + 1, posted)`. Acks are COUNTED, never jumped to
    `posted`, so a late ack for answer 1 cannot open the gate for answer 2.
  - The state lives in a `tokio::sync::watch::Sender<NegotiationGate{acked, outgoing_pending}>` on
    the session. A watch is value-based, so no wake-up can be missed. `tokio::sync::Notify`'s
    `notify_waiters` stores no permit, the trap CLAUDE.md records for the enrollment Cancel.
  - `outgoing_pending` mirrors `outgoing_negotiation_at`. It is set and cleared at the same sites,
    and expires with `calling::OUTGOING_NEGOTIATION_TIMEOUT_MS`.
  - `call_subscribe` with `wait_for_ack: true` snapshots `target = posted` under the lock, then
    awaits `acked >= target && !outgoing_pending` with ONE 15 000 ms deadline
    (`MEDIA_ANSWER_ACKNOWLEDGEMENT_TIMEOUT_MS`).
  - When the call ends, the sender is dropped, so waiters return an error ("the call ended").
  - **On timeout the request goes out anyway, and the negotiation is NOT rejected.** The client
    goes further: it fails its `_MediaAcknowledgment` operation and rejects the negotiation, but
    still sends the source request. Rejecting an answer we already applied is not ours to declare.
- **Page gate:** the reconcile waits up to 10 s for `media.connectionState === "connected"`, then
  goes ahead. The simulated media is already `"connected"`; a mock hook can hold it at
  `"connecting"` for one spec.
- **Bundle evidence:**
  - @1922692 renegotiateIncoming: `…await n.execute("SignalingAcceptRenegotiation",()=>this.signalingSession.acceptRenegotiationAsync(o,l,i)),await n.execute("SignalingMediaAcknowledgement",()=>this.mediaAcknowledgmentOperation);const c=await n.execute("CompleteNegotiation",…)`
  - @2835615 onward, in requestSource: `if(this.negotiation.negotiationCompletedPromise.isPending){…try{await this.negotiation.negotiationCompletedPromise.promise}catch(o){this.logger.safe.info("source request should be sent even if negotiation rejected",o)}}`, then `"requestSource is postponed till transport is connected"`.
  - `mediaAnswerAcknowledgementTimeoutSec:15` (@982783).
- **Callbacks:** HANDLE `/call/mediaAcknowledgement/` in `handle_call_frame`, after the link merge.
  Nothing is posted back.
- **Status:** the arrival of ack frames is measured. The gate is read-from-bundle. Its effect on
  delivery is inferred (LV1).

### 4.6 Answer the service's renegotiation declaring only what the answer carries

- **Method / link:** `POST` to the offer's own `mediaAnswer` link (`renegotiation_answer_link`,
  taken once, `src/bin/server.rs:7962-8001`).
- **Body:** unchanged apart from `callModalities`:

```json
{"mediaAnswer":{"sender":{…},"callModalities":["audio","ScreenViewer"],
 "links":{"mediaAcknowledgement":"<ours>"},
 "clientContentForMediaController":{"controlVideoStreaming":"<ours>","csrcInfo":"<ours>"},
 "mediaContent":{…}}}
```

- **The rule (page side):** `answerModalities(answerSdp)` reads the answer's own sections, and it
  always starts from `["audio","ScreenViewer"]`, which is exactly the measured body.
  - It adds `"Video"` only when a main-video section has a port other than 0 and a direction of
    `sendonly` or `sendrecv`.
  - It adds `"ScreenSharer"` only when a sharing section does the same.
  - `"ScreenViewer"` stays UNCONDITIONAL. The client's `getSignalingMediaTypes` would drop it when
    its sharing state is not `receive`, but the client pre-negotiates a sharing entity at join and
    teams-lite does not. Declaring `["audio"]` alone has never been posted, and the service could
    read it as "this endpoint stopped viewing screens". Making it conditional is a later
    experiment, if ever wanted.
- **The rule (backend side, a rail):** `call_answer_media` NARROWS the list to what the posted SDP
  backs (`calling::declarable_modalities`). It drops an unbacked `Video` or `ScreenSharer` with one
  journal line, and posts.
  - It narrows rather than refusing because a tab loaded before this change keeps sending
    `"Video"`, and refusing would cost that tab its receive path while the backend can correct the
    claim itself.
  - The same rule for `call_offer_media` is the video-send workstream's to adopt.
- **One owner, one function** (README § 3, collision 8). This workstream owns how `callModalities`
  is derived for every ANSWER. There is one derivation from directions, `calling::signaling_media_types`
  over video-send's `media_section_list` (the client's `getSignalingMediaTypes`, as
  advanced-call-control § 4.0.5 specifies it; this M1 lands it if it is first), and one answer function
  built on it, `calling::declarable_modalities`, taking the answer SDP plus the call's hold state.
  The page's `answerModalities` mirrors it on shared SDP fixtures. Today's measured list is its default. video-send's `modalities-parity` switch
  and advanced-call-control's `[]` answer on a held one-to-one (`is_one_to_one()`, § 4.8) are FLAGS of
  that function, not new derivations. ringing M6b and media-robustness M6 change how the page APPLIES
  an answer and do not touch this list.
- **The stated exception to the switch rule** (README § 3). Dropping the unbacked `"Video"` ships
  with no switch, because it restores the measured body rather than changing it: `"Video"` arrived
  in `5b355f0` (2026-09-04 21:45), after both measured answers. Its live proof is README § 4 step 7
  (LV1 here), which must show the audio rates unchanged.
- **Bundle evidence:** @1862757 `getSignalingMediaTypes(t){const i=n=>n===…MEDIA_STATE.send||n===…MEDIA_STATE.sendReceive,r=[];return t.audio&&t.audio!==…inactive&&r.push("Audio"),i(t.video)&&r.push("Video"),t.sharing===…send?r.push("ScreenSharer"):t.sharing===…receive&&r.push("ScreenViewer"),r}`, fed by `s=await this.updateMediaModalities({offered:a},i)` in renegotiateIncoming (@1922692).
- **Status:** `["audio","ScreenViewer"]` was measured accepted (2026-08-06 meeting, 2026-09-04
  1:1). The rule is read-from-bundle. CLAUDE.md's "`Video` is the camera modality in either
  direction" is contradicted by the client.

### 4.7 Derive each receive section's xSourceStreamId

- **Local**, from the service's offer AS IT ARRIVED, before `fromMsSdp`. For each `m=` line in the
  service's own order, rejected and `x-data` sections included:
  - if `a=x-source-streamid:<v>` is present, the value is `v`, parsed with sdp-transform's
    `toIntIfInt` (numeric gives a number);
  - otherwise the value is `index + 10`.
- It is recomputed on every offer, and the planner reads the LATEST offer's map when it builds a
  request, never a value snapshotted when the track arrived.
- **Bundle evidence:**
  - @2379372 `fromRemote(e,t){…e.media.forEach((r,n)=>{const s=this.getMediaEntityByMid(r.mid);s&&(…s.setXSourceStreamId(r.xSourceStreamId||this.generateSourceStreamId(n)),…s.setSubStreamIndex(r.subStreamIndex))…`
  - @2385481 `generateSourceStreamId(e){return e+10}`.
  - `enableIncomingMlineOrderFix:!1` (@3071869), so the index is the service's own order.
- **Status:** algorithm read-from-bundle. Whether the service writes the attribute is unmeasured
  (LV0).

### 4.8 Decide whether a call is a conference: the service's own `isMultiParty`

- **Rule:** `CallSession.multi_party: Option<bool>`, set from what the service and the client
  state:
  - `Meeting` gives `Some(true)`, and the join answer's own `state.isMultiParty` is read too
    (measured to exist, NATIVE-CALLING.md:225);
  - an outgoing call gives `Some(ring.len() != 1)`, which is the client's own rule;
  - an incoming invite reads `conversationInvitation.isMultiParty` (`/_decoded/…` or bare);
  - any later frame carrying `conversationUpdate.state.isMultiParty` updates it (escalation).
  It is published as `call_state.multi_party` (`true`, `false` or `null`).
- **This is the ONE answer to "is this call one-to-one?" in the app** (README § 3, collision 6).
  `src/calling.rs` gets one predicate, `CallSession::is_one_to_one()` = `multi_party == Some(false)`,
  so an unknown call is never one-to-one. Every other workstream reads it rather than deriving its
  own:
  - video-send reads it for the codec filter. `call_prepare.one_to_one` (`src/bin/server.rs:7645`)
    is computed by this predicate on the placing branch, and the answering branch returns it too;
  - advanced-call-control reads it for hold, keypad and transfer, and adds its own eligibility on
    top (not escalated, one other party, a `8:` peer) without redefining one-to-one;
  - incoming-and-conversation M7 reads it for its copy.
  `call_state` carries `multi_party` and NO `one_to_one` field; the page reads
  `call.multi_party === false`. `fromMixer` (`this.callUsesMixer=a.fromMixer`, @1268723) answers
  a different question (does the media go through a mixer) and is journaled as a fact, never used
  as a second predicate. Whoever lands the invite parse first (this M1, or incoming M2's
  `IncomingCall.multi_party`) builds it for both.
- **Consequences:**
  - the backend refuses `call_subscribe` only for `Some(false)`;
  - the planner skips when `multi_party === false`;
  - an UNKNOWN call proceeds, because a one-to-one carries no roster sources and so can produce no
    request anyway (the client builds a fake roster for it: `!this.multiParty&&!this.callUsesMixer&&this.participantManager.handleRosterUpdate(this.getFakeRoster(a.body),!0,o)`).
- **Bundle evidence:**
  - @1376960 `e.conversationInvitation&&(…this.setMultiParty(e.conversationInvitation.isMultiParty,t),…)`.
  - @1347163 `e.state&&(this.setMultiParty(e.state.isMultiParty,t),…)`.
  - @1373921 `this.setMultiParty(this.participantManager.getParticipantsToInitiateCallWith().length!==1,i)`.
  - @2835615 `async requestSource(t,i,r,n){if(!this.multiParty)return;`.
- **Status:** read-from-bundle for the invite. M0 records whether this tenant's invites carry the
  field. The meeting-awareness workstream adds readers for `conversationUpdate`; reuse them if they
  land first.

### 4.9 Bind the service's SSRC to its VIDEO section (conditional)

- **Local SDP rewrite, behind the `bind` switch**, applied ONLY:
  - at the renegotiation-offer call site (`web/src/lib/call-media.ts:1071`);
  - ONLY when `multi_party !== false`;
  - ONLY to `m=video` sections with a port other than 0, a service-side direction of `sendonly` or
    `sendrecv`, and no `a=ssrc` line.
  It is NEVER applied to the incoming first offer (342) or to any answer (994). Those carry the
  verified 1:1 audio, and a real caller's `toMsSdp` writes `x-ssrc-range` there too.
- **Rewrite:**
  - with `a=ssrc-group:FID`, add `a=ssrc:<id> cname:<per-call uuid>` per flow SSRC;
  - otherwise add `a=ssrc:<x-ssrc-range min> cname:<uuid>`;
  - then remove the `x-ssrc-range` line.
  It adds no msid and no msid-semantic, and it never touches an audio section or a session-level
  line. It is idempotent.
- **Bundle evidence:**
  - @2367595 `fromMsSdp(e){if(e.port===0)return;…if(e.ssrcs)this.updateMsids(…);else{const i=this.getFlowSsrcs(e);i.length>0?i.forEach(r=>{this.addStream(e,r,QD,…)}):e.xSsrcRange&&this.addStream(e,e.xSsrcRange.ssrcMin,QD,…)}delete e.xSsrcRange}`.
  - The client's own `toMsSdp` writes the range FROM its sending SSRC (`e.xSsrcRange={ssrcMin:i,ssrcMax:i+t}`), which is why the minimum is the SSRC that is bound.
- **Promotion rule** (written down so that it cannot be skipped): `bind` becomes default only if
  1. LV0 or LV1 show RTP ARRIVING AND DROPPED: the transport packet rate rises by more than the
     audio-only baseline once the share starts, while that mid's `framesDecoded` stays 0; and
  2. LV3 turns the verdict to SEEN; and
  3. `web/e2e/webrtc-receive-binding.spec.ts` passes for the section shapes LV0 recorded.
  Otherwise the code path is deleted in M8. A rewrite earns its place only when a measurement names
  it (CLAUDE.md § Audio calls).
- **Status:** the client's behaviour is read-from-bundle; the need is inferred.

### 4.10 Who is speaking

- **Local:** `getContributingSources()` on the first audio receiver.
  - It polls every 1000 ms, starting 1000 ms after the call connects, paused while
    `document.hidden`.
  - "Speaking now" means a CSRC that is NEW or whose `timestamp` changed since the last poll, which
    is the client's own two-sided filter.
  - A CSRC is mapped to an ENDPOINT only when it EQUALS the `source_id` of a published
    `main-audio` stream (`publishing[].streams[].endpoint_id`).
  - An unknown CSRC names nobody.
  - A CSRC that maps to the USER'S OWN account (`publishing[].self`) names nobody either. They know
    when they are speaking, and that endpoint has no row to draw on. This also keeps the only
    possible live rig from producing a misattribution.
  - Speaking state is held for 1500 ms after the last change.
- **Bundle evidence:**
  - @2433391 `this.activeSpeakerManager.onContributingSourcesChanged(n.filter(s=>{const a=this.lastSources.find(o=>o.source===s.source);return!a||a.timestamp!==s.timestamp}).map(s=>s.source))`
  - @2434623 `getContributingSources(){…const e=this.peerConnection.getReceivers().find(t=>t.track.kind==="audio");return e?e.getContributingSources?.():[]}`
  - @1484182 `hasAudioSource(t){return this.audioSourceIds.some(i=>i===t)||this.audio.id===t}`
  - `webrtcContributingSourcesPollingStartDelay` and `…PollingInterval` (1e3).
- **Status:** read-from-bundle. Whether this tenant's mixed audio carries CSRCs equal to the
  roster's ids is unmeasured. The driver reads that directly in LV0, with no app change.

### 4.11 Inbound media-controller notifications

- `/call/csrcInfo/` and `/call/controlVideoStreaming/` (`src/calling.rs:115`, `138`) are LOGGED by
  shape only: key names and `sequenceNumber`. A line is written once per change of shape, because
  csrcInfo can be frequent. Nothing is acted on.
- The client drops a stale sequence for csrcInfo and dominant speaker, but NOT for
  controlVideoStreaming: @1222505 `this.handleWebRtcNotification(I.WEBRTC_NOTIFICATION_TYPE.CONTROL_VIDEO_STREAMING,r.controlVideoStreaming,!1)`.
  controlVideoStreaming goes to the client's SENDER, which is video-send's business.
- **Status:** read-from-bundle; arrival unmeasured (LV0 counts them by path).

### 4.12 Pin, zoom, pan, full screen: local only

- Pin changes which tile is large and what size that tile asks for (1080). Nothing is published.
- calling-roster.js carries `pin-participant`, and calling-pluginless.js has
  `maxNumberOfPinnedVideos:1` (@3084752). No zoom or full-screen protocol exists.

### 4.13 Spotlight: not built

- Read: published state `spotlight` (@940992), in an unmeasured shape.
- Set: `publishStateAsync` → `links.publishState`, rejected here (§ 3).

## 5. Changes by file

**`web/scripts/live-digest.ts`** (NEW, M0)
- Side-effect free, with no browser-driver import, so the guard never reads it as a driver.
- Exports:
  - `sectionAttributes(sdp)`: per m-section index, mid, label, direction, accepted or rejected,
    `x-ssrc-range` present, `x-source`, the `x-source-streamid` VALUE, `x-multi-stream`,
    `a=ssrc` / `a=msid` / FID present, sdes:mid extmap present, codec names and H.264
    profile-level-ids, and `x-signaling-fb` present;
  - `outgoingCaptureFilter(frame)`: keeps only `{method, mid, source_id, stream_msid, kind,
    max_height, modalities, caps_shape, wait_for_ack}` for `call_subscribe` and
    `call_answer_media`, and never the envelope, `sdp` or `token`;
  - `receiveVerdict(stats)`;
  - `transportSurplus(before, after)`.
- Unit-tested (`live-digest.test.ts`), including a writeRequest-shaped frame carrying a token field
  and an SDP, whose capture must hold neither.

**`web/scripts/join-live.ts`** (M0, merged to master BEFORE LV0)
- `PEER_CONNECTION_PROBE` also wraps `setRemoteDescription`, keeping `{type, sdp}` of each applied
  remote description and every `track` event as `{mid, kind, streamIds, trackId, at}`.
- `SIGNAL_PROBE` stamps each inbound message with `performance.now()`. It also wraps
  `WebSocket.send`, applying `outgoingCaptureFilter` IN THE PAGE before anything is stored.
- `readMediaStats` adds, per `inbound-rtp`:
  `{mid, ssrc, kind, bytes, packets, framesReceived, framesDecoded, frameWidth, frameHeight,
  pliCount, nackCount, packetsLost, codec}`. It also adds the `transport` `bytesReceived` and
  `packetsReceived`, and per audio receiver `getContributingSources()` /
  `getSynchronizationSources()` (ids and timestamps).
- A two-phase hold: a baseline window before the share starts, then the share window. Rates are
  computed per second.
- `readSignals` adds:
  - the ack arrival relative to our answer, and the ack body's key names;
  - each renegotiation's `mediaContent` keys;
  - the roster's main-audio ids per endpoint;
  - whether the invite or conversation frames carry `isMultiParty`;
  - csrcInfo and controlVideoStreaming counts and key names.
- `--expect-rev <sha>`: read the staged `VERSION` (`$XDG_DATA_HOME/teams-lite/service/VERSION`),
  check that `/proc/<MainPID of teams-lite-backend.service>/exe` is not `(deleted)` (the method
  `stale_backend_pid` uses in `bin/teams-lite-service.sh:353-362`), and refuse to measure unless
  both match. Print both in the header either way. It aims nothing.
- A RECEIVE VERDICT for single-install runs, judged from `getStats` ALONE (`framesDecoded`,
  `frameHeight`), never from a `data-picture` attribute that does not exist before M3.
- M1 adds `--receive <none|fmt,msid,ack,bind,speaker>`, which writes
  `localStorage['teams-lite.experiment.video-receive']` through `context.addInitScript` in the
  driver's own ephemeral context, and prints the flags in the header.
- The rails are unchanged: same pinned meeting, same proof immediately before the click, a hang-up
  on every path out.

**`web/scripts/call-live.ts`** (M0)
- Imports `sectionAttributes` from `live-digest.ts` in place of its own `mediaLines` (`357-`).
- Its pinned 1:1 target is untouched.

**`src/calling.rs`** (M1)
- `pub struct VideoReceiveCaps {max_fs, max_mbps, max_fps, profile_level_id}` with
  `for_screen()` (8160/135000/1500/`64001f`) and `for_camera(height)` over
  `RECV_LADDER = [(1920,1080),(1280,720),(960,540),(640,360),(426,240),(320,180)]`.
- `round_up_mbps(raw)` follows `P6` and `WI(…, true)` exactly, including returning `P6[0]` above
  the table.
- `to_fmtp_string()` gives `VD()`'s spelling for the legacy body.
- `pub enum StreamMsid { Number(u32), Text(String) }`.
- `SourceRequest {mid, source_id: i64, stream_msid: StreamMsid, caps: VideoReceiveCaps}`.
- `source_request_payload`: the modern inner string is hand-built in the client's order
  (`controlVideoStreaming{sequenceNumber, controlInfo{sourceId, streamMsid, fmtParams:[{max-fs,
  max-mbps, max-fps, profile-level-id}]}}`) with `serde_json::to_string` per scalar. The legacy
  body keeps its array with `fmtParams` as `to_fmtp_string()`. `DEFAULT_VIDEO_FMTP` is removed
  in M1b.
- `pub const MEDIA_ANSWER_ACKNOWLEDGEMENT_TIMEOUT_MS: u64 = 15_000` and
  `VIDEO_SUBSCRIPTION_DISPOSE_MS: u64 = 2_000`, each citing its bundle name.
- `IncomingCall.multi_party: Option<bool>`, read from `conversationInvitation.isMultiParty`.
  `pub fn multi_party_in_frame(&Value) -> Option<bool>` reads `conversationUpdate.state.isMultiParty`.
- `pub fn declarable_modalities(sdp) -> Declarable {video_sending, screen_sending}`, by label,
  port and direction.
- `RosterStream::json` gains `endpoint_id`.
- Tests:
  - the exact inner STRING for screen and camera at each height, and the parsed JSON equal to the
    bundle's object;
  - a release (`-1` with initial caps);
  - Number vs Text preserved;
  - `subStreamIndex` never present;
  - `round_up_mbps` at every boundary;
  - the legacy string;
  - `multi_party_in_frame` with both prefixes;
  - `declarable_modalities` on fixtures (camera recvonly / sendonly, sharing sendonly, a rejected
    section).

**`src/bin/server.rs`** (M1, M4)
- `CallSession` gains:
  - `multi_party: Option<bool>`;
  - `media_answers_posted: u64`;
  - `negotiation: watch::Sender<NegotiationGate{acked: u64, outgoing_pending: bool}>`;
  - `subscriptions: BTreeMap<String, PostedSubscription{kind, source_id, stream_msid, caps}>`.
- `json()` gains `multi_party`. Each `publishing` person entry gains `"self": m.mri ==
  self.local.id`, and each stream carries `endpoint_id`. The endpoint filter is unchanged.
- `call_subscribe` parses through a pure `parse_subscription(params, &CallSession)`. It REFUSES:
  - `multi_party == Some(false)` ("this call is between two people, and Teams takes no source
    requests in one");
  - `source_id` not `-1` and not `>= 1`;
  - `source_id >= 1` that is not a stream published by an endpoint OTHER than this one in the
    current roster;
  - `kind` that does not match that stream's label (`screen` ⇔ `applicationsharing-video`,
    `camera` ⇔ `main-video`);
  - a MISSING `kind`, or a present `fmt_params`, both with the sentence "This page was loaded
    before the backend changed — reload it to receive video";
  - `stream_msid` that is not an integer from 0 to 4 294 967 295 or a printable non-whitespace
    ASCII string of 1 to 128 characters (the legacy browser id, Firefox's braces included);
  - `max_height` outside {1080, 720, 540, 360, 240, 180} for a camera, or present for a screen.
  A `-1` for a mid with no recorded subscription answers `{released:false}` without posting.
- The params `caps_shape: "legacy"|"client"` and `wait_for_ack: bool` exist during M1 to M8 only.
- `call_answer_media`: narrowed modalities (§ 4.6); posted counted before the POST. The same
  counting applies at `7846` and `7914`.
- `handle_call_frame`, after the link merge:
  - the ack branch (`acked = min(acked+1, posted)`, `send_modify`);
  - `multi_party_in_frame`;
  - shape logs for csrcInfo and controlVideoStreaming, once per shape change.
- `outgoing_negotiation_at` set and clear sites also update `outgoing_pending`.
- `handle_incoming_call` copies `invite.multi_party`. `call_prepare` (placing) sets
  `Some(ring.len() != 1)`. `call_join` sets `Some(true)`.
- Journal lines:
  - `subscribed: kind= source= mid= stream_msid= caps={fs,mbps,fps,profile} seq= waited_ms= acked=a/p (modern=)`;
  - `released: mid= seq=`;
  - `the service acknowledged media answer a/p`;
  - `media answer declared "Video" with no camera section sending — dropped from the declaration`.
- Tests:
  - every `parse_subscription` refusal, including an incoming group invite read as a conference;
  - the gate: wakes on an ack, a late ack does not open a later answer, an outgoing pending offer
    holds it, call end releases it, 15 s timeout proceeds;
  - the counting is before the POST (a source scan, like the existing handler scans);
  - publishing excludes only THIS endpoint (new; closes a map open issue);
  - `call_subscribe` stays in `MACHINE_METHODS` and in the guard (existing tests).

**`web/src/lib/video-receive-experiment.ts`** (NEW, M1)
- The ONE reader of the experiment key `{fmt, msid, ack, bind, speaker}`. Malformed or unknown
  input falls back to the defaults.
- The header comment states that it shapes requests and inbound descriptions only, can never widen
  an outward action, is written only by the sanctioned driver, and is deleted in M8. Unit-tested.

**`web/src/lib/ms-sdp.ts`**
- M1: `sourceStreamIdsByMid(sdp)` and `sectionDirections(sdp)` (label, direction, port per mid).
- M2: `bindVideoSsrcs(sdp, cname)` (§ 4.9). `fromMsSdp` takes an optional
  `{bindVideoSsrcs?: string}`.
- Correct the stale header ("The one difference") and any comment claiming the transform does one
  thing.

**`web/src/lib/call-media.ts`**
- M1:
  - `RemoteVideo` gains `sourceStreamId` (from the latest offer) and `browserStreamId`
    (`event.streams[0]?.id`, kept for the `msid=false` control until M8), and keeps
    `mid` / `label` / `sharing`.
  - `RemoteVideoTracks.learnLabels` becomes `learnSections(offerSdp)` (labels plus
    `sourceStreamIdsByMid`). It is called BEFORE `setRemoteDescription` on the first offer
    (`341-342`) and on renegotiations (`1069-1071`).
  - `CallMedia` gains `contributingSources()` and `sectionIsLive(mid)` (not stopped, port not 0).
- M2:
  - video no longer needs `event.streams[0]`, and each tile is `new MediaStream([event.track])`
    per mid, so two sections can never share Chrome's stream association. It reports `mute` and
    `unmute` to the tile.
  - AUDIO is UNCHANGED: `if (!remote) return` stays for audio.
  - The `bind` option is passed at the renegotiation call site only (`1071`), for a conference
    only, with a per-call cname. The first-offer (`342`) and answer (`994`) sites never pass it,
    and a unit test pins that.
- `simulatedCallMedia` (`1195-`) mirrors `sourceStreamId` from the mock offer, returns contributing
  sources from a mock-only event, produces a frameless stream on a mock-only event, and can be held
  at `"connecting"`.
- Correct the stale header ("rewritten in exactly ONE respect").

**`web/src/lib/video-subscriptions.ts`** (NEW, pure, M4)
- `planSubscriptions({multiParty, sections, publishing, current, pinnedEndpoint, speakers,
  tileLayout, now})` returns `[{type:'subscribe'|'release', mid, sourceId, streamMsid, kind,
  maxHeight}]`.
- Invariants, each unit-tested:
  - a screen goes only on a sharing-labelled section and a camera only on a main-video one;
  - only live sections (not stopped, port not 0, recvonly or sendrecv) get any action, releases
    included;
  - nothing at all when `multiParty === false`;
  - publishing entries with `self: true` are subscribed (the user's other device is real) but
    never mapped to a speaker;
  - a wanted source's mid is never released;
  - a release fires only after 2000 ms unwanted with no other source taking the mid;
  - a switch is `-1` then the new source, back to back, on one mid;
  - a size change is one request with the same source;
  - identical (mid, source, caps, streamMsid) produces nothing;
  - when cameras outnumber camera sections the order is pinned, then most recent speakers, then
    roster order, with 3 s hysteresis (marked inferred).
- `receiveQuality(kind, visibleCameraTiles, pinned, underShare)`.

**`web/src/lib/active-speaker.ts`** (NEW, pure, M5)
- `speakingSources(prev, cur)` (new or timestamp changed).
- `speakersByEndpoint(sources, publishing)` (exact equality; a `self` entry names nobody).
- `heldSpeaking(state, now, 1500)` and `recency(history)`.
- Unit-tested, including "an unknown CSRC names nobody" and "the user's own account names nobody".

**`web/src/lib/call.ts`** (M1)
- Types: `PublishedStream.endpoint_id`, `PublishingParticipant.self`, `ActiveCall.multi_party`,
  `SubscribeKind`.
- `answerModalities(answerSdp)` per § 4.6. Unit-tested on SDP fixtures.
- The `qm` citation from NATIVE-CALLING.md is not reused: in this bundle `qm` is an IPv4 regex
  (@1645620). Lowercase `"audio"` is accepted as a MEASURED fact.

**`web/src/lib/ws-client.ts`** (M1)
- `callSubscribe({callId, mid, sourceId, streamMsid: number|string, kind, maxHeight?, capsShape?,
  waitForAck?})`. `fmtParams` is removed.
- The default 30 s ceiling covers a 15 s ack wait plus the POST.

**`web/src/lib/store.ts`**
- M1:
  - `answerRemoteOffer` (`2720-2742`) declares `answerModalities(answer)`.
  - `subscribeToRemoteVideo` sends `kind`, `streamMsid` (`sourceStreamId` or `browserStreamId` per
    `msid`), `capsShape` and `waitForAck` per the flags.
  - Per-mid subscription state `{state: queued|sent|accepted|refused, at, reason}` is kept for M3.
  - Today's re-send after each answered renegotiation stays until M4.
- M4: `reconcileVideoSubscriptions()` replaces it. It runs on:
  - an answered renegotiation, re-sending only for a mid whose xSourceStreamId changed or that came
    back from port 0, which is the client's no-re-send behaviour, verified in LV4;
  - a `call_state` whose publishing changed (`onCallState`, `2623`);
  - remote-video changes (`2855`), layout changes and pin changes.
  It is throttled to 300 ms, serialized per mid, and arms the 2 s dispose timers. On a switch it
  CLEARS the mid's name and sets it to `waiting` BEFORE the request, and names it only when the RPC
  resolves.
- M5: a CSRC poll timer (gated by `speaker`) sets `callSpeaking` (endpoint → lastSpokeAt).
- M6: `callPinnedEndpoint` (local, per call).
- `stopCallMedia` (`2804-2820`) also clears timers, pin, speaking and subscription state.

**`web/src/lib/call-stage.ts`**
- M3: the picture state per mid, and the label for a `self` endpoint ("{name} (your other device)").
- M6: the gallery in `callStageLayout` (`342-361`):
  - camera tiles plus avatar tiles for people with no video;
  - ordered pinned, then speaking (most recent first), then roster order;
  - capped at 9 on wide screens and 4 below md, with `overflow`;
  - `galleryColumns(n, viewport)` (1, 2, 2x2, 3x2, 3x3; at most 2 below md);
  - `visibleCameraTiles(layout)`;
  - a mid not currently wanted is NOT drawn.
- `callMiniPicture` (`370-375`): share, then pinned camera, then current speaker's camera, then the
  first camera.

**`web/src/lib/call-recording.ts`** (M3)
- `recordingSources` (`113-141`) composites only remote mids whose picture is `live` or `paused`.
  A `waiting`, `stalled`, `refused` or released mid is not drawn, because a labelled black tile in
  the file is a claim.
- The local pin is IGNORED: the recording is a record of the meeting, not of one reader's view.

**`web/src/components/call-video.tsx`**
- M3: `RemoteVideoFrame` (`27-72`) carries `data-picture="waiting|live|paused|stalled|refused"`
  (§ 7). Frames are detected with `requestVideoFrameCallback` where available, else
  `loadeddata` / `resize` / `currentTime`. The overlay uses `FadeArc`
  (`web/src/components/loading-ui/fade-arc.tsx`).
- `LocalVideoFrame`'s screen preview becomes `object-contain` (`102`).
- M5: a `speaking` prop.
- M7: `SharedScreenViewport`: CSS transform zoom from 1x to 4x, clamped pan, pinch,
  double-click / double-tap toggling 1x and 2x, ctrl+wheel. Full screen through
  `requestFullscreen`, with `webkitEnterFullscreen` on an iPhone `<video>`; the button is hidden
  when neither exists.

**`web/src/components/call-stage.tsx`**
- M6: the gallery grid, the overflow chip opening People, and a "Pin for me" control — hover on a
  fine pointer, long-press (`web/src/components/use-long-press.ts`) on a coarse one, with 44 px
  rows.
- M5: the speaking ring and the People "Speaking" signal (rows that exist), and the `AvatarCard`
  subtitle (`531-545`).
- M7: the Escape ladder (`150-170`):
  - with `document.fullscreenElement` set, the app does nothing, because the browser's Escape
    exits full screen;
  - otherwise a zoomed share resets to Fit first, then the open panel closes, then the stage folds;
  - it never hangs up.
- The header is unchanged.

**`web/mock/server.ts`**
- M1:
  - `MOCK_RENEGOTIATION_OFFER` (`4757-4780`) gains `a=x-ssrc-range` per video section and
    `a=x-source-streamid:13` on mid 3 only. The page must derive `11` for mid 1 (index 1) and read
    `13` for mid 3.
  - `call_answer_media` (`10451-10466`) narrows like the backend.
  - The ack is modelled internally (200 ms after an answer), and `call_subscribe` with
    `wait_for_ack` waits for it (15 s bound). `{kind:'call_media', withhold_ack:true}` holds it.
  - `call_subscribe` (`10513-10521`) mirrors EVERY backend refusal with the same sentences, and
    records each request with a timestamp. `{kind:'call_subscriptions'}` reads the list back, and
    `{kind:'call_subscriptions', reset:true}` clears it (a spec MUST reset).
  - `call_state` carries `multi_party`, `self` and `endpoint_id`.
- M4/M6: `mockPublishing` (`4708-4747`) is driven by
  `{kind:'call_meeting', cameras:N, sharing:bool, camera_off:<name>, self_device:bool}`, emitting
  roster deltas and a matching renegotiation with N main-video sections.
- M5: mock-only events for `simulatedCallMedia`: `call_mock_speaking {source_ids}`,
  `call_mock_dark {mid}`, `call_mock_connecting {hold}`.

**`web/scripts/preview.ts`** (M3, M5-M7)
- `--call` captures:
  - waiting, stalled and refused tiles;
  - a gallery of 4 in both themes;
  - the phone gallery with the chip;
  - the speaking ring cropped (`--dpr 3`);
  - the pinned layout;
  - the zoomed share with its toolbar in both themes;
  - the share at phone width.
- All inside `withPreview`, with the MOCK sentinel asserted.

**`web/e2e/webrtc-receive-binding.spec.ts`** (NEW, M2; the `webrtc-glare.spec.ts` pattern)
- `about:blank`, two real peer connections, canvas tracks plus an oscillator, no app, no backend.
- PARAMETERISED by the section shapes LV0 recorded: audio msid present or absent, ssrc present or
  absent, sdes:mid present or absent.
- It measures per-transceiver `framesDecoded` with and without `bindVideoSsrcs`, including a
  sender on a NON-minimum SSRC of the range.
- It measures that the audio receiver's track, `event.streams` and element playback are unchanged
  across a renegotiation that adds video bindings.

**`web/e2e/calling.spec.ts`** (extend `544-605`)
- The request is the client's: read back mid 3 with stream_msid 13, mid 1 with stream_msid 11,
  `kind` and `max_height`.
- Nothing is requested before the ack (withhold, then release).
- Nothing is requested before `"connected"` (hold).
- The answer declares no `Video` without a camera, and declares it with one.
- In M4: a camera turned off is released no sooner than 2 s; a stopped section is never released;
  a switch is `-1` then the new source, with the name cleared and waiting in between.
- No subscription when `multi_party === false`.
- Tiles go waiting → live, and dark → stalled with the copy; refused shows its copy.
- The user's other device is labelled as such and never named as speaking.
- In M6: a gallery of 4 asks for 360; pin asks for 1080 and moves the tile first.
- The speaking ring follows mock CSRCs.
- In M7: zoom and Fit; Escape in full screen leaves the stage alone; Escape resets zoom before
  folding.
- Every spec resets its hooks.

**`web/e2e/mobile.spec.ts`**
- At most 4 tiles plus a chip, in 2 columns.
- A long-press "Pin for me" with 44 px rows.
- Double-tap zoom.
- Toolbar targets of at least 44 px.
- The header unchanged.

**`NATIVE-CALLING.md`, `CLAUDE.md`:** see § 13.

## 6. Gating and consent

- **No new RPC.** There is no new `OUTWARD_METHODS` or `MACHINE_METHODS` entry and no new guard
  name. The Rust test that holds every gated method to being named in
  `.claude/hooks/guard-live-automation.sh` keeps passing (`call_subscribe` and
  `call_answer_media` are already named, line 981).
- **`call_subscribe` stays `MACHINE_METHODS`** (write token, refused read-only). A subscription or
  a release asks to RECEIVE and publishes nothing about the user. The backend now refuses MORE:
  - a known one-to-one;
  - a source the roster does not publish;
  - this endpoint's own sources, so the user's own camera can never be drawn as a colleague's;
  - a kind that disagrees with the stream's label;
  - an old page's shape.
- **`call_answer_media` stays OUTWARD.** Its declaration now NARROWS to what the posted SDP backs,
  so the backend, not the page, is what keeps `"Video"` off an answer that sends no camera.
- **Local only, nothing published:** the ack wait, the csrcInfo / controlVideoStreaming logs, the
  CSRC poll, Pin for me, zoom / pan / full screen, and the gallery. The `dominantSpeakerInfo` link
  stays unsent.
- **`call_status` (open) gains `multi_party`, `self` and per-stream `endpoint_id`.** None is a
  credential: an endpoint id is a device id the service publishes to everybody in the meeting.
- **A read-only backend:** refuses `call_subscribe` exactly as today and never posts a release.
- **The experiment switch:** one page-side reader, written only by the sanctioned driver in its own
  ephemeral context. It cannot aim anything or add an outward call, and M8 deletes it.
- **Live verification:** only `join-live` (`--from-chat`, `--pair`, `--share`, `--swap`) and
  `call-live` against their pinned targets, or the user's own click.
  - The driver change is MERGED to master before it is run live, because the guard sanctions the
    file by name (lines 514-517) and review is what makes that name safe.
  - `--receive` and `--expect-rev` are flags, never targets.
- **The mock mirrors every new refusal** with the same sentences.

## 7. UI and copy (including mobile)

**Surfaces:** the full stage (`StageContent`, `StagePeople`, `AvatarCard`), the mini window, and
`RemoteVideoFrame` / `SharedScreenViewport`. There are no new header controls, because the header
already overflows on phones.

**Picture state per remote tile** (`data-picture`), driven by the mid's SUBSCRIPTION state and its
FRAMES:

| State | When | What is drawn |
| --- | --- | --- |
| `waiting` | The request is queued or sent, or it was accepted less than 20 s ago with no frame. | A small FadeArc and "Waiting for {name}'s screen…" / "Waiting for {name}'s camera…" / "Waiting for the picture…" |
| `live` | A frame decoded (`videoWidth > 0`). | No overlay. |
| `paused` | ONLY after `live`: the track is muted, or no new frame for 3 s (camera) / 10 s (screen), the client's `notRenderingTimeInterval`. | A badge: "Picture paused". |
| `stalled` | Accepted 20 s ago or more and never `live`. The clock starts at the 200, not at the track. | "{name}'s screen has not arrived" (camera: "{name}'s camera has not arrived"; no name: "The picture has not arrived"), then "Teams accepted the request but has not sent the picture yet." |
| `refused` | The subscription was refused. | "Could not ask for {name}'s screen" (camera: "…camera"), then "Teams refused the request for this picture." The raw reason goes to the console. |

- A mid that is released or not wanted is NOT drawn. The person stays in the gallery as an avatar.
- On a switch the tile drops its old name and shows `waiting` until the new source is answered,
  so B's first frames never appear under A's name.
- The user's other device's tile is labelled "{name} (your other device)".

**Gallery (no share):**
- Columns 1, 2, 2x2, 3x2, 3x3; at most 9 tiles wide and 4 below md (2 columns).
- Avatar tiles for people with no video. Nobody is subscribed for them.
- Order: pinned, then speaking (most recent first), then roster order.
- Past the cap, a chip "+{n}" (`aria-label` "{n} more people", `title` "Open People to see
  everyone"), which opens People.

**With a share:** the share keeps the content (existing rule). Faces become a strip, pinned first,
with 2 on a phone plus the chip.

**Speaking:**
- A 2 px primary ring on the tile and on the People avatar, a People-row signal "Speaking", and a
  tile `aria-label` of "{name}, speaking".
- On the audio-only `AvatarCard` the subtitle reads "{name} is speaking" / "{a} and {b} are
  speaking" / "{n} people are speaking", and returns 1.5 s after silence.
- Nothing is drawn for an unknown CSRC or for the user's own account.

**Pin for me:**
- Fine pointer: a hover button "Pin for me" (`aria-label` "Pin {name} for you") and "Unpin". The
  badge reads "Pinned" with `title` "Only you see this layout".
- Coarse pointer: a long-press menu with the same item, in 44 px rows.
- Keyed by endpoint, per call, cleared when the call ends. A pinned tile asks for 1080p.

**Shared screen:**
- A toolbar at the tile's top right (on hover; always shown on a coarse pointer): "Zoom out",
  "Zoom in", a "150%" readout, "Fit to window", and "Full screen" / "Exit full screen". The button
  is hidden when neither API exists.
- While zoomed, drag pans. A one-time hint reads "Drag to move around" ("Pinch to zoom" on touch).
- Double-click or double-tap toggles Fit and 200%.
- Escape follows § 5's ladder and never hangs up.

**Mini window:** share, then pinned camera, then current speaker's camera, then the first camera,
then the avatar.

**Mobile:**
- 2 columns, 4 tiles plus the chip.
- Pinch and double-tap zoom.
- Full screen through the iPhone video element's native full screen.
- Every new target at least 44 px.

**Copy:** English only, no contractions, and nothing claims a picture arrived before a frame
decoded.

## 8. Tests, mock and captures

- **Rust (`cargo test`):** the `calling.rs` and `server.rs` tests listed in § 5. The exact inner
  STRING is pinned AND the parsed JSON is compared with the bundle's object, so a later change of
  key order fails loudly rather than silently.
- **Web unit (`bun run test` + `typecheck`):**
  - `live-digest.test.ts`: the allow-list strips a token and an SDP;
  - `ms-sdp.test.ts`:
    - `sourceStreamIdsByMid` over rejected and x-data lines;
    - `bindVideoSsrcs` with FID vs range, idempotent, no msid, audio untouched, port 0 skipped,
      recvonly skipped;
  - `call-media` unit: the bind option is passed only at the renegotiation site;
  - `video-subscriptions.test.ts`: every invariant, including "mid rejected → no -1" and "switch is
    -1 then B";
  - `active-speaker.test.ts`;
  - `call-stage.test.ts`;
  - `call.test.ts` (`answerModalities` on SDP fixtures);
  - `video-receive-experiment.test.ts`.
- **Browser, no tenant:** `webrtc-receive-binding.spec.ts`, parameterised by LV0's recorded shapes.
  It is the gate for `bind`.
- **Mock + E2E:** as in § 5. Every spec resets `{kind:'call_subscriptions', reset:true}` and
  `call_invite {reset:true}`.
- **Preview:** `cd web && bun run preview -- --out /tmp/call --call`, with the new captures listed
  in § 5.
- **Scope of runs:**
  - Rust and web for M1 and M4 (they cross the protocol);
  - web only for M3, M5, M6 and M7;
  - the browser spec for M2;
  - a full E2E run compared against a BASELINE run on master (CLAUDE.md's flake note), on free
    ports (`E2E_MOCK_PORT` / `E2E_WEB_PORT`).

## 9. Live verification plan

Every step runs only after its milestone is merged to master and the always-on service has
re-staged it. Each run passes `--expect-rev <master sha>`, so a held restart cannot make a run
measure old code. The receiver is always 19440 (the staged pair). The user's part is joining the
pinned meeting (`33706839527254`) from real Teams on another device and keeping the always-on
service up (their own start).

| Step | After | Command | The user does | Success signal |
| --- | --- | --- | --- | --- |
| **LV0** baseline | M0 | `cd web && bun run join-live -- --from-chat --hold 45 --expect-rev <sha>` (`--local` if the tailnet name does not resolve) | Speaks for the first ~15 s WITHOUT sharing, then turns their camera on, shares their screen and keeps speaking. | The call stays up and is hung up. The digest prints, per service section, every § 5 attribute (x-source-streamid value or absent, sdes:mid, ssrc/msid/FID, profile ids). It also prints: the ack arrival relative to our answer, and its body keys; whether the invite or conversation frames carry `isMultiParty`; the outgoing `call_subscribe` params; track events (mid → stream ids, including any audio track with empty streams); per-mid inbound-rtp; the transport packet rate in the baseline and share windows; CSRCs against the roster's main-audio ids. The audio inbound/outbound rates become the regression reference. |
| **LV1** one variable at a time | M1 | `--receive fmt`, then `--receive fmt,msid`, then `--receive fmt,msid,ack` (order adjusted by LV0) | The same share and camera, kept on across the three runs (about 3 min). | The journal shows `subscribed: kind=screen … caps={…}` answered 200 each run. The VERDICT reads SEEN in the first variant where the screen mid has `framesDecoded > 0` and `frameHeight > 0`. Audio rates are within noise of LV0. The call never ends from a receive failure. |
| **LV2** minimality, then the default | M1, then M1b | `--receive <k>` alone; after M1b, no `--receive` | Shares again. | The same verdict with only variable k, and the default run is also SEEN. **If nothing was SEEN:** M1b still promotes the client shape as a correctness alignment after one default run shows no audio regression, and the remaining hypotheses are recorded (bind if RTP is dropped; otherwise the reinviteless join and the legacy spelling). |
| **LV3** SSRC binding | M2 + the browser spec | `--receive fmt,msid,ack,bind` | Shares and speaks. | Run ONLY if LV0/LV1 showed the share window's transport rate rising by more than the baseline with `framesDecoded` 0. Success: `framesDecoded > 0` on the screen mid with `inbound-rtp.ssrc` equal to the declared range minimum; inbound audio keeps climbing while they speak; the call stays up. Otherwise `bind` is never promoted. |
| **LV4** lifecycle | M4, and only after a SEEN verdict | `--hold 60` | Stops sharing, starts again, turns the camera off, at ~15 s intervals. | The captured requests show: no `-1` for a mid whose section the service zeroed; one subscribe after the share restarts; no re-send for an unchanged section after a renegotiation, with the picture still `live` afterwards; no tile stuck at `waiting`. |
| **LV4b** a real switch (optional) | M4 | `--hold 60` | Shares from one device, then from a second device of their own (a takeover). | The sharing mid gets `-1` then the new source, and the tile goes live under the right label. If the service refuses the pair, that is recorded and the single-request switch becomes the inferred alternative. |
| **LV5** who is speaking | M5 (digest needs only M0) | `--receive speaker --hold 40` | Speaks, pauses, speaks. | The CSRC set changes within ~1 s of speech and equals the main-audio `source_id` of their other endpoint. With `speaker` on, NO tile and no row carries `data-speaking` for the user's own account. The ring for a COLLEAGUE is proven on the mock, and live only by the user's own look in a meeting with a colleague. If no CSRCs appear at all, the feature stays off. |
| **LV6** camera receive | M6, and main-video sections from video-send, or send working | `cd web && bun run join-live -- --pair` (19442 sends its camera, 19440 watches), then `--pair --share` | Starts both installs (`systemctl --user enable --now teams-lite.target` and `teams-lite-app.service`). | The pair VERDICT is SEEN on a main-video mid with the receiver's tile named, and the request's caps show 1080 for one tile. A real multi-camera gallery is checked only by the user's own look. |
| **LV7** zoom and full screen | M7 | preview captures, then the user's own look | Opens the app on a phone during a meeting with a share; tries pinch, double-tap and Full screen. | Text in the share is readable when zoomed. Full screen enters and exits. Escape inside full screen exits only full screen. |

## 10. Milestones

Each milestone is shippable alone.

| # | Milestone | Effort | Risk | Depends on other workstreams |
| --- | --- | --- | --- | --- |
| M0 | **Measure first.** `live-digest.ts`, join-live and call-live instrumentation, `--expect-rev`; driver-only, merged before LV0. Ends with LV0. | M | None outward. Mitigated: an allow-list filter in the page, unit-tested; shape-only output. | meeting-awareness M0 also extends `SIGNAL_PROBE`: share `live-digest.ts`, whoever lands second rebases. |
| M1 | **The client's request and an honest answer.** Caps, the hand-built inner string, xSourceStreamId, the ack/outgoing gate with a watch, `multi_party`, `parse_subscription` with roster/label/self rails, `endpoint_id`/`self` on publishing, `learnSections` before `setRemoteDescription`, answer modalities from the SDP plus backend narrowing, the mock mirror with read-back. Switch keys `fmt`, `msid`, `ack` default OFF; dropping `"Video"` is ON (it restores the measured body; the one stated exception to the README's switch rule, proven by LV1 = README § 4 step 7). M1 also lands the one one-to-one predicate (§ 4.8, README collision 6) and the one answer-modalities function (§ 4.6, README collision 8). Then LV1/LV2 and **M1b**, which promotes the client shape: M1b ships separately (README row VR1b) and only after LV1/LV2. | M | Low. A refused body costs a dark tile, today's outcome. The gate delays a tile by at most 15 s. There is no `setRemoteDescription` change. | meeting-awareness M3 (roster fidelity changes `publishing`: a publisher must never be dropped; coordinate before merging). video-send (backend narrowing touches `call_answer_media`, its camera-answer path). incoming-and-conversation (an incoming group call is now a conference). |
| M2 | **SSRC binding (conditional) and per-mid tile streams.** `bindVideoSsrcs` behind `bind` at the renegotiation site for a conference; per-mid video streams (default, video only; audio untouched); the parameterised browser spec; LV3. | S | Medium if bind is ever promoted. Mitigated: video-only, cname-only, one call site, browser-proven per measured shape, promotion only on a measured need. Per-mid wrapping is low (video tiles only, E2E-pinned). | media-robustness-devices (touches the same track handler: coordinate). |
| M3 | **Honest tiles.** The subscription-aware picture state, `paused` only after `live`, the own-device label, recorder rules, `LocalVideoFrame` set to object-contain, E2E with a dark stream, preview. | S | None outward. Mitigated: a 20 s threshold counted from the 200. | media-robustness-devices (remote media element robustness: the audio half is theirs). |
| M4 | **Lifecycle.** Planner, reconcile, the 2 s dispose then `-1`, switch as `-1` then B, no release on stopped mids, no re-send per negotiation, mids drawn only while wanted, the publishing endpoint-filter test, mock roster hooks. Goes live only after a SEEN verdict; then LV4/LV4b. | M | Medium: a wrong `-1` blanks the share. Mitigated: invariants, unit tests over roster flaps, the dispose delay, live sections only. | meeting-awareness M3 (deltas and per-endpoint roster). |
| M5 | **Who is speaking.** CSRC poll, endpoint mapping, the own account names nobody, ring and People signal, AvatarCard line, the `speaker` key, LV5. | M | Low technically; the real risk is misattribution. Mitigated: exact mapping only, off until LV5. | meeting-awareness (People rows; the ring only decorates rows that exist). Independent of M1-M4. |
| M6 | **Gallery and Pin for me.** Layout, avatar tiles, overflow, tile-count caps, pin to 1080, speaker-follow, long-press on phones. Live only after SEEN and main-video sections; LV6. | L | Medium: request churn (mitigated by throttle and dedupe), and camera sections may never be offered (the dependency). | video-send (reinviteless join or a renegotiation that yields main-video sections); M4; M5 for speaker-follow. |
| M7 | **Zoom, pan and full screen.** Viewport, toolbar, Escape ladder with the full-screen rule, E2E desktop and phone, preview; LV7. | S | None outward; Escape conflicts mitigated by the ladder and a spec. | none |
| M8 | **Retire the switch.** Delete the experiment reader, `caps_shape` / `wait_for_ack`, `browserStreamId`, and the `bind` path if never promoted. Keep `--receive none` only if still useful as a baseline. Final docs pass. | S | Low. It removes the fast rollback, so it runs only after one real meeting used the defaults. | none |

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| An inbound rewrite reaches the verified 1:1 audio, whose first offer and answers carry `x-ssrc-range` too. | Bind at the renegotiation call site only, in a conference only, on service-sending video only. A unit test pins that the first-offer and answer sites never pass the option. The switch is default off, and promotion needs a measured need plus a browser proof per recorded shape. |
| Binding changes the meeting's audio receiver association. | No msid and no msid-semantic, video sections only. The browser spec asserts the audio track, streams and playback are unchanged, using the audio shape LV0 recorded rather than an invented one. |
| The verified audio handler changes. | It does not: the audio branch keeps `if (!remote) return`. The proposed fallback was dropped. |
| `isMultiParty` is absent on this tenant's invites. | `None` proceeds, and a one-to-one has no roster sources, so it produces no request. LV0 records the field. |
| The `index + 10` fallback is computed over the wrong list. | It is computed on the service's offer as it arrived, over every m-line in order, recomputed per offer, and read at request time. The mock exercises both the attribute and the fallback. |
| The service refuses or ignores the new fmtParams type or key order. | Behind the switch, one variable per run. A refusal is a journal error and a dark tile; the receive path never ends a call (`store.ts:2716-2742`). |
| The ack never arrives, or a late ack opens a later answer. | A bounded 15 s wait that then proceeds. Acks are counted, a watch channel cannot miss a wake-up, and the call ending releases waiters. |
| A `-1` blanks the share being watched, or churn floods requests. | Planner invariants; releases only after 2 s unwanted and only on live sections; a 300 ms throttle; per-mid serialization; dedupe on (mid, source, caps, msid). |
| Who-is-speaking names the wrong person, or the user. | Exact equality with a published endpoint's main-audio id only; the user's own account names nobody; off until LV5. |
| Main-video sections are never offered. | The gallery is mock-proven; going live is an explicit video-send dependency. People's "Camera on" keeps telling the truth; no tile is drawn for a section that does not exist. |
| A dark rectangle reads as a working share. | Subscription-aware `data-picture` with explicit copy, from frame events, never from the 200. |
| An LV run measures old code (the restart is held by an `@claude` run). | `--expect-rev` checks the staged `VERSION` and that the running backend's exe was not replaced. |
| Driver instrumentation leaks a secret. | The outgoing capture copies allow-listed fields in the page (unit-tested with a token and an SDP); output is shapes, ids and counts only. |
| The mock diverges from the backend. | Every refusal is mirrored with the same sentence; the ack and narrowing are modelled; the read-back hook lets E2E assert exact params. |
| The switch lingers as a second code path. | One reader; defaults flip per milestone after a live proof; M8 deletes it. |

## 12. Open questions and decisions for the user

**Open questions** (each answered by the step named):
1. Does the service write `a=x-source-streamid`, `a=x-source` or `a=x-multi-stream` on its
   sections? (LV0)
2. Does its video section negotiate sdes:mid, and does any `a=ssrc` / `a=msid` accompany its range?
   (LV0)
3. Does RTP reach this machine and get dropped (the share-window transport rate above baseline),
   or does the service send nothing? This one number orders M1 against M2. (LV0)
4. Does the ack frame carry refreshed media-controller links, and does its body correlate to an
   answer? (LV0)
5. Do this tenant's invites carry `conversationInvitation.isMultiParty`? (LV0, on the next
   incoming group call)
6. Does Chrome give every remote video section one shared MediaStream today? (LV0's track digest;
   M2 makes it irrelevant for tiles)
7. Does the service honour the requested size (`frameHeight` for a 360 request)? (once bytes flow)
8. Must subscriptions be re-sent after a service renegotiation? The client does not. (LV4)
9. Do CSRCs equal roster main-audio ids here, or do voices arrive as separate SSRCs? (LV0, LV5)
10. What makes the service offer main-video sections? (shared with video-send)
11. Where does a spotlight state appear in a roster frame? (one organizer action during a hold)
12. Does `profile-level-id=64001f` matter when the answer negotiated another profile? (LV0 prints
    both)

**Decisions for the user:**
1. **Schedule LV0.** It needs you in the pinned meeting from real Teams: speak first, then turn
   your camera on and share. About 1 minute.
2. **If no variant decodes after LV1/LV2,** may M1b still promote the client's request shape as a
   correctness alignment? (Proposed: yes, after one default run shows no audio regression.)
3. **Should speech from your own other device be shown?** (Proposed: never. The user's own account
   names nobody.)
4. **May `call_status` carry each published stream's `endpoint_id`?** It is a device id the service
   already gives everybody in the meeting, and it is what lets pin and speaking tell your laptop
   from your phone.
5. **For the spotlight read:** spotlight someone once as organizer during a join-live hold, when
   convenient.
6. **Gallery caps:** 9 on a wide screen and 4 on a phone, with an overflow chip. Confirm or adjust.

## 13. Review notes

**Docs to update, each with the milestone that disproves the claim:**
- NATIVE-CALLING.md § 10.2 (~1136-1150), with M1:
  - `streamMsid` is the xSourceStreamId on the wire; the `MediaStream` id is a lookup key;
  - `fmtParams` is an array of one object;
  - add the release, the switch, the gate, the conference rule, the caps table, and the key order.
- NATIVE-CALLING.md:845-848, with M1: retract "byte for byte". Three things differed: `streamMsid`,
  the `fmtParams` type, and the key order.
- NATIVE-CALLING.md § 10.1, with M1: `Video` means SENDING. The `qm` sentence is wrong for this
  bundle (`qm` is an IPv4 regex); keep the measured acceptance of lowercase `audio`.
- NATIVE-CALLING.md:1672-1687, with LV0-LV3: record the results; strike `mdRequestId` as a receive
  candidate (it matches our OWN sharing streams, @1944571); add the transport-rate method.
- NATIVE-CALLING.md § 2.5 (~278-283), with M2: the conditional video binding and why audio and msid
  are left alone.
- CLAUDE.md § Video in a meeting, the bullet "AND THE ANSWER DECLARES `Video`…" (~6385), with M1:
  replace it with the client's rule and the backend narrowing. Add the receive rules (request
  shape, gate, xSourceStreamId, conference by `isMultiParty`, endpoint keying, honest picture
  states, lifecycle, who is speaking, pin, zoom, the switch and its removal).
- CLAUDE.md § Audio calls (~6057-6080), with M2: the "ONE respect" bullet is already stale
  (candidates, IPv6, extmap ids). State the binding exception only if it is promoted, with its
  measured reason.
- CLAUDE.md § A call is a PAGE (~6555), with M6/M7: the gallery and mini-window order, the Escape
  ladder with full screen and zoom, and "a mid not wanted is not drawn".
- CLAUDE.md § Recording a call (~6470), with M3: the recorder composites only live or paused
  pictures and ignores the local pin.
- CLAUDE.md § Automation safety, with M0/M1: join-live's `--receive`, `--expect-rev` and the new
  digest fields; the pair rig is for SENDING, and receive is measured against the user's own share.
- Code comments, each with the milestone that touches it: `store.ts:2726-2733`, `call-media.ts`
  header, `ms-sdp.ts` header, and the `DEFAULT_VIDEO_FMTP` docstring.

**Accepted from review 1 (code and protocol accuracy):**
- **Kind is not one-to-one.** Verified: `src/bin/server.rs:14984-14995`. Replaced by the service's
  own `isMultiParty`, which the bundle sets from the invite (@1376960), from conversation state
  (@1347163) and from the outgoing participant count (@1373921) (§ 4.8). Partial pushback: the
  claim that `server.rs:7645` has the same flaw is wrong. That `one_to_one` is answered only on the
  PLACING branch, where `Call` means `ring.len() == 1`, so it is exact there.
- **mri keying.** Verified: `others()` filters by mri, while `publishing` filters by endpoint. Pin,
  speaking and tiles are keyed by endpoint; the user's own account is never named a speaker; LV5 is
  restated around the digest.
- **`data-picture` before M3.** LV verdicts now come from `getStats` alone. M0 is driver-only: every
  frame already reaches the page as `call_signal` (`server.rs:14605`), so ack timing, csrcInfo and
  invites need no backend change. The backend journal lines move to M1.
- **The switch.** Verified: `set stream` → `unsubscribe` (`-1`) → `requestSource(msi)`
  (@2349945, @2352155). The planner follows the client, and LV4b records the result.
- **The browser spec's audio shape.** Parameterised by LV0's recorded shapes.
- **The gate.** A watch replaces Notify; acks are counted; outgoing offers hold the gate; call end
  releases waiters; the timeout proceeds without rejecting, as stated.
- **The msid control and Firefox ids.** `browserStreamId` is kept beside `sourceStreamId` until M8;
  the legacy validation is a bounded printable string.
- **The reload sentence** is keyed on a MISSING `kind` (verified: `ws-client.ts` sends
  `fmt_params` only if given).
- **`qm`** is an IPv4 regex (@1645620); the citation was dropped.
- **`subStreamIndex` is always omitted** (`multiStreamSupported:!1`, verified).
- **The CSRC filter** counts a new source too (`!a||…`, verified).
- **controlVideoStreaming is not sequence-dropped** (verified `,!1`); its log is rate-limited.
- **The surplus** is measured against an audio-only baseline with speech, in the same run, as
  packets per second.
- **Key order.** Verified: serde_json has `default` and `std` only. The inner string is hand-built
  in the client's order; tests pin the string and compare parsed JSON.
- **Binding scope** to service-sending sections, plus the non-minimum SSRC case in the spec.
- **`answerModalities` from the answer SDP.**
- **`paused` only after `live`;** stalled copy counted from the 200.
- **Timestamps on `SIGNAL_PROBE`;** the redundant Rust `media_section_attributes` was dropped.
- **Dedupe.** The "two pages" rationale was wrong (verified: only the page holding `callMedia`
  subscribes, `store.ts:2759`). The backend dedupe was dropped and the planner dedupes.
  `learnSections` runs before `setRemoteDescription`, and the planner reads the latest offer.
- **No release on a rejected mid.**
- **Config values labelled as defaults,** the camera's initial caps via the send ladder, and "360
  under a share" marked inferred.
- **The audio fallback dropped.**
- **Explicit gates:** M4 and M6 go live only after SEEN; a fallback if nothing decodes; M5 is
  independent.

**Rejected or modified from review 1:**
- **"Reuse `TEAMS_LITE_CALL_DEBUG` / `CALL_CAPTURE` for the digest":** rejected as the
  live-verification path. Both need the user's own unit restarted with environment variables,
  which is not tooling's to do, and both write live identities and SDP. They remain the user's
  tool. The driver digest is the agent's.

**Accepted from review 2 (rules, safety, regressions):**
- **The blocker (fromMsSdp's three call sites).** Verified (342, 994, 1071). Binding is now limited
  to 1071, in a conference, and a unit test pins the other two sites.
- **The audio fallback dropped.**
- **The kind rule** (duplicate of review 1).
- **ScreenViewer stays unconditional,** so only `"Video"` is dropped, restoring the exact measured
  body (verified: `5b355f0` at 21:45 came after the 1:1 verified at 09:40).
- **Binding promoted only on a measured need,** else deleted; § Audio calls added to the docs.
- **Release and switch drawing:** an unwanted mid is not drawn, and the name is cleared before a
  switch.
- **LV5 restated.**
- **`--expect-rev` against a held restart.**
- **The driver merged before live use,** with an in-page allow-list and a unit test.
- **Dedupe rationale** (duplicate).
- **Ack counting** (duplicate).
- **Kind checked against the roster label.**
- **A `speaker` key** in the switch and `--receive`.
- **Stall timer from the 200, and refused copy.**
- **Docs for § A call is a PAGE and § Recording a call,** with explicit recorder rules.
- **Escape in full screen.**
- **Pin keyed by endpoint.**
- **`live-digest.ts`** shared by both drivers.

**Rejected or modified from review 2:**
- **"`call_answer_media` should REFUSE an unbacked `Video`":** modified to NARROW. A tab loaded
  before this change sends `"Video"` on every answer; refusing would cost that tab its receive path,
  while narrowing makes the published declaration exactly what the SDP backs, which is the consent
  property that matters.
- **"Keep everything not proven default-off except logging, the modality fix and tile UI":** partly
  kept. Also default-on in M1:
  - the backend's new REFUSALS, because they narrow what a client may ask, and their failure mode
    is a dark tile, which is today's outcome, with no outward or audio effect;
  - in M2, per-mid video tile streams, which touch neither audio nor the wire.
  Every wire or SDP change (`fmt`, `msid`, `ack`, `bind`, `speaker`) stays behind the switch.
- **"Simulated media will wait 10 s for connected":** unfounded for the default case. Verified:
  `simulatedCallMedia.connectionState` already returns `"connected"`
  (`call-media.ts:1320-1322`). Only a hold hook was added, for the one spec that proves the wait.
