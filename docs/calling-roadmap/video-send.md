# Video send: getting the user's camera and screen into a Teams meeting and a 1:1 call

**Workstream:** video-send. **Status:** design, final after two adversarial reviews (§ 13).
**Sources:** the code at the commits named below; `NATIVE-CALLING.md`; the Teams web client's
public bundle as fetched 2026-11-05 (`calling-pluginless.js` unless another file is named). Every
bundle snippet below was re-read in that bundle for this document. A claim marked **measured** was
answered by the tenant; **read-from-bundle** is the client's code; **inferred** is neither.

## 1. Summary

Today the service turns down every video section teams-lite offers, camera and screen alike, in
meetings and on a 1:1. This workstream replaces the guessed "add a section mid-call" offer with the
shapes the real web client uses. They are found with a ladder of experiments that changes one
variable per step. Every step sits behind a per-call switch from a closed list. Only the sanctioned
live drivers turn a switch on, so the verified audio path (1:1 both ways, meeting join) stays
byte-for-byte what it is today for the user, and Rust golden tests pin that.

The ladder starts with steps that cost nothing to try:

- drop the PowerPoint-Live "content-sharing session" that the 2026-11-05 client never opens for a
  screen share;
- offer the camera `sendrecv`;
- add signaling feedback;
- replay the 2026-08-06 "reserve at the join" layout on today's fixed stack.

It then climbs to the client's two real meeting shapes: non-reinviteless, and reinviteless with
`mediaDescriptions` plus the `updateMediaDescriptions` door.

Whichever step first gets a picture to the other install in the pair rig becomes the default, after
the user has also checked by ear that audio still works. Then three more things follow:

- **Honest:** one press makes one outward act, and in a meeting the control stays "pending" until
  the meeting's own roster shows the stream.
- **Safe:** no share control where the browser cannot capture a screen, and the capability is
  checked before anything outward happens.
- **Good:** a text hint and bounded capture for the screen, encodings driven by the meeting's
  `controlVideoStreaming`, and pause/resume.

## 2. Where we are today

Tree at `c941850` (master).

**The page (web/src/lib/call-media.ts)**

- An outgoing call or a join offers audio only. Reserving sections was reverted, and the reason is
  in a comment: `web/src/lib/call-media.ts:353-363`.
- `LocalSenders.reserve` exists and nothing calls it (`call-media.ts:696-712`). It uses
  `RESERVED_DIRECTION = "recvonly"` (`:521`) and `CONFERENCE_CAMERA_CHANNELS = 2` (`:506`).
  **Two camera channels is a guess**, read from the gap in the service's own mids (NATIVE-CALLING.md
  § 10.3a: "the two main-video mids … are inferred, not seen").
- A capture is added as a new section, or reuses an idle one, always `sendonly`
  (`call-media.ts:785`, `:789`).
- `noteAccepted` counts only `currentDirection === "sendonly"` (`:827-830`). `stop` sets an idle
  section to `inactive` (`:853`).
- Every remote video track becomes a tile, whatever its mid (`call-media.ts:325-336` →
  `RemoteVideoTracks.add`, `:931`).
- The screen is captured with `getDisplayMedia({video:{frameRate:{ideal:15}}, audio:false})`
  (`:591`). It carries no content hint.

**The store (web/src/lib/store.ts)**

- `setSending` has **no in-flight guard** (`store.ts:2917`). A screen press calls
  `callStartSharing` first, before the capture and before any capability check (`:2927`).
- Offer modalities are `["audio", ...kinds.map(modalityFor)]` (`:3009`). Every answer declares
  `["audio","Video","ScreenViewer"]` (`:2733-2737`).
- A join opens media with no `oneToOne` (`:3085`). An answered call does not pass it either
  (`:3098-3116`). A placed call passes `prepared.one_to_one` (`:3049`). An incoming invite from a
  group call is still `CallKind::Call` (`src/bin/server.rs:14983-14985`), so `kind` cannot tell a
  1:1 from a group call.

**The backend (src/calling.rs, src/bin/server.rs)**

- The capability masks are `73463` / `63928042` (`src/calling.rs:41-42`), sent on the invitation
  (`:1268`) and the join (`:1796`).
- **Where the mask came from:** it was copied from "a request the service accepted" (`fc43c1e`).
  The only browser capture of a real client is § 2.5's, an **anonymous guest** in the lobby
  (NATIVE-CALLING.md:287-288, :341-343). So bit 512 speaks for the guest ring's ECS, not for the
  signed-in user's.
- `MediaContent` is `{blob, contentType}` and nothing else (`calling.rs:217-248`). Its `json()`
  feeds all of these: attach (`:1333`), acceptance (`:1343`), media answer (`:1357`), media offer
  (`:1390`/`:1405`), start-outgoing (`:1444`), invitation (`:1237`) and join (`:1764`, reached
  through `join_meeting`, `:1864`/`:1875`). `examples/meeting_join_probe.rs:134,149` also calls
  `join_meeting`.
- `/call/updateMediaDescriptions/` and `/call/controlVideoStreaming/` are published as OUR
  callbacks (`calling.rs:115-116`, `:1145-1147`, `:1181-1184`). Nothing ever POSTs to the
  service's `updateMediaDescriptions` link. It is one of the 13 links measured on the
  `callAcceptance` frame (NATIVE-CALLING.md:1125-1129), merged by `handle_call_frame`
  (`server.rs:14682`). `Links` has no accessor for it (`calling.rs:396-445`).
- `RosterStream` parsing **drops every stream with no `sourceId`** (`calling.rs:2111`).
  `call_state.publishing` filters out this endpoint (`server.rs` `CallSession::json`, around
  `:2359-2378`). There is no "our own streams" view.
- `OUTWARD_METHODS` has 23 entries (`server.rs:367`). `call_start_sharing` replaces a session it
  already holds rather than refusing (`server.rs:8042`). `call_offer_media` prefers
  `mediaRenegotiation` and handles an inline answer (`server.rs:8211-8306`).
- `can_send_media` requires Connected and the `mediaRenegotiation` link (`server.rs:2396-2397`).
- The texts at `server.rs:7411` and `:7517` tell the user to "turn it on in Settings". Calling has
  no switch, so both are stale.
- `every_gated_method_is_named_in_the_automation_guard` (`server.rs:17954-17970`) checks that a
  method appears ONCE in the hook. The hook's script scan has TWO alternations
  (`.claude/hooks/guard-live-automation.sh:981`), so the pin cannot see a method missing from one
  of them. (All 69 gated methods are in both today, checked for this document.) The behavioural
  gate loop covers only place, accept, hangup and mute (`server.rs:17138`).

**The UI (web/src/components, web/src/lib/call-stage.ts)**

- The own-screen preview is `object-cover` (`web/src/components/call-video.tsx:102`). That breaks
  the module's own rule that "a screen is FITTED and a face is CROPPED" (`:24-25`).
- The share control is drawn whenever `can_send_media` is true (`call-stage.tsx:334-354`). Its
  takeover hint is `shareTakeoverHint` (`web/src/lib/call-stage.ts:266-272`).

**The mock and the drivers**

- The mock refuses a second `call_start_sharing` (`web/mock/server.ts:10527-10531`), which the
  backend no longer does. `web/e2e/calling.spec.ts:800-808` relies on that refusal as its
  session-leak detector.
- `web/scripts/call-live.ts:138-146` launches with no desktop-capture flags. `join-live.ts:472-487`
  has them. The pinned colleague consented to being **rung** (`call-live.ts:60-73`).
- No `--pair` run has ever happened. It needs both installs up (NATIVE-CALLING.md:859-925). Since
  the glare, door and `clientContentForMediaController` fixes (`a357c18`, 2026-09-05) and the
  x-ssrc-range fix (`27b040b`, 2026-09-04), no send has been tried against the tenant.

**What has been measured against the tenant** (NATIVE-CALLING.md § 8b, § 10.8):

- every mid-call video section, camera and screen, comes back `port=0`;
- reserving three `recvonly` sections at the join (2026-08-06, before `27b040b`, while the
  x-ssrc-range bug claimed 652 million SSRCs on every video section) was rejected and poisoned the
  receive path;
- taking the content-sharing session displaced the presenter. **That presenter was the user's own
  other device in real Teams** (NATIVE-CALLING.md:1583-1587). CLAUDE.md § Video in a meeting calls
  it "a colleague's real share", which is wrong.

## 3. Gaps covered, deferred, rejected

Covered (from `w1/ws/video-send.json`):

- `meeting-send-reinviteless`
- `camera-direction-sendrecv`
- `one-to-one-video`
- `video-sdp-mediacontent-parity`
- `inbound-vdclc-sender-control`
- `screen-share-send-quality`
- `screen-share-pause-resume`
- `own-screen-preview-cropped`
- `share-button-without-capture-support`

Two defects found during review are covered too:

- **capture toggles with no in-flight guard** (map open issue);
- **the mock's `call_start_sharing` diverging from Rust.**

| Gap | Decision | Why |
| --- | --- | --- |
| `camera-simulcast` | **Deferred** until a ladder step gets a picture through | The screen is never simulcast in the client and is rejected exactly like the camera, so simulcast is not the common cause. It is large SDP work: rids, `a=simulcast`, the MS envelope, and the 100-wide `x-ssrc-range` that comes with simulcast. Desktop Chromium does enable it: `Cte` merges `video:{layerScaleFactors:[1,2],enableLocally:!0,…}` under `isFormFactor("Desktop")`. So NATIVE-CALLING § 10.3a's "Simulcast is off" is a reading of the base settings only, and must be corrected. The earlier "R7: 100-wide range without simulcast" proxy is **dropped**: without simulcast the client's own `getSsrcRangeForIndex` gives 0 width, so a 100-wide range alone is nobody's shape. |
| `camera-background-effects` | **Deferred** | It needs a segmentation model shipped in the app (`calling-webcv` is WASM). Fetching one from a CDN would break the rule against third-party requests. It is worth nothing until a picture arrives. If ever built it must fail closed: a failed blur turns the camera off, never on raw. |
| `give-take-control` | **Rejected** | The control channel is an SCTP data channel this app never opens (x-data is stubbed at port 0). Being controlled needs Edge's `EdgeControlInjector`. Controlling a colleague's machine is the most invasive act in a meeting. `allowScreenSharingControl(){return this.screenSharingControl?.isScreenSharingControlEnabled()&&this.allowDataChannel()}`. |

## 4. Protocol

### 4.0 Building blocks every step uses

#### 4.0.1 The experiment switch (closed list, driver-only)

- **Carried by:** `call_prepare` takes `experiments: string[]` (it is already a `MACHINE_METHODS`
  entry). `calling::experiments::parse` validates the list and the backend stores it on the
  `CallSession`, echoes it in the response and journals one line:
  `[calling] call experiments: [...]`.
- **Refused, never ignored:**
  - an unknown name;
  - a name that does not apply to this kind of call (a 1:1 name on a join, for example);
  - a broken combination rule;
  - any experiment on the ANSWERING branch (`call_prepare` with `call_id`), because no sanctioned
    driver answers calls.
- **Advertised:** `call_status` / `call_state` carry `known_experiments` (the static list), so a
  driver can find out BEFORE it joins anything whether a backend knows a switch (§ 9, `--check-only`).
- **Set by:** the page reads the list from `localStorage["teams-lite.call-experiments"]` (a JSON
  array) in `startCall` and `joinMeeting` only. Only the sanctioned drivers write that key, through
  `context.addInitScript` in their own ephemeral browser contexts.
- **Visible:** every page's stage subtitle says "experimental calling" when `call.experiments` is
  non-empty.

| Name | Applies to | What it changes | Rule |
| --- | --- | --- | --- |
| `no-content-sharing-session` | screen, any call | A screen press skips `call_start_sharing` (no `addModality` contentSharing POST). | — |
| `no-reinviteless-capability` | meeting join | `endpointCapabilities: 72951` (73463 without 512). | excludes `reinviteless` |
| `camera-sendrecv` | camera | The camera's section goes out `sendrecv`, not `sendonly`. | — |
| `video-signaling-fb` | our OFFERS | `a=x-signaling-fb:* x-message app send:src recv:src,vc` on every live video section. | — |
| `join-reserve` | meeting join | The first offer reserves camera channels plus one sharing section, all `recvonly`. | — |
| `join-reserve-ten-cameras` | meeting join | 10 camera channels instead of 2 (inferred from the 13-section capture, § 4.6). | requires `join-reserve` or `reinviteless` |
| `join-screenviewer` | meeting join | The join's `callInvitation.callModalities` is `["audio","ScreenViewer"]`. | — |
| `reinviteless` | meeting | The client's reinviteless shape (§ 4.7-4.10). Implies the `join-reserve` layout. | excludes `no-reinviteless-capability` |
| `reinviteless-all-cameras` | meeting | Every camera channel is reinviteless, not just the first. | requires `reinviteless`; excludes `reinviteless-sharing-only` |
| `reinviteless-sharing-only` | meeting | Camera cap 0: only sharing is reinviteless, and the camera goes by renegotiation on its reserved mid. | requires `reinviteless` |
| `track-before-update` | meeting | The client's order: `replaceTrack` before the update POST (§ 4.8). | requires `reinviteless` |
| `modalities-parity` | any call | `callModalities` derived from directions, as the client does (§ 4.11). | — |
| `bw-seed` | offers and answers | `mediaContent.applyChannelParameters` with a send-side seed (§ 4.12). | — |
| `one-to-one-layout` | outgoing 1:1 | First offer: audio + `main-video inactive` + `applicationsharing-video inactive`, with no codec filter (§ 4.13). | — |
| `one-to-one-required-features` | outgoing 1:1 | `mediaContent.requiredFeatures: "nonByPass"`. | — |

The list is spelled once in Rust and once in TypeScript. A cross-process test (`include_str!`)
pins the two together, and the same test pins the combination rules.

#### 4.0.2 The layout comes from the SDP the backend relays, not from a client-supplied map

The backend already relays the join offer, labels included (`a=label:` written by `toMsSdp`), and
it receives the answer on the `mediaAnswer` frame. A new `calling::media_section_list(sdp) ->
Vec<Section {kind, mid, label, direction, rejected}>` is the structured form, and the existing
`media_sections` becomes a projection of it. The session keeps two things:

- `layout` = our offered video sections in order, kept only if the answer gave them a non-zero port;
- `reinviteless_mids` = those whose OFFERED direction was `sendrecv` while `reinviteless` is on.
  The page writes that direction (§ 4.7), so the cap rule is spelled once, in the page, and the
  backend just reads it.

#### 4.0.3 Confirmation signals (S-levels)

| Level | Signal | Where it is read |
| --- | --- | --- |
| S1 | The answer gives our labelled section a non-zero port. **Weak:** an accepted section with no bytes is the known open failure. | backend journal (`media_sections`), page `noteAccepted` |
| S2 | The SFU sends RTCP receiver reports for our outbound video SSRC (`remote-inbound-rtp`). | sender `getStats` (driver) |
| S3 | Our OWN endpoint's roster stream for that label is in a sending direction. Under reinviteless sharing it must also carry `mdRequestId == last_sharing_md_request_id`. | backend `own_streams` / `sending_confirmed` |
| S4 | `/call/controlVideoStreaming/` frames reach the sender. | backend journal, `framePaths` |
| S5 | The receiver's `publishing` lists the sender endpoint's stream, and a section is offered for it. | receiver `call_state` (driver) |
| S6 | Receiver `framesDecoded > 0`. **Depends on the video-receive workstream**, because the receive path still delivers 0 bytes on subscriptions. | receiver `getStats` (driver) |

`sending_confirmed` (backend fact, all pages agree) is S3 alone. A 1:1 has no roster, so it has no
`sending_confirmed`. That is a stated limit (§ 7).

### 4.1 R0: today's protocol, measured with a second participant

Nothing on the wire changes. This is the baseline every later step is compared with, and it has
never been taken with the pair (§ 2).

- **Method / link / body:** unchanged. `call_offer_media` posts `mediaNegotiation`
  (`calling.rs:1405`) to `mediaRenegotiation`.
- **Callbacks:** `/call/mediaAnswer/`, `/call/rejection/`.
- **Status:** **measured** only alone in the meeting. The pair run is new.

### 4.2 R0b: a screen share without the content-sharing session (`no-content-sharing-session`)

- **Method:** the screen goes out through the existing `call_offer_media` only. `call_start_sharing`
  is not called.
- **Link / body:** as R0. Modalities are `["audio","ScreenSharer"]`.
- **Bundle evidence** (read-from-bundle). `startScreenSharing` acquires the screen, enables the
  modality and renegotiates. It **never** calls `startContentSharingAsync`:

  ```js
  this.getScreenHandle(t).acquire().then(()=>(l.logSuccess("sharing stream acquired"),
    this.mediaStateConfigurationHelper.enableModality(2),
    this.allowDataChannel()&&this.mediaStateConfigurationHelper.enableModality(3), …))
  .then(()=>this.updateMediaModalities(void 0,n,r,a))
  ```

- **Whose session it is:** the session belongs to class `Uo`. Its props include `moveSlide`,
  `startPointerMode`, `requestControl` and `grantControl` — **PowerPoint Live**, not a screen. It is
  created only by `createContentSharingSession` (an app-level API that nothing in `calling-app.js`
  calls) or by `onContentSharingStarted` for a session somebody else started:

  ```js
  async createContentSharingSession(t,i,r,n,s=Ie()){ … const a=new Uo(this.callTelemetry,this.logger,this.signalingSession,t,i,n,r); …
  ```

- **Status:** read-from-bundle. NATIVE-CALLING.md:1506 says "The client's own path is
  `startScreenSharing` → `startContentSharingAsync`". That cites the August bundle
  (`calling-pluginless-779e39a54b8bcd49.js`), which can no longer be re-read. The 2026-11-05 bundle
  says otherwise.
- **Why it comes first:** it is the cheapest step. The session is also the likely cause of the
  measured displacement: our endpoint becomes a PPT-style presenter, and the real presenter's VBSS
  section is zeroed (inferred).

### 4.3 R1: the capability mask without bit 512, joins only (`no-reinviteless-capability`)

- **Method / link:** POST `call_join` to `calling_conversationServiceUrl` (`calling::endpoints`,
  `calling.rs:174`).
- **Body:** today's `join_payload` with `"endpointCapabilities": 72951`. Nothing else changes.
  `clientEndpointCapabilities` stays `63928042`.
- **Who decides which bit** (README § 3, collision 9). This ladder decides bit 512 (reinviteless);
  incoming-and-conversation decides bit 16 (`brokerEnabledIncoming`, cleared on its WV attach,
  `73447`). Both read ONE constant set in `calling.rs`: the captured mask plus named bit constants,
  so no body spells a mask of its own. If R1 wins and is promoted, the attach drops bit 512 too, or
  its design says why it keeps it.
- **Bundle evidence:** the client ORs 512 into every body it builds — attach (`WV`), callAcceptance
  (`qV`), the outgoing invitation and the join (`gM`):

  ```js
  (this.getMaxReinvitelessMediaForVBSSForWeb()||this.getMaxReinvitelessMediaForVideoForWeb())&&(this.telemetryHelper.setReinviteless(this.callUsesMixer),e=e|512),e}
  ```

- **But a session is reinviteless only in a MIXER call:**

  ```js
  isReivitelessEnabled(){return(this.signalingSession.getMaxReinvitelessMediaForVideoForWeb()>0||this.signalingSession.getMaxReinvitelessMediaForVBSSForWeb()>0)&&this.callUsesMixer}
  ```

  and the call layer passes only the `*Multiparty` keys to `v8`:

  ```js
  const a={maxReinvitelessMediaForVBSSMultiparty:this.signalingSession.getMaxReinvitelessMediaForVBSSForWeb(),maxReinvitelessMediaForVideoMultiparty:this.signalingSession.getMaxReinvitelessMediaForVideoForWeb()};
  ```

- **What follows:**
  - A 1:1 declares 512 and sends no `mediaDescriptions`. That is the client's own 1:1 shape, so
    **this switch is meetings only**, and a 1:1 invitation keeps 73463.
  - The hypothesis is narrower: in a MEETING, declaring 512 and then sending no `mediaDescriptions`
    is nobody's shape (inferred).
- **Expected non-result:** the mask was copied precisely because joins earned `400` with an empty
  body (`fc43c1e`; `calling.rs:34-40`: "Revisit them together with the join payload, never alone").
  An empty-body 400 here teaches nothing. If it happens, R1 runs again only combined with the R4
  layout (R4c).
- **Status:** read-from-bundle, plus the history above.

### 4.4 R2: the camera offered `sendrecv` (`camera-sendrecv`)

- **Method / link:** the existing `call_offer_media` → `mediaRenegotiation` (fallback
  `startOutgoingNegotiation`).
- **Body:** unchanged `mediaNegotiation {callModalities, sender, links {mediaAnswer, rejection},
  mediaContent}` (`calling.rs:1405`). Only the SDP changes: the camera's `m=video` carries
  `a=sendrecv`.
- **Bundle evidence:**

  ```js
  calculateVideoDirection(t,i){ … return t&&(n=n&&(t==="sendrecv"||t==="sendonly"),s=s&&(t==="sendrecv"||t==="recvonly")),n?"sendrecv":s?"recvonly":"inactive"}
  createSender(e,t,i){ … s=i!==B.MODALITY.sharing||n?.getExtension("reinviteless")?B.MEDIA_STATE.sendReceive:B.MEDIA_STATE.send;return r.direction=s
  ```

  Video is never `sendonly` in the client. Only a non-reinviteless sharing section is.
- **Side effect:** a `sendrecv` section also receives, so the SFU may put a stream on it. A track
  event on one of OUR send mids is kept out of `RemoteVideoTracks` (§ 5). Which source to draw there
  is the video-receive workstream's decision.
- **Status:** read-from-bundle.
- **Callbacks:** `/call/mediaAnswer/`, `/call/rejection/`.

### 4.5 R3: signaling feedback on our OFFERS' live video sections (`video-signaling-fb`)

- **Body:** inside the `call_offer_media` (and join) blob, on every `m=video` with a non-zero port:
  `a=x-signaling-fb:* x-message app send:src recv:src,vc`. Audio is untouched (its
  `app recv:dsh` twin is left out, to keep the verified section byte-for-byte).
- **Bundle evidence:**

  ```js
  r.port!==0&&(r.type===B.MEDIA_TYPE.audio&&this.context.configProvider.config.webrtcAudioChannelSignalingFeedback?r.signalingFbXMessage={payload:"*",param:…}:r.type===B.MEDIA_TYPE.video&&this.context.configProvider.config.webrtcVideoChannelSignalingFeedback&&(r.signalingFbXMessage={payload:"*",param:…}))
  webrtcAudioChannelSignalingFeedback:"app recv:dsh",webrtcVideoChannelSignalingFeedback:"app send:src recv:src,vc"
  ```

- **A labelled DEVIATION:** the client's `toMsSdp` checks `port!==0`, not offer versus answer, so it
  adds the line to answers too. Teams-lite adds it to offers only, because every answer addition
  this app ever tried earned `SdpParsingFailure` and ended the call (NATIVE-CALLING.md § 10.8,
  "The answer is left alone").
- **Status:** read-from-bundle. The audio twin is **measured** in the guest capture
  (NATIVE-CALLING.md:304).

### 4.6 R4: the reserved meeting layout, one variable at a time

R4 is four sub-steps, each adding one switch to the previous one:

| Step | Switches | What it isolates |
| --- | --- | --- |
| R4a | `join-reserve` | The 2026-08-06 shape (recvonly reserved sections, 512 kept, `["audio"]`) replayed on today's stack. Since then, `27b040b` (sane `x-ssrc-range`) and `a357c18` (glare refusal, `mediaRenegotiation` door, `clientContentForMediaController`) have landed. The 2026-08-06 reserved sections carried the 652-million-SSRC range. |
| R4b | + `join-screenviewer` | The client's join modalities. |
| R4c | + `no-reinviteless-capability` | Together with R4b, the client's whole non-reinviteless shape. |
| R4d | + `join-reserve-ten-cameras` | The client's channel count, as best inferred. |

**Method / link:** POST `call_join` to `calling_conversationServiceUrl`.

**Body** (R4c shown):

```jsonc
// join_payload as today, plus:
"endpointCapabilities": 72951,                     // R4c only
"callInvitation": {
  "callModalities": ["audio", "ScreenViewer"],     // R4b+
  "links": { /* unchanged */ },
  "mediaContent": { "blob": "<SDP>", "contentType": "application/sdp-ngc-1.0" },
  "clientContentForMediaController": { /* unchanged */ }
}
```

SDP sections:

- mid 0: audio, `sendrecv`
- mids 1..N: `main-video`, `recvonly`. N = 2, or 10 under R4d.
- mid N+1: `applicationsharing-video`, `recvonly`

Each reserved video section uses the conference codecs (H264, AV1, rtx) and its label.

Turning a capture on reuses the reserved section through the existing renegotiation, with no new
m-line:

- **camera:** mid 1 goes to `sendrecv` under `camera-sendrecv`, otherwise `sendonly` (R4a replays
  `sendonly`). The track is attached. Modalities are `["audio","Video"]`, plus `"ScreenViewer"`
  under `modalities-parity`.
- **screen:** the sharing mid goes to `sendonly`. The session request follows `no-content-sharing-session`.

**Bundle evidence:**

```js
addModalities(e){const t=this.mediaManager.isEmpty();!this.isMultiparty&&t&&!this.isPstnCall&&(e.video=e.video||"inactive",e.sharing=e.sharing||"inactive"), …
  case B.MODALITY.video:n=this.numVideoChannels;break;case B.MODALITY.sharing:n=this.numVbssChannels;break; …
  (0,qh.times)(n,s=>{const a=this.mediaManager.createMediaEntity(i);this.configProvider.config.reinvitelessEscalationFix?a.setExtension("reinviteless",this.reinvitelessContext.maxStreamsForModality[i]>s): …
setDirections(e){const t=!yn(e);for(const i of aS)for(const[r,n]of this.getTransceiversByModality(i).entries()){if(this.findEntityForTransceiver(n)?.getExtension("reinviteless")&&t){n.direction="sendrecv";continue}const o=e[i]||B.MEDIA_STATE.inactive;n.direction=r===0&&n.currentDirection?o:d3(o), …
function d3(e){let t=e;return bi(e)&&(t=jg(e)),t||B.MEDIA_STATE.inactive}
calculateSharingDirection(t,i,r){return … this.mediaStateConfigurationHelper.isSending(2)?"sendonly":this.isSomeoneSharing||this.callUsesMixer||this.isEscalationInProgress?"recvonly":void 0}
getSignalingMediaTypes(t){ … t.sharing===…send?r.push("ScreenSharer"):t.sharing===…receive&&r.push("ScreenViewer"),r}
```

With `callUsesMixer`, video and sharing are `recvonly`. They are therefore present (`!e[i]` is
false), so a meeting's FIRST offer carries the sections. That fits the measured 13-section BUNDLE on
an audio-only join (NATIVE-CALLING.md:300). It corrects NATIVE-CALLING.md:1454-1460 ("A CONFERENCE
really does add sections mid-call").

**Channel count:** `numVideoChannels = isMultiparty && numVideoChannelsGvc || 1`. The default
`numVideoChannelsGvc` is not a literal in the bundle (`Cte` sets 17 only under an unrelated
condition, `qg()==="36"`). If the 13 sections are audio + N video + sharing + data, then N = 10
(inferred). The "2" today comes from the service's own mids in its offers to an audio-only endpoint.
That may be the service's numbering for sections it adds, not the client's layout, so it is a guess
too. R4d tests the other guess.

**What a reserved section does when the camera goes off:** the client maps an idle non-reinviteless
section through `d3`, so it goes back to `recvonly`, never `inactive` (§ 5, `stop`). `noteAccepted`
counts `sendonly` and `sendrecv` (§ 5).

**Status:** read-from-bundle. The 2026-08-06 failure is **measured**, but on a different stack.

**Callbacks:** none new.

### 4.7 R5a: the reinviteless meeting join (`reinviteless`)

- **Method / link:** POST `call_join`.
- **Body:** as R4 (with `join-screenviewer` when stacked). `endpointCapabilities` stays 73463.
  `mediaContent` gains `mediaDescriptions`:

  ```jsonc
  "mediaContent": {
    "blob": "<SDP>",
    "contentType": "application/sdp-ngc-1.0",
    "mediaDescriptions": {
      "descriptions": [
        { "mid": "1", "direction": "recvonly" },   // first camera channel (cap 1; all under reinviteless-all-cameras; none under reinviteless-sharing-only)
        { "mid": "3", "direction": "recvonly" }    // the sharing section
      ],
      "requestId": 1
    }
  }
  ```

  - The descriptions list **only the reinviteless entities**. Audio is not reinviteless. Data is
    included in the client only when it has offerable data subtypes, and this app has none.
  - `subStreamIndex` is omitted (undefined without multistream).
  - `negotiationTag` is omitted inside `mediaDescriptions`, because the caller passes none on a join.
    It could also appear at the `mediaContent` level (`t&&(u.negotiationTag=t)`); for a join from
    the UI that is undefined (inferred).
- **The SDP for a reinviteless section:**
  - a=`sendrecv` with no track;
  - `a=msid:- <index>`, where index is the m-line index (the client's `Dj` fixup — not the mid,
    although here the two coincide);
  - non-reinviteless reserved sections stay `recvonly`;
  - audio is byte-for-byte unchanged.
- **Bundle evidence:**

  ```js
  this.reinvitelessContext.enabled&&(this.mediaManager.updateMediaEntitiesWithLocalTracks(),u.mediaDescriptions=this.getMediaDescriptions(this.offeredModalities,void 0,void 0)), … t&&(u.negotiationTag=t),this.callbacks.setBWSeed(u)
  getMediaDescriptions(e,t,i,r=!1){ … if(this.configProvider.config.reinvitelessEscalationFix&&v.isDisabled()||!v.getExtension("reinviteless")&&!_)continue; … if(!b||b===B.MEDIA_STATE.inactive){this.configProvider.config.enableDisabledMediaFixupV2!==!1&&(o.push({mid:v.getMid(),direction:B.MEDIA_STATE.inactive}), …);continue} …
    const y={mid:v.getMid(),direction:!v.getLocalTrackId()&&bi(b)?jg(b)||B.MEDIA_STATE.inactive:b,subStreamIndex:v.getSubstreamIndex()};v.getLocalTrackId()&&bi(b)&&(y.label=rS(v.getModality())), … return c&&this.setLastSharingMdRequestId(m),{descriptions:o,negotiationTag:t,requestId:m}}
  getNextMediaDescriptionsRequestId:()=>(this.mediaDescriptionsRequestIdState.current+=1,this.mediaDescriptionsRequestIdState.current)
  !t.msid&&(a||r.getExtension("reinviteless"))&&(t.msid=`- ${s??i}`)   // inside e.media.forEach((t,i)=>…), sdpModifiers.enableLocalAddMsidFixup:!0
  ```

  The base settings have `enableDisabledMediaFixupV2:!1`, and `Cte` does not override it, so
  inactive entries are not pushed.
- **Known differences from the client, recorded rather than closed:**
  - the client's offer probably carries a data section (inferred from 13 sections);
  - its offer carries `applyChannelParameters`, because `setBWSeed` defaults to on — that is
    `bw-seed`, stackable as R5a′;
  - the channel count may differ (R4d / `join-reserve-ten-cameras`).
- **Safety rail** (§ 9): the R5a SDP says `sendrecv` on trackless sections. If the service, or a
  colleague's client reading the roster, trusted the SDP over the descriptions, the roster could
  show the user as publishing. The driver checks for that and aborts before any press.
- **Status:** read-from-bundle. Never sent.
- **Success signal (inferred):** the answer keeps the reserved mids at non-zero ports, our own
  roster streams carry `mdRequestId: 1` and a receive direction, and nothing shows us as sending.
- **Callbacks:** none new. `/call/updateMediaDescriptions/` is already published in the
  acknowledgement (`calling.rs:1146-1147`). The client spells it with no trailing slash
  (`UPDATE_MEDIA_DESCRIPTIONS:"/call/updateMediaDescriptions"`). That is harmless; arrivals are
  logged by path.

### 4.8 R5b: camera on with no SDP — the new RPC `call_update_media_descriptions`

- **Method:** POST, `OUTWARD_METHODS`.
- **Link:** the service's `updateMediaDescriptions`, from the `callAcceptance` frame (measured to
  exist, NATIVE-CALLING.md:1125-1129). The client stores it there:
  `this.links[I.LINKS.UPDATE_MEDIA_DESCRIPTIONS]=e.callAcceptanceAcknowledgement.links.updateMediaDescriptions`
  (incoming), and from `callAcceptance` (outgoing).
- **Params, built from PRIMITIVES:** `{call_id, sending: ["camera"] | ["screen"] | ["camera","screen"] | []}`.
  The page never supplies a mid, a direction or a label.
- **What the backend builds from its layout** (§ 4.0.2):
  - the first reinviteless camera mid is `sendrecv` with label `main-video` when the camera is on,
    otherwise `recvonly`;
  - other reinviteless camera mids are `recvonly`;
  - the reinviteless sharing mid is `sendonly` with label `applicationsharing-video` when the screen
    is on, otherwise `recvonly`.
- **Body on the wire** (the client's `TG` after its transport strips `payload`, NATIVE-CALLING.md § 2.3):

  ```json
  {"UpdateMediaDescriptions":{"mediaDescriptions":{
    "descriptions":[{"mid":"1","direction":"sendrecv","label":"main-video"},
                    {"mid":"3","direction":"recvonly"}],
    "requestId":2}}}
  ```

  The backend owns `requestId`, as a per-call pre-increment counter starting after the join's 1.
- **Order — POST FIRST by default, and that is a deliberate deviation:**
  1. The page calls the RPC.
  2. The backend passes every check BEFORE the network (dispatch gate, phase, reinviteless, link,
     layout), POSTs, and on 2xx returns the mid it marked sending for each kind.
  3. The page verifies those mids are its own reserved ones, and only then calls `replaceTrack`.

  The client does the opposite (`startMediaDescriptionsUpdate` → `streamSendersManager.activate`,
  then the POST). In this app's page/backend split, that order would send camera RTP to Microsoft's
  media server before the write lock had been checked, because the section is already negotiated
  `sendrecv` over a live bundle. Under `track-before-update` the client's order is used, and only
  in driver contexts.
- **Rollback:**
  - on any refusal, nothing was attached (POST-first) and the capture is released;
  - on a mid mismatch, the page posts the recvonly update and releases;
  - under `track-before-update`, `replaceTrack(null)`, as the client's
    `rejectMediaDescriptionsUpdate` does.
- **Glare:** `outgoing_negotiation_at` is NOT set. There is no SDP in flight, so the service's own
  renegotiations are still answered.
- **Bundle evidence:**

  ```js
  function TG(e,t){return st(e,"signalingSession cannot be null"),{payload:{UpdateMediaDescriptions:{mediaDescriptions:t}}}}
  this.updateMediaDescriptionsAsync=(a=Ht(),o)=>{ … Ni(this.fsmState===I.SIGNALING_FSM_STATE.CONNECTED,"updateMediaDescriptions operations are only allowed when the call is connected");const l=this.links[I.LINKS.UPDATE_MEDIA_DESCRIPTIONS]; … this.http.sendPostRequest({url:l,requestName:dt.UPDATE_MEDIA_DESCRIPTIONS.name,payload:TG(this,o),causeId:a})
  async updateMediaDescriptions(t,i,r){ … const s=await this.mediaSession.startMediaDescriptionsUpdateAsync(t,i,r); … await this.signalingSession.updateMediaDescriptionsAsync(t,s);const a=await this.mediaSession.completeMediaDescriptionsUpdateAsync(t); … }catch(s){ … const a=await this.mediaSession.rejectMediaDescriptionsUpdateAsync(t);
  async startMediaDescriptionsUpdate(e,t,i){ … this.callbacks.updateLocalMediaSources(),await this.streamSendersManager.activate(e),this.getMediaDescriptions(this.reinvitelessAppliedModalities,t,i,!0)}
  ```

- **`filterUnnegotiatedMidsInMediaDescriptions:!1` is the client's default**, so the client keeps
  sending mids the server never negotiated. This app refuses an update that would name a mid the
  answer zeroed. That rule is this app's safety rail, not the client's: such a section is stopped in
  the browser and can carry nothing.
- **Status:** read-from-bundle. Never sent. The link's existence is measured. The response body, and
  whether the service ever POSTs to our callback, are unknown, and both are journaled by key.
- **Callbacks:** the result arrives on `/conversation/rosterUpdate/` (S3: our own main-video stream
  turns sendrecv). Possibly also `/call/controlVideoStreaming/` (S4).

### 4.9 R5c: screen on, reinviteless

- **Run as:** `reinviteless,no-content-sharing-session`, which is the client's shape. Plain
  `reinviteless` (with the session) is a variant, run only if the first fails.
- **Method:** `call_update_media_descriptions` with `sending` including `screen`.
- **Body:**

  ```json
  {"UpdateMediaDescriptions":{"mediaDescriptions":{
    "descriptions":[{"mid":"1","direction":"recvonly"},
                    {"mid":"3","direction":"sendonly","label":"applicationsharing-video"}],
    "requestId":k}}}
  ```

  The backend stores `last_sharing_md_request_id = k` for EVERY update that carries the sharing
  entry, `recvonly` included, as the client does.
- **Bundle evidence:**

  ```js
  i&&v.getModality()==="sharing"&&(y.replacementContext=i),c=c||v.getModality()==="sharing"   …   return c&&this.setLastSharingMdRequestId(m)
  s=(r?.mediaStreams??[]).filter(c=>n(c.type)&&c.mdRequestId===i),a=s.some(c=>c.direction===Ns.send||c.direction===Ns.sendReceive);s.some(c=>c.direction===Ns.receive)&&!a&&…this.declareSharingStolen(…)
  ```

- **Displacing a presenter:** a sharing send while somebody else presents is allowed, and it is
  journaled exactly as `call_start_sharing` journals it (`server.rs:8055`, `:8089`). The design's
  earlier rule "a sharing send requires the session held" is **dropped**, because the client opens
  no session for a screen (§ 4.2).
- **Status:** read-from-bundle. The roster's per-stream `mdRequestId` field is **measured** to exist
  (NATIVE-CALLING.md:1684, seen as 0).
- **Callbacks:** `/conversation/rosterUpdate/`; `/conversation/contentSharingUpdate/` and
  `/conversation/contentSharingEnd/` (already handled).

### 4.10 Answering the service's renegotiation while reinviteless

- **Method / link:** the existing `call_answer_media` → the offer frame's own `mediaAnswer`
  (`CallSession.renegotiation_answer_link`).
- **Body:** the existing `mediaAnswer` body (`calling.rs:1357`), plus
  `mediaContent.mediaDescriptions` restating the session's current descriptions with the next
  `requestId`. The backend builds them; the page supplies nothing.
- **Before `createAnswer`:** the page re-asserts `sendrecv` on its reinviteless transceivers, as the
  client's `setDirections` does on every negotiation (`syncTransceivers`).
- **Bundle evidence:**

  ```js
  this.reinvitelessContext.enabled&&(_.mediaDescriptions=this.getMediaDescriptions(o,void 0,void 0)),this.offeredDataSubTypes.length&&(_.dataSubTypes=this.offeredDataSubTypes),this.callbacks.setBWSeed(_)
  ```

  (the media session's answer builder; it is not a method named `createAnswerAsync`).
- **Status:** read-from-bundle.
- **Callbacks:** `/call/mediaAcknowledgement/` (existing).

### 4.11 Modality parity (`modalities-parity`)

`callModalities` on join, offer and answer are derived from directions:

- `"audio"` unless audio is inactive;
- `"Video"` ONLY while the camera is sending;
- `"ScreenSharer"` while the screen is sending, otherwise `"ScreenViewer"` when receiving sharing
  (always, in a meeting).

```js
getSignalingMediaTypes(t){const i=n=>n===…send||n===…sendReceive,r=[];return t.audio&&t.audio!==…inactive&&r.push("Audio"),i(t.video)&&r.push("Video"),t.sharing===…send?r.push("ScreenSharer"):t.sharing===…receive&&r.push("ScreenViewer"),r}
```

It reverses `5b355f0`'s "`Video` on every answer" (`store.ts:2733-2737`; `git log -S'"Video"' --
web/src/lib/store.ts` names that commit, 2026-09-04 21:45) under the switch only. On the ANSWER it
is a flag of video-receive's one answer-modalities function (`calling::declarable_modalities`,
video-receive § 4.6; README § 3, collision 8), not a second derivation; on the join and the offer it
calls the same `signaling_media_types` derivation that function is built on. **Promoting it belongs to video-receive** (§ 12).

**Status:** read-from-bundle.

### 4.12 Send-side bandwidth seed (`bw-seed`)

- **Body:** `mediaContent.applyChannelParameters =
  {"multiChannelParameter":{"mids":["*"],"mediaParameter":"{\"sendSideBWSeed\":{\"seedValueBitsPerSec\":600000}}"}}`.
  `mediaParameter` is a JSON string inside the JSON.
- **The value is the client's FIRST-RUN seed.** The client keeps the last measured value in
  `localStorage["bwSeed"]` (`aggregation:"last"`, capped at `15e5`). Separately, `Cte` seeds its own
  allocator with `sendBandwidthSeedValue=2e5`.

```js
setBWSeed(t){ … t.applyChannelParameters={multiChannelParameter:{mids:this.configProvider.config.bwSeedOptions.mids,mediaParameter:JSON.stringify({sendSideBWSeed:{seedValueBitsPerSec:i}})}}
setBWSeed:!0,bwSeedOptions:{mids:["*"],aggregation:"last",aggregateLastN:null,constValue:6e5,minValue:-1,cappingValue:15e5,useLocalStorage:!0,localStorageKey:"bwSeed"}
```

**Status:** read-from-bundle. The value is inferred.

### 4.13 1:1: the client's first-offer layout (`one-to-one-layout`, `one-to-one-required-features`)

- **Method / link:** POST `call_place` → `calling_conversationServiceUrl`.
- **Body:** `invitation_payload` with SDP = audio `sendrecv` + `main-video a=inactive` +
  `applicationsharing-video a=inactive` — one of each, with no codec filter (the client's
  `allowedVideoCodecs` is `[]` for a 1:1). `callModalities` stays `["audio"]`. The mask stays 73463
  (§ 4.3). Under `one-to-one-required-features`, `mediaContent.requiredFeatures: "nonByPass"`.
- **Turning a capture on:** the existing renegotiation, with the reserved mid flipped. The camera
  goes to `sendrecv` with modalities `["audio","Video"]`; the screen goes to `sendonly`.
- **No content-sharing session on a 1:1**, whatever `no-content-sharing-session` says. This was an
  open question before and is answered by § 4.2: `startScreenSharing` opens none.
- **No source requests** on a 1:1. An incoming 1:1 already adopts the peer's sections by label
  (`call-media.ts:347`).

```js
addModalities(e){const t=this.mediaManager.isEmpty();!this.isMultiparty&&t&&!this.isPstnCall&&(e.video=e.video||"inactive",e.sharing=e.sharing||"inactive")
this.numVideoChannels=this.isMultiparty&&this.configProvider.config.numVideoChannelsGvc||1
this.configProvider.mediaConfig.webrtcRequiredFeatures!==""&&!this.sessionContext.config.isConference&&(u.requiredFeatures=…)
async requestSource(t,i,r,n){if(!this.multiParty)return;
```

- **Where the old `inactive` measurement came from:** today's `RESERVED_DIRECTION = "recvonly"`
  (`call-media.ts:521`) was chosen after `inactive` was measured to be zeroed **in a meeting**. That
  says nothing about a 1:1, where the client sends `inactive` (inferred: `setDirections` → `d3`
  keeps `inactive`).
- **Status:** read-from-bundle.
- **Callbacks:** `/call/mediaAnswer/`, `/call/rejection/`.

### 4.14 Inbound `controlVideoStreaming` (vdclc) frames to the sender

- **Inbound POST:** the service POSTs to our `/call/controlVideoStreaming/`, published in
  `clientContentForMediaController` (`calling.rs:1181-1184`) and in the acknowledgement (`:1145`).
- **Body:** `{"controlVideoStreaming":{"sequenceNumber":n,"controlInfo":[{"sourceId":s,"fmtParams":"max-fs=..;max-mbps=..;max-fps=..;max-br=..;ssrc=..;rid=..;KeyFrame=1"} or "fmtParamsList":[...]]}}`.
- **Parse, as the client does:**
  - `maxFps = max-fps/100`;
  - `maxBr = max-br*1200`;
  - `ssrc`, `rid` and `KeyFrame` are read as they are;
  - a frame older than the last one for that source is dropped.
- **Nothing is answered.** The page constrains its sender with `RTCRtpSender.setParameters`
  (`scaleResolutionDownBy` from `max-fs`, `maxBitrate`, `maxFramerate`), clamped to the capture.

```js
handleSignalingMessage(e){const t=e.controlInfo[0].fmtParamsList||[e.controlInfo[0].fmtParams],i={type:"vdclc",sourceId:e.controlInfo[0].sourceId,fmtParamsList:t},r=this.extractSequenceNumber(t)??e.sequenceNumber; …
parseFmtParams(e){ … maxFps:+t.get(…MAX_FPS_PATH)/100,maxBr:+t.get(…MAX_BR_PATH)*1200, …
```

**Status:** read-from-bundle; never observed. Arrivals are counted from M1 on (S4).

### 4.15 Screen capture: content hint and bounds

- **The track:** `contentHint = "text"`, which is the client's `webrtcDisplayStreamContentHint:"text"`,
  applied in `applyVideoState` for `ScreenShare`.
- **The capture:** `getDisplayMedia({video:{width:{max:1920},height:{max:1080},frameRate:{ideal:15,max:15}},audio:false})`.
  `getDisplayMedia` rejects `min` and `exact`, so the client's own min=max resolution table cannot
  be copied literally (inferred: this is the spec's rule).
- **No `degradationPreference`:** the client's `degradationPreference:"maintain-framerate"` is in
  the SAFARI settings class (`_te`), not Chromium's (`Cte`). So none is set here, and the earlier
  draft that called it the client's Chrome behaviour is withdrawn.

**Status:** read-from-bundle.

### 4.16 Screen pause and resume (local only)

- **Pause:** `sender.replaceTrack(frozenTrack)`, where `frozenTrack` is `canvas.captureStream(1)`
  redrawn once a second from the last screen frame, carrying `contentHint "text"`.
- **Resume:** `replaceTrack(screenTrack)`.
- **No signaling** in either direction.

```js
async setVbssPaused(e,t){const i=this.context.callDeviceManager.getScreenShareOutput(); … return i.setPaused(e)}
this.outputStream=this.canvas.captureStream(1),this.outputTrack=this.outputStream.getVideoTracks()[0], … this.outputTrack.contentHint=e.contentHint
```

**Status:** read-from-bundle.

### 4.17 Give and take control (REJECTED)

`takeControl` POSTs `{participants:{from}}` to `contentSharingTakeControl`, and the rest runs over
SCTP:

```js
takeControl(e,t){ … this.signalingSession.http.sendPostRequest({url:this.links[I.LINKS.CONTENT_SHARING_TAKE_CONTROL],payload:iV(this.signalingSession), … withoutTrouter:!0
```

Not implemented (§ 3).

## 5. Changes by file

The milestone tag is in brackets (§ 10).

**src/calling.rs**

- **[M1] Golden pins first.** Every builder gets an exact-JSON test in its default shape, generated
  from today's code BEFORE any refactor: `attach_payload`, `acceptance_payload`,
  `media_answer_payload`, `media_offer_payload`, `start_outgoing_negotiation_payload`,
  `invitation_payload`, `join_payload`. This is the ONE golden fixture set (README § 3, collision
  4): meeting-participation M1's "golden joins" extend it with the new kinds and the
  `suppress_dialout` variant rather than pinning the same bodies a second time.
- **[M1] Own streams.** Add `own_streams_of(roster_member, endpoint_id)`. It reads the same paths as
  `streams_of` but does NOT require a `sourceId`, and it reads `mdRequestId`. `streams_of`, and so
  `publishing` and the receive path, is left untouched.
- **[M1]** `Links::update_media_descriptions()`; `media_section_list` (§ 4.0.2), with
  `media_sections` becoming its projection.
- **[M1] Received content stays inbound.** A separate inbound type, `ReceivedMediaContent {content,
  negotiation_tag, other_keys}`. `parse` keeps the service's `negotiationTag` and names its other
  keys for the journal. Outbound `MediaContent` can never carry a parsed field.
- **[M3]** `pub mod experiments { pub const ALL: [&str; 15]; pub struct Experiments {…}; pub fn
  parse(&[String], CallShape) -> Result<Experiments> }`. `CallShape` is meeting join, outgoing 1:1
  or answering, and it carries the applicability and combination rules.
  `pub const REINVITELESS_CAPABILITY: u32 = 512`.
- **[M3] Shaped builders.** `SignalingShape { modalities, endpoint_capabilities, media_extras }`,
  where `media_extras` holds `mediaDescriptions`, `requiredFeatures` and `applyChannelParameters`.
  New `invitation_payload_shaped` and `join_payload_shaped` take it. The existing
  `invitation_payload`, `join_payload`, `join_meeting` and `place_call` become wrappers passing
  `SignalingShape::default()`, so `examples/meeting_join_probe.rs` and the ~20 test call sites are
  unchanged and the golden pins prove the default is byte-for-byte today's.
  `join_meeting_shaped` / `place_call_shaped` are what `server.rs` calls.
- **[M5]** `media_descriptions_json(entries, request_id)` (no `requestId` on an empty list, as in
  the client); `update_media_descriptions_payload(descriptions)` →
  `{"UpdateMediaDescriptions":{"mediaDescriptions":…}}`; `MediaDescription {mid, direction,
  label}`.
- **[M8]** `VideoControl` + `video_control_from_frame(url, body)`.

**src/bin/server.rs**

- **[M1] Fields.** `CallSession` gains `own_streams: Vec<OwnStream>`, `sending_confirmed:
  Vec<String>` and `from_mixer: Option<bool>`.
- **[M1] JSON.** `CallSession::json` adds `own_streams [{label, direction, md_request_id}]` and
  `sending_confirmed`.
- **[M1] Journal.** `handle_call_frame` (`:14601`) logs `/call/updateMediaDescriptions` and
  `/call/controlVideoStreaming` arrivals by shape (path, keys, never values).
- **[M1] Confirmation.** It also computes `sending_confirmed` from OUR endpoint's streams only (one
  account on two endpoints is the case that matters). It is NOT set from answer acceptance (S1). It
  is recomputed on every `rosterUpdate`.
- **[M1] Stale text.** The two "turn it on in Settings" texts (`:7411`, `:7517`) become "calling
  is not connected yet — try again in a moment".
- **[M2]** Read `fromMixer` from `callNotification.mediaContent` and from the attach answer's
  `mediaContent`. The client sets `this.callUsesMixer=!!e.callNotification.mediaContent?.fromMixer`
  (and `this.callUsesMixer=r.fromMixer` from the attach response). It is recorded and journaled as a
  FACT about the media path, never as a predicate. Whether a call is one-to-one comes from ONE place,
  video-receive's `CallSession::is_one_to_one()` over the service's `isMultiParty` (video-receive
  § 4.8; README § 3, collision 6): the answering branch of `call_prepare` returns `one_to_one` from
  that predicate, as the placing branch already returns it (`:7645`). An unknown call reads as not
  one-to-one, which is today's behaviour (the answering branch returns no `one_to_one` today).
- **[M3] `call_prepare`** (`:7401`): parse `experiments` for the meeting and placing branches, store
  them, echo them, journal them, and refuse them on the answering branch.
- **[M3]** `call_status` / `call_state_payload` (`:14422`) carry `known_experiments`.
- **[M3]** `call_join` (`:7727`) and `call_place` (`:7656`) build their shape from the session's
  experiments.
- **[M4]** On the join's answer (the `mediaAnswer` frame), record `layout` and `reinviteless_mids`
  (§ 4.0.2).
- **[M5] New `OUTWARD_METHODS` entry `call_update_media_descriptions`** (24 entries). It refuses,
  before `post_call_signal`:
  - a phase that is not Connected;
  - a call without `reinviteless`;
  - a missing link;
  - an unknown `sending` kind;
  - a kind with no reinviteless mid in the layout.

  It builds the descriptions (§ 4.8), POSTs, and on 2xx records `descriptions`, `sending` and
  `last_sharing_md_request_id`, emits `call_state`, journals the shape, and returns `{call_id,
  request_id, descriptions, response_keys}`.
- **[M5]** `call_answer_media` (`:7962`) attaches the current descriptions under reinviteless.
  `can_send_media` requires the `updateMediaDescriptions` link under reinviteless.
- **[M8]** Emit `call_video_control` to the page.
- **[M1] Tests.** `every_gated_method_is_named_in_the_automation_guard` (`:17954`) requires every
  method in EACH alternation of line 981 (all 69 already are). The behavioural gate loop (`:17138`)
  gains `call_join`, `call_answer_media`, `call_offer_media`, `call_start_sharing` and
  `call_stop_sharing`; it gains `call_update_media_descriptions` in [M5].

**.claude/hooks/guard-live-automation.sh** [M5]: add `call_update_media_descriptions` to both
alternations on line 981, with a rationale comment above the grep.

**.claude/hooks/guard-live-automation.test.py** [M5]: a script naming the method against
127.0.0.1:19420 or 19440 is blocked; the same name in a `grep` pattern is not.

**.claude/hooks/guard-prod-chat-target.sh** and its test [M1, pending user decision D2]: add
`teams\.microsoft\.com|teams\.cloud\.microsoft|teams\.live\.com` to `live_address`. The MCP
browser may still look at real Teams but never type or click in it. That closes the gap the
2026-08-05 capture went through (NATIVE-CALLING.md:341-343).

**web/src/lib/call-experiments.ts** (new) [M3]:

- `CALL_EXPERIMENTS`, `validateCallExperiments(list, shape)` (the same rules as Rust),
  `readCallExperiments(storage)`, `has(set, name)`.
- Unknown names are dropped from the read and reported in the console; the backend refuses them
  anyway.

**web/src/lib/ws-client.ts**:

- **[M3]** `callPrepare` takes `experiments`.
- **[M5]** `callUpdateMediaDescriptions(callId, sending)` via `writeRequest` (pinned by
  `every_gated_method_is_called_with_the_write_token`, `server.rs:18015`).

**web/src/lib/call.ts**:

- **[M1]** `CallStatus` gains `known_experiments`. `ActiveCall` gains `experiments`, `own_streams`
  and `sending_confirmed`. It gains no `one_to_one`: the page reads video-receive's
  `multi_party === false` (README § 3, collision 6).
- **[M1]** Correct the doc of `CallPreparation.one_to_one` (`:136-143`). Today it decides only the
  codec filter of an added section, because `reserve` is unused, and a conference's first offer
  carries video and sharing sections too.
- **[M2]** `screenCaptureSupported()`, which is
  `typeof navigator.mediaDevices?.getDisplayMedia === "function"`.

**web/src/lib/ms-sdp.ts** [M3/M5]: `toMsSdp(sdp, labels, options?: {videoSignalingFb?: boolean;
tracklessMsid?: Map<string, number>})`.

- The fb line goes on live video sections of OFFERS only.
- `tracklessMsid` writes `a=msid:- <m-line index>` on a reinviteless section that has no msid.
- With no options the output is byte-for-byte unchanged, and a test pins it.

**web/src/lib/call-media.ts**:

- **[M2]** `startCallMedia` excludes from `RemoteVideoTracks` any track whose mid is one of
  `LocalSenders`' own send mids (`isOwnSendMid`). Only video-receive may claim those mids.
- **[M3]** `CallMediaOptions.experiments`. `LocalSenders.start` takes its direction from a pure
  `sendDirectionFor(kind, experiments, reinviteless)`.
- **[M4]** `reserve(pc, {direction, cameraChannels, reinvitelessMids})`, wired for joins under
  `join-reserve` or `reinviteless`. `stop` returns a reserved non-reinviteless section to
  `recvonly`, never `inactive`, under the layout switches. `noteAccepted` counts any negotiated
  direction that includes send.
- **[M5]** Reinviteless sections are reserved `sendrecv`. `activate(kind)` / `deactivate(kind)`
  call `replaceTrack` on the reserved transceiver with no offer. `answerRemoteOffer` re-asserts
  `sendrecv` on reinviteless transceivers before `createAnswer`. `reservedMidFor(kind)` lets the
  store check the mids the backend returned.
- **[M7]** The 1:1 layout under `one-to-one-layout`: `inactive` reserved sections, no codec filter.
- **[M8]** Screen `contentHint "text"` and the bounded constraints (§ 4.15). `applyVideoControl`
  calls `setParameters`, clamped to the capture, and drops out-of-order frames.
- **[M9]** `pauseScreen()` / `resumeScreen()` with a frozen canvas track.

**web/src/lib/store.ts**:

- **[M2] In-flight guard.** `setSending` (`:2917`) sets a per-kind `sendingInFlight` ref in its
  FIRST synchronous statement and clears it in `finally`, so a press while one is out is refused
  silently. That is the `sendingRef` rule of § Sending messages. The control is disabled meanwhile.
- **[M2] Capability before outward act.** A screen checks `screenCaptureSupported()` BEFORE
  `callStartSharing`. When it fails, a `CaptureUnavailableError` is thrown and nothing goes out.
- **[M2]** `answerCall` passes `oneToOne: prepared.one_to_one === true`, read off the answering
  branch's `call_prepare` answer exactly as `startCall` reads the placing branch's (`store.ts:3049`).
  That value comes from video-receive's one predicate (README § 3, collision 6).
- **[M3]** `startCall` and `joinMeeting` read the experiments and pass them to `callPrepare` and
  `openCallMedia`. `answerCall` does NOT. Under `no-content-sharing-session` the screen skips
  `callStartSharing`, and a 1:1 under `one-to-one-layout` always does. Under `modalities-parity`,
  modalities at `:3009` and `:2733` are computed from directions.
- **[M5]** Under reinviteless, `setSending` calls `callUpdateMediaDescriptions` first, checks the
  mids, then `activate`. Release and rollback follow § 4.8.
- **[M5/M6] The awaiting state.** In a meeting, after the act resolves, the toggle stays pending
  until `sending_confirmed` includes the kind. The bound is `MEDIA_ANSWER_TIMEOUT_MS` (35 s, the
  client's `mediaAnswerTimeoutSec:35`). On timeout the capture is released and
  `captureUnconfirmedMessage` is shown. It ships with M5 for reinviteless (driver-only) and with M6
  for the default path, once a real roster echo has been seen and pinned. A backend that does not
  send `sending_confirmed` reads as confirmed whenever `sending` holds the kind.
- **[M8]** Apply `call_video_control` events.

**web/src/lib/call-failure.ts** [M2/M5]: `screenShareUnsupportedMessage()` [M2] and
`captureUnconfirmedMessage(kind)` [M5], with unit tests.

**web/src/lib/call-stage.ts**:

- **[M2]** `export const MEETING_SCREEN_SEND_VERIFIED = false`. While it is false,
  `shareTakeoverHint` appends the measured fact (§ 7). The press is NOT refused (decision D1).
- **[M2]** Pure `sendControlState(kind, call, localVideo, inFlight)` → `off | pending | on`, with
  the backward-compatible reading above.
- **[M3]** The subtitle suffix.

**web/src/components/call-stage.tsx** [M2]:

- The share control is drawn only when `screenCaptureSupported()`. On an insecure origin it is drawn
  only on a FINE pointer (`useCoarsePointer()` false), so a desktop reader still reaches the https
  sentence while a phone, which cannot share even over https, sees no control.
- Both toggles render `data-state` and `aria-busy` and a progress ring on the 44 px target.

**web/src/components/call-video.tsx** [M2]: line 102 uses `object-contain` for `kind === "screen"`
and keeps `object-cover` plus mirroring for the camera. [M9]: a Pause/Resume control and overlay.

**web/mock/server.ts**:

- **[M2]** `call_start_sharing` (`:10527`) REPLACES a held session like Rust and counts
  `mockSharingReplacedWithoutRelease`, exposed through the `{kind:"call_sharing_order"}` reply. The
  spec asserts 0, which keeps the leak detector.
- **[M3]** `call_prepare` validates experiments with the same rules (refusing unknown names,
  answering-branch use, and broken combinations) and echoes them. `call_status` carries
  `known_experiments`.
- **[M5]** A `call_update_media_descriptions` arm with every backend refusal. It assigns
  `requestId` and sets `own_streams` (with `md_request_id`) and `sending_confirmed` after
  `MOCK_CONFIRM_MS`. `call_offer_media` sets `sending_confirmed` for a meeting after its answer.
- **[M5] Hooks:** `{kind:"call_media", refuse_md:true}` and `{kind:"call_media",
  withhold_confirmation:true, bound_ms?}`. A spec MUST reset them.

**web/scripts/join-live.ts**:

- **[M1] Sender-side instrumentation.** Per-kind `remote-inbound-rtp` (`packetsLost`,
  `roundTripTime`, `fractionLost`); outbound `frameWidth`, `frameHeight` and
  `qualityLimitationReason`; receiver `totalAudioEnergy` per inbound audio (used with `--tone`).
- **[M1]** `SIGNAL_PROBE` also keeps `call_state` frames and the `call_status` reply.
- **[M1]** `reportPair` prints the sender's own `framePaths`, `own_streams`, `sending_confirmed` and
  its offer and answer sections, and ends on a graded S1-S6 verdict that includes audio survival on
  both installs.
- **[M3] `--experiment a,b` and `--receiver-experiment a,b`.** Validated by importing
  `CALL_EXPERIMENTS` and injected by `addInitScript` into the driver's own context.
- **[M3] `--check-only`.** It loads both fronts, reads `known_experiments` off the `call_status`
  reply and exits without pressing Join.
- **[M3]** Abort when `call_prepare` does not echo the experiments.
- **[M4] Roster safety rail.** When the switches touch the first offer, the receiver (baseline)
  joins first. If its roster holds anybody other than the user's own account (`others`
  non-empty), the driver refuses to let the sender join and hangs up. The rail reads the
  backend's own `others` and `own_streams`, never a page-side roster parse, and meeting-awareness
  M3 keeps their semantics (pinned by a test there). After the sender joins and
  BEFORE any press, the receiver's `publishing` and the sender's `own_streams` must show no sending
  `main-video` or `applicationsharing-video` from the sender's endpoint. Otherwise both hang up and
  the step is recorded as "publishes at join".
- The usage header is updated. The target is unchanged.

**web/scripts/call-live.ts** [M7]:

- `--camera`, `--share` and `--experiment` for the pinned 1:1.
- **The share rail.** `--share` reuses `join-live`'s exact launch flags
  (`--auto-select-desktop-capture-source=Entire screen`, `--auto-accept-this-tab-capture`). It
  refuses unless the browser was launched headless by this file — a constant no environment can
  flip — and the page's `navigator.userAgent` says `HeadlessChrome`. That UA check is measured on
  this machine's Chromium before the flag ships; if the UA does not say so, the launch-option check
  alone stands.
- Before each press it re-reads the composer's and the call control's `data-conversation-id`.
- It hangs up and releases on every path out, and prints the offer and answer sections and
  `remote-inbound-rtp`.
- The header names the rail. The flags ship only after the colleague's fresh consent (D5).

**web/scripts/preview.ts**: the `--call` run gains, in both themes:

- [M2] `-camera-pending`, `-share-takeover-hint`, `-mobile-no-share`, `-own-screen-fitted`;
- [M3] `-experimental-subtitle`;
- [M5] `-camera-awaiting` and `-camera-unconfirmed`;
- [M9] `-share-paused`.

**web/e2e** — see § 8.

**NATIVE-CALLING.md** and **CLAUDE.md** — see § 10, each update tied to the milestone that changes
the behaviour.

## 6. Gating and consent

**The new RPC**

- `call_update_media_descriptions` is an `OUTWARD_METHODS` entry. It is the act that tells a
  meeting the user's camera or screen is live: the same consent as `call_offer_media`, so it gets
  the same gate.
- The write token, and a read-only backend refuses it at the dispatch gate. It is named in BOTH
  alternations of the guard, and the strengthened pin now checks both. It is called through
  `writeRequest`, which is the only check that can see this, since the mock does not enforce the
  token.
- It is in the behavioural gate loop (read-only refused, untokened refused, tokened accepted).
- The mock mirrors every refusal.
- It is built from primitives (`sending` kinds). The page cannot choose a mid, a direction or a
  label.

**Nothing leaves before the gate**

- POST-first (§ 4.8): no camera RTP goes to the media server before the backend has passed the
  dispatch gate and every pre-network check. The client's own order exists only behind
  `track-before-update`, in driver contexts.

**The experiments param**

- No new method: it rides `call_prepare` (`MACHINE_METHODS`). It is validated against a closed
  list on both sides; unknown or inapplicable names are refused, and it is refused on the answering
  branch.
- It changes only the SHAPE of bodies the user already consents to (join, invitation, offer,
  answer). It opens no capture and adds no recipient.
- **Whether a reserved or reinviteless join publishes anything is NOT assumed. It is checked.** The
  claim is "expected to publish nothing (recvonly, inactive, or trackless with recvonly
  descriptions); the driver verifies the roster before every press and hangs up otherwise" (§ 9).
- Only sanctioned drivers write the key, in ephemeral contexts. Every page shows "experimental
  calling" and the journal names the switches, so a switch set by accident is never silent.

**Removed outward side effects**

- The screen-capture capability is checked BEFORE `call_start_sharing`, so a phone without
  `getDisplayMedia` no longer takes the presenter role (from a colleague, or from the user's own
  other device).
- The double press can no longer fire two outward acts. The in-flight guard implements "one press,
  one act".

**What is NOT changed:** the takeover. A share while somebody presents is still one press, as
CLAUDE.md § Video in a meeting requires. What changes is what the hint says before the press
(§ 7). A local refusal is offered to the user as decision D1 and is not built.

**What needs no gate**

- Reading vdclc frames, applying sender parameters, pause/resume, the content hint and the capture
  constraints. Each only constrains or replaces a stream the user already chose to send.
- A camera or screen opens only on the user's own click, with the browser's own permission prompt
  as the second gate.

**Live runs and other surfaces**

- Live runs go only through `join-live` (including `--pair`, `--share`, `--swap`, `--experiment`,
  `--check-only`), `call-live` (the pinned 1:1, after fresh consent), or the user's own click.
- No step drives real Teams with the browser MCP tools (§ 13).
- The calendar and mail are untouched.

## 7. UI and copy (all English)

**Surfaces:** the stage header's camera and share toggles (`call-stage.tsx`), the own-preview tile
(`call-video.tsx`), the call notice (`lib/notice.ts` via `reportCall`), and the stage subtitle.

**Camera toggle** (`data-state`, `aria-busy`):

| State | Label | Title / ring |
| --- | --- | --- |
| off | "Turn the camera on" | — |
| pending, in flight (every call) | "Turning the camera on…" | disabled, ring |
| pending, awaiting the meeting (meetings; M5 reinviteless, M6 default) | "Turning the camera on…" | "Waiting for the meeting to show your camera", ring |
| on | "Turn the camera off" | — |

**Share toggle:** the same states. Off reads "Share the screen", pending reads "Starting your
screen share…", on reads "Stop sharing the screen".

**Takeover hint** (title on the share control while somebody else presents):

- While `MEETING_SCREEN_SEND_VERIFIED` is false: "Share the screen — this stops the screen {Name} is
  sharing, and teams-lite cannot show yours in a meeting yet". The press is still allowed.
- Once verified: today's sentence, "Share the screen — this stops the screen {Name} is sharing".

**Notices:**

- Unconfirmed after 35 s, camera: "The meeting never showed your camera, so teams-lite turned it
  off. Open this meeting in Teams to use it."
- Unconfirmed after 35 s, screen: "The meeting never showed your screen, so teams-lite stopped
  sharing. Open this meeting in Teams to share."
- A press racing a capability loss: "This browser cannot share a screen. Share from a computer
  instead."
- Kept as they are: `captureRefusedMessage`, `captureDroppedMessage`, and the insecure-origin
  sentence.

**Subtitle:** append " · experimental calling" when `call.experiments` is non-empty.

**Own screen preview:** fitted (`object-contain`). The camera preview stays cropped and mirrored.

**M9, pause/resume:** "Pause sharing" / "Resume sharing" on the own-screen preview. While paused,
the overlay reads "Paused — the meeting sees this frame".

**Mobile**

- The share control is not drawn where `getDisplayMedia` is missing (iOS Safari, Android Chrome),
  and it is not drawn on a coarse pointer on an insecure origin either. A phone's https fix would
  not bring a share control back, so sending a phone reader off to fix the address is the defect
  `call-failure.ts` already avoids for iPhone.
- The camera toggle stays (user-facing camera, mirrored preview). The ring fits the existing 44 px
  target, and nothing is added to the folded window.

**Stated limits**

- A 1:1 has no roster, so there is no "awaiting the meeting" state. "On" there means the answer
  accepted the section, which is today's meaning.
- A second page shows `sending` from the backend, without the first page's in-flight state.

## 8. Tests, mock and captures

**Rust: `calling::tests`**

- [M1] Exact-JSON golden pins for all seven builders in their default shape. They are committed
  before any refactor.
- [M1] `own_streams_of` reads a stream with no `sourceId` and keeps `mdRequestId`. `streams_of` is
  unchanged (the existing tests stay green).
- [M1] A received `negotiationTag` cannot reach an outbound body. This is a compile-time separation
  plus a scan test.
- [M3] `experiments_are_a_closed_set_with_rules` covers unknown names, inapplicable names (a 1:1
  name on a join), excluded and required combinations, and the answering branch.
- [M3] Each shaped builder with an empty extras set equals its golden pin;
  `the_mask_without_the_reinviteless_bit_is_the_captured_one_minus_512` (72951).
- [M5] `a_reinviteless_join_carries_only_the_reinviteless_entities` (exact JSON, `requestId` 1, no
  `negotiationTag`, no audio); `an_update_body_is_the_clients_TG_without_its_envelope` (exact JSON,
  capital U); `last_sharing_request_id_moves_on_every_update_that_carries_sharing`.
- [M8] `a_video_control_frame_is_read_like_the_client` covers `fmtParams` and `fmtParamsList`,
  `max-fps/100`, `max-br*1200`, and sequence ordering.

**Rust: `server::tests`**

- [M1] The guard pin requires EACH alternation. The behavioural gate loop covers every `call_*`
  outward entry.
- [M1] `sending_confirmed_reads_our_own_endpoint_only` (one account, two endpoints).
- [M1] The stale "turn it on in Settings" strings are gone.
- [M3] `call_prepare` refuses experiments on the answering branch.
- [M3] A cross-process pin that `web/src/lib/call-experiments.ts` lists exactly
  `calling::experiments::ALL`, with the same rules (`include_str!`).
- [M5] A source scan: the `call_update_media_descriptions` arm checks phase, reinviteless, link and
  layout before `post_call_signal`.

**Web unit** (`bun run test`)

- `call-experiments.test.ts`.
- `ms-sdp.test.ts`: the fb line appears on live video sections of offers only (never on audio, a
  stub or an answer); `tracklessMsid` uses the m-line index; with no options the output is
  byte-for-byte unchanged.
- `call-media` pure helpers: `sendDirectionFor`; `isOwnSendMid`; a stopped reserved section goes to
  `recvonly` (fake transceiver); `noteAccepted` counts `sendrecv`; POST-first ordering (a refused
  update attaches no track).
- `call-stage.test.ts`: `sendControlState` (including the absent-`sending_confirmed` reading and the
  in-flight state), the hint with `MEETING_SCREEN_SEND_VERIFIED` false and true, and the subtitle
  suffix.
- `call-failure.test.ts` and `call.test.ts` (`screenCaptureSupported`).

**E2E against the mock** (`bun run test:e2e`)

`calling.spec.ts`:

- **(a)** The default path is unchanged: exactly one `call_offer_media` per camera press, counted at
  the RPC level.
- **(b)** In-flight guard: press the camera control three times in one task, and press the share
  control twice while held. RPCs are counted, not tiles: one `call_offer_media`, one
  `call_start_sharing`.
- **(c)** With `["reinviteless","no-content-sharing-session"]` set in the spec's own context:
  `call_prepare` echoes them; a camera press produces `call_update_media_descriptions` and NO
  `call_offer_media`; the toggle goes pending, then on once the mock confirms; the subtitle says
  experimental calling.
- **(d)** `refuse_md`: nothing was attached, the capture is released and `captureRefusedMessage` is
  shown.
- **(e)** `withhold_confirmation` with a short `bound_ms`: the capture is released and
  `captureUnconfirmedMessage` is shown.
- **(f)** The existing takeover test (`:827`) keeps passing: the title regex still matches, and the
  new clause is asserted.
- **(g)** `mockSharingReplacedWithoutRelease` is 0 at the end of the session test (`:800-808`). The
  CLAUDE.md sentence about the mock refusing a second session is updated.
- **(h)** `no-content-sharing-session`: `call_sharing_order` holds no `start_sharing`.
- **(i)** The own-screen preview's computed `object-fit` is `contain`.

`mobile.spec.ts`: with `getDisplayMedia` deleted via `addInitScript`, there is no `call-share`
control and no `call_start_sharing` RPC, and the camera control is present.

Every spec resets every new hook.

**No-tenant WebRTC** (new `web/e2e/webrtc-reinviteless.spec.ts`, the `webrtc-glare.spec.ts` pattern:
two real peer connections, `about:blank`, a canvas `captureStream`). It records browser facts the
design rests on:

1. Whether Chrome writes `a=msid` and `a=ssrc` for a trackless `sendrecv` section.
2. A `replaceTrack` on it gives the far side `framesDecoded > 0` with NO renegotiation.
3. A `recvonly` reserved section flipped to `sendrecv` by renegotiation decodes frames.
4. A 1:1 `inactive` → `sendrecv` flip decodes frames.
5. Whether a `track` event fires on our `sendrecv` section when the far side sends nothing. This
   feeds `isOwnSendMid`.

**Captures:** `cd web && bun run preview -- --out /tmp/call --call`, with the new names in § 5.

**Gates:**

- `cargo test`;
- `python3 .claude/hooks/guard-live-automation.test.py` (and the browser-guard test if D2 lands);
- `bun run test`, `bun run typecheck` and `bun run test:e2e`, compared against a baseline run on
  master because of the known flaky sibling pairs (CLAUDE.md § Pictures).

## 9. Live verification plan

Every ladder milestone SHIPS to master with the switches off by default. The staged pair re-stages
itself, and the released build (19442) needs CI plus the user's own Update click. A live step can
only run once both installs carry the build. `--check-only` proves that with no join.

| # | Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- | --- |
| 0 | Both installs up and current | `systemctl --user enable --now teams-lite.target`; `systemctl --user enable --now teams-lite-app.service`; the in-app Update on 19442 | **The user** | `bin/teams-lite-service.sh status` shows both up. `cd web && bun run join-live -- --pair --check-only --experiment camera-sendrecv` prints each front's `known_experiments` and joins nothing. |
| 1 | R0 baseline with a second participant | `cd web && bun run join-live -- --pair`, then `-- --pair --share`, then `-- --pair --swap` | agent | The graded S1-S6 line. Audio bytes stay non-zero on both installs, and neither call ends early. |
| 2 | R0b, the screen without the session | `cd web && bun run join-live -- --pair --share --experiment no-content-sharing-session` | agent | S-level ≥ R0's for the screen. The journal shows no `addModality` POST. |
| 3 | R1 (joins only), R2, R3, one at a time on top of the best so far, each as camera, `--share`, `--swap` | `cd web && bun run join-live -- --pair --experiment no-reinviteless-capability` (then `camera-sendrecv`, then `video-signaling-fb`) | agent | A step "moves" when it reaches a higher S-level than R0. R1's empty-body 400 is a non-result (§ 4.3). A step that costs audio, or ends with `SdpParsingFailure` or 410, is reverted that run and recorded. |
| 4 | R4a-d, the reserved layout, one switch at a time | `cd web && bun run join-live -- --pair --experiment join-reserve` (then `,join-screenviewer`, then `,no-reinviteless-capability`, then `,join-reserve-ten-cameras`) | agent; the roster rail refuses if anybody but the user's account is in the meeting | The join answer keeps the reserved mids at non-zero ports. The service's renegotiations echo our mids, not zeroed slots. Nothing publishes at the join (rail). A camera press reaches S1 or higher. The receiver keeps its own receive path (it runs the baseline). |
| 5 | R5a/b/c, reinviteless | `cd web && bun run join-live -- --pair --experiment reinviteless,no-content-sharing-session` (camera); the same with `--share` and with `--swap`; sub-variants `,reinviteless-sharing-only`, `,reinviteless-all-cameras`, `,bw-seed`, `,track-before-update` only if the plain one fails | agent; same rail | **R5a:** our own streams carry `mdRequestId 1`, nothing publishes at the join, and the receiver sees no sending stream from us before a press. **R5b:** the update answers 2xx (response keys journaled), our main-video turns sendrecv (S3), then S2, S4, S5. **R5c:** our sharing stream carries `mdRequestId == last` with a sending direction. |
| 6 | The real client's own bodies, if the ladder stalls | the user's own signed-in Teams web, in the pinned meeting, DevTools Network: join, camera on, share, then a SANITIZED HAR export (no cookies, no authorization headers) | **The user** (D3) | A python `-I` extractor that refuses a file still holding an `authorization` or `cookie` header prints only: the join's `callInvitation.mediaContent` keys, `mediaDescriptions`, `endpointCapabilities` and `callModalities`; the `UpdateMediaDescriptions` bodies; the m-line digest. **Signed in**, so it answers the ECS question a guest capture cannot. |
| 7 | 1:1 outgoing video | `cd web && bun run call-live -- --camera --experiment one-to-one-layout`; then `--share`; then add `one-to-one-required-features` | agent drives; the consenting colleague must be present and must have said yes to video and screen (D5) | The answer accepts the reserved mid as sendrecv. The colleague confirms he sees Chrome's test pattern, or the blank headless screen. Audio both ways stays up. |
| 8 | 1:1 incoming video | none: the colleague calls the user | **The user's own click** to answer; the colleague turns their camera on | A tile showing the colleague's camera (the user's eyes). The user's camera press is accepted on the adopted section. |
| 9 | Promotion, right after whichever step first reaches S5/S6 | `cd web && bun run join-live -- --pair` with NO `--experiment`, then `--share` and `--swap`; then `-- --pair --tone` | agent, then **the user's own click** | The same S-level as the winning step with no switch. Receiver `totalAudioEnergy > 0` under `--tone`. **The user hears and is heard** in the pinned meeting and, if a 1:1 default changed, in a 1:1 with the colleague, on the promoted build, the same day. The user sees their own camera arrive on their other device. |

**Abort rules for every run:**

- hang up both sessions on any audio regression, on any early call end, or on the roster rail
  firing;
- record the outcome in NATIVE-CALLING.md § 10.8's ladder table, whichever way it went.

## 10. Milestones

Each milestone ships alone. "Depends on" names other workstreams.

1. **M1: Measure from the sender's side, and pin today's bodies.**
   - Golden pins for all seven builders.
   - `own_streams`, `sending_confirmed` and `md_request_id` (data only, no UI).
   - `ReceivedMediaContent`; journal lines for the two inbound doors.
   - The guard pin strengthened to both alternations, and the gate loop widened.
   - The browser-guard hardening (if D2).
   - Driver instrumentation and the S1-S6 verdict.
   - The stale Settings text, and the doc fixes that are pure corrections: NATIVE-CALLING.md:1506
     (`startScreenSharing` opens no session in the 2026-11-05 bundle), :1454-1460 (a meeting's first
     offer carries the sections), § 10.3a ("Simulcast is off" is base settings only), :341-343
     (provenance: guest capture, and the MCP route closed if D2); CLAUDE.md's "colleague" →
     "the user's own other device" for the 2026-08-06 takeover; `call.ts:136-143`.
   - Then run R0.

   Effort S. Risk none (no outward body changes; golden pins guard the refactors that follow).
   Depends on: video-receive (shares `join-live.ts` and the receive-side blind-spot
   instrumentation; land the driver changes once).
2. **M2: Honest, safe controls on today's protocol.**
   - The in-flight guard; capability before any outward act; the share control's capability and
     pointer gate.
   - The takeover-hint fact, with `MEETING_SCREEN_SEND_VERIFIED`.
   - The own-screen preview fitted.
   - Own send mids kept out of the remote tiles.
   - The answering branch's `one_to_one` from video-receive's one predicate (README § 3, collision
     6); `fromMixer` journaled as a fact.
   - The mock's `call_start_sharing` aligned, with the leak counter.
   - The mobile and calling e2e and the captures.

   Effort S. Risk low. Depends on: video-receive M1 (the `is_one_to_one()` predicate, for that one
   item; the rest of M2 does not wait); incoming-and-conversation (the attach answer's `fromMixer`;
   take it once); media-robustness-devices (serialize `setRemoteAnswer` with the negotiation queue —
   the guard here removes the double press, the queue there removes the rest). **No
   media-robustness milestone schedules that serialization yet**; the README records it as an open
   coordination item, and until it lands this guard is the only protection.
3. **M3: The experiment switch, and the cheap steps.**
   - `calling::experiments` and `call-experiments.ts`, with the cross-process pin.
   - The `call_prepare` param, echo and journal; `known_experiments`; the subtitle; the mock mirror.
   - `join-live --experiment`, `--receiver-experiment`, `--check-only` and the echo check.
   - Switches: `no-content-sharing-session`, `no-reinviteless-capability` (joins),
     `camera-sendrecv`, `video-signaling-fb`, `modalities-parity`, `bw-seed`.
   - Live: R0b, R1, R2, R3.

   Effort M. Risk medium but contained: the defaults are pinned byte-for-byte and only drivers set a
   switch. Depends on: video-receive (agreement that `modalities-parity` stays switch-only until
   they promote it).
4. **M4: The reserved meeting layout (non-reinviteless).**
   - `reserve` wired under `join-reserve` with direction, channel count and the reinviteless set.
   - `stop` → `recvonly`; `noteAccepted` counts `sendrecv`.
   - The backend layout from the relayed SDP and its answer.
   - `join-screenviewer`, `join-reserve-ten-cameras`; the roster safety rail; two-peer specs (3)
     and (5).
   - Live: R4a-d.

   Effort M. Risk medium-high, because it changes the FIRST join offer (driver-only, with the
   receiver kept on the baseline). Depends on: nothing that lands later. The roster safety rail
   reads the backend's own `others` (today's) and this workstream's `own_streams` (M1), so it does
   not wait for meeting-awareness M3 (P3); in return meeting-awareness M3 must keep the semantics of
   both fields (anybody but the user's own account; our own endpoint's streams), pinned by a test,
   because a rail that misreads `others` lets a test camera go into a meeting with a colleague in
   it. video-receive (the receive path R4 may poison is theirs to watch).
5. **M5: Reinviteless.**
   - `mediaDescriptions` on the join and on answers; trackless `sendrecv` sections and the msid
     fixup.
   - `call_update_media_descriptions` (gate, guard, write token, mock refusals, POST-first,
     rollback).
   - Activate/deactivate; the awaiting state and its bound for reinviteless calls.
   - The sub-variant switches; e2e (c)-(e); two-peer specs (1) and (2).
   - Live: R5a-c.

   Effort L. Risk high on the protocol side (new body, new door), contained by the switch, the
   closed validation, POST-first and rollback. Depends on: plane-reliability (a stable calling
   registration on both installs through a long ladder session).
6. **M6: Make the winning shape the default — CONDITIONAL.** It runs right after whichever ladder
   step first reaches S5/S6, so it may follow M3 or M4 without waiting for M5.
   - Defaults switch to that shape for meetings, and `MEETING_SCREEN_SEND_VERIFIED` flips.
   - The default-path awaiting state ships, with the roster echo that run showed now pinned.
   - The losing switches are deleted after one release.
   - The promotion gate of § 9 step 9, including the user's own listening check.
   - Docs: CLAUDE.md § Video in a meeting (the switch rules; the replacement for "RESERVING … WRITTEN
     AND NOT USED"; the new RPC; the awaiting state; "A SCREEN is a SESSION before it is a track"
     corrected by the R0b result); NATIVE-CALLING.md § 10.3 (the exact reinviteless bodies) and
     § 10.8 (the ladder table).

   Effort S. Risk medium, because the default join changes for everybody; mitigated by the gate.
   Depends on: video-receive (S6 needs their receive fix to be a clean signal; without it promotion
   rests on S3-S5).
7. **M7: 1:1 video, both directions.**
   - `one-to-one-layout`, `one-to-one-required-features`; no session on a 1:1.
   - `call-live --camera`, `--share`, `--experiment` with the headless rail, after fresh consent.
   - Two-peer spec (4); live steps 7-8; then the 1:1 default, under the same promotion gate.
   - Docs: NATIVE-CALLING.md § 8/8a/8b and CLAUDE.md § Audio calls (the 1:1 first offer), and
     § 10.7a ("What is BUILT").

   Effort M. Risk medium: the first offer of every outgoing 1:1 changes under the switch. Depends
   on: incoming-and-conversation (the incoming adopt path and `fromMixer`); ringing-alerts-history
   (nothing to change, but a 1:1 first-offer change is felt on their verified ring path, so they
   re-verify the ring).
8. **M8: Sender quality once a picture arrives.** The screen `contentHint "text"` and bounded
   capture; vdclc handling (backend parse plus event; page `setParameters`); a capture of the
   decoded resolution at the receiver. Effort M. Risk low (it constrains a stream already chosen; a
   wrong cap costs quality, never the call). Depends on: media-robustness-devices (the device and
   quality surfaces read the same sender stats).
9. **M9: Pause and resume a screen share.** A frozen canvas track at 1 fps; `replaceTrack` both
   ways; the control and the overlay; an e2e asserting the sender's track is the canvas track; a
   capture. Effort S. Risk low (local only). Worth building only after M6.

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A step changes the first offer of the verified audio path and audio regresses. | Golden pins keep the defaults byte-for-byte. Switches are set only in driver contexts. Every verdict checks audio bytes and call survival on both installs, and a regressing step is reverted the same run. Promotion requires the user to hear and be heard (fake-microphone bytes prove a transport, not audio). |
| Reserving at the join poisons the receive path (measured once). | The receiver runs the baseline (`--receiver-experiment` is opt-in). The sender's own receive is reported separately. R4a replays on today's stack, isolating the later fixes. |
| A reserved or reinviteless join publishes the user as "camera on" at join. | The roster rail aborts before any press. It is also a promotion criterion, and the mock e2e asserts `own_streams` is not sending after join. |
| Camera RTP leaves before the outward gate. | POST-first is the default. `track-before-update` exists only in drivers. A unit test proves a refused update attaches nothing. |
| The receive path still delivers 0 bytes, so S6 cannot prove a send. | S2-S5 grade the send without it. This is coordinated with video-receive. |
| Both pair installs are one account, and an SFU might treat a same-person endpoint differently. | The service has routed a same-account other-endpoint share before (2026-08-06). The final confirmation is the user's own click with a colleague (step 9). |
| An older released build silently ignores a switch. | `--check-only` reads `known_experiments` with no join, and `call_prepare` echoes the list. The driver aborts on either missing. |
| The update body or casing is wrong, and the service answers 400 with nothing named. | The body is `TG` verbatim. Response keys and service headers are journaled. Rollback via POST-first. One variable at a time, as `attach` and `startOutgoingNegotiation` each named their field. |
| The tenant enables reinviteless for VBSS only. | `reinviteless-sharing-only`. The signed-in HAR (D3) answers it directly. |
| The camera light is on while the meeting shows nothing. | In flight: disabled control. In a meeting: the awaiting state with a 35 s release, shipped only where a confirmation signal has been seen. A rejected section is already released today. |
| The awaiting state kills a working camera because our roster-echo derivation is wrong. | It is not on the default path until M6, and M6 pins the echo the winning run showed. Under reinviteless (drivers only) the verdict prints `own_streams` and whether the release fired, so a wrong derivation shows as "S6 + released unconfirmed". |
| Chrome does not write `a=msid` / `a=ssrc` for a trackless `sendrecv` section. | Two-peer spec (1) records it first. `tracklessMsid` adds the client's fixup only for reinviteless sections and only under the switch. |
| A `sendrecv` camera mid receives a stranger's stream and draws a ghost tile. | `isOwnSendMid` keeps our mids out of `RemoteVideoTracks`. Video-receive decides when a source is mapped there. |
| A switch is left set in a real browser. | Only drivers write the key, in ephemeral contexts. Every page shows "experimental calling" and the journal names the switches. |
| The takeover hint sentence outlives the fix. | It is one constant, flipped in the promotion PR, and a test asserts the hint is today's sentence when the constant is true. |
| `call-live --share` sends the user's real desktop to a colleague. | Headless is a constant, a UA check refuses a headed browser, and the capture is the blank headless surface, as in `join-live`. |

## 12. Open questions, and decisions the user must make

**Decisions**

- **D1, the takeover.** Today a share while somebody presents takes the screen from them and, as
  measured, shows nothing of the user's. This design keeps the press (CLAUDE.md's rule) and only
  adds "teams-lite cannot show yours in a meeting yet" to the hint. Alternative: refuse locally
  until sending is verified. That would be a temporary reversal of the documented rule and needs the
  user's explicit decision, plus rewrites of `calling.spec.ts:827`, the preview capture and the
  CLAUDE.md bullet. **Recommendation:** keep the press. First measure `no-content-sharing-session`
  with a real presenter: the user shares from real Teams while the driver presses Share. If the
  displacement came from our PPT-style session, the switch removes it and the question goes away.
- **D2, closing the browser-guard gap.** Should `guard-prod-chat-target.sh` treat
  `teams.microsoft.com`, `teams.cloud.microsoft` and `teams.live.com` as live fronts (observe-only)?
  **Recommendation:** yes. It affects every workstream that might plan a real-client capture, and
  it retires the method the 2026-08-05 capture used.
- **D3, a signed-in capture of the real client.** Will the user record a sanitized HAR from their
  own Teams web, in the pinned meeting (join, camera on, share), and hand it over? It is the
  cheapest way to learn this tenant's reinviteless caps and `numVideoChannelsGvc`, and could
  collapse R1-R5 to one step.
- **D4, first-offer experiments and the pinned meeting.** Is it acceptable to run the
  `join-reserve` / `reinviteless` steps only when nobody but the user's own account is in the pinned
  meeting? The driver enforces this and refuses otherwise.
- **D5, the colleague's consent.** Will Clément BOSLE (the pinned `call-live` colleague, consented
  for ringing) agree to receive a test camera and a blank-screen share? Who calls whom for the
  incoming check? `call-live`'s constant comment records the answer.
- **D6, the promotion gate.** Agree that every default change needs the user's own hear-and-be-heard
  check in the pinned meeting (and a 1:1 if that default moves), on the same day.
- **D7, ownership of `modalities-parity`.** It reverses `5b355f0`'s "Video on every answer". The
  video-receive workstream owns promoting it, on its own evidence. Confirm.
- **D8, the default share path after R0b.** If `no-content-sharing-session` is neutral or better,
  should the default stop opening the content-sharing session (CLAUDE.md "A SCREEN is a SESSION")
  at M6?

**Open questions**

- The signed-in tenant's `maxReinvitelessMediaFor{Video,VBSS}ForWeb` and `numVideoChannelsGvc`. The
  bundle defaults are not literals, and bit 512 in the copied mask speaks only for the guest ring.
- What does the service answer to an `updateMediaDescriptions` POST (an empty 2xx, or a body with
  links)? Does it ever POST to our `/call/updateMediaDescriptions` callback?
- Must a user toggle's `mediaDescriptions` carry the service's `negotiationTag`? In the client it is
  normally undefined on a toggle. Echoing it is a late sub-variant if the plain body is refused.
- Does our own roster stream carry a `sourceId` before it publishes? (This is why `own_streams_of`
  does not require one.)
- Does Chrome write `msid` and `ssrc` for a trackless `sendrecv` section? (Two-peer spec (1).)
- Is the client's 13-section join really audio + 10 video + sharing + data? (D3 answers it.)
- Does Chrome expose platform background blur on the user's devices? This only matters if effects
  are ever picked up again.

## 13. Review notes

Two adversarial reviews: code and protocol accuracy, and project rules, safety and regressions.
Each claim below was re-checked in the code or the bundle before it was accepted.

**Accepted and changed:**

1. **The client opens no content-sharing session for a screen.** Verified: `startScreenSharing`
   acquires, enables, renegotiates and never calls `startContentSharingAsync`. The session is `Uo`,
   the PPT-Live object. The design's "screen via `call_start_sharing` first" and the RPC's
   "sharing requires the session held" are removed, `no-content-sharing-session` is the first step
   after R0, and the CLAUDE.md and NATIVE-CALLING.md claims are scheduled for correction.
2. **"The client's layout exactly" was false.** Verified: `numVideoChannels` entities are created,
   and only the first `maxStreamsForModality` are reinviteless; `getMediaDescriptions` lists only
   those. The 2+1 layout is labelled a guess. A ten-camera variant and cap variants were added.
   Caps came as **named modifiers**, not the free `video:1,sharing:1` parameter grammar the reviewer
   suggested: a parameter grammar would be a second parser on both sides of the socket, and a
   closed list of names is validated once each.
3. **R4 did not differ from 2026-08-06 in "exactly two variables".** Verified in git (`b26b9e0` on
   2026-08-06; `27b040b` and `a357c18` later; the 652-million SSRC range was live on that run). R4
   is now R4a (replay) through R4d.
4. **The 512 bit is ORed on 1:1 too, and a session is reinviteless only with `callUsesMixer`.**
   Verified (`getEndpointCapabilities`, `isReivitelessEnabled`, `v8` fed only `*Multiparty`). R1 is
   meetings-only, and its empty-body 400 is a non-result.
5. **"Publishes nothing" was asserted, not proven; the mask's provenance was overstated.** The
   roster rail was added, the gating sentence downgraded, and the provenance recorded: copied in
   `fc43c1e` from the only real-client request ever captured, a guest's.
6. **The takeover guard reversed a documented rule and would break `calling.spec.ts:827`.**
   Verified. The local refusal is dropped and became decision D1. The measured "victim" was the
   user's own device (NATIVE-CALLING.md:1583-1587), and CLAUDE.md's "colleague" is corrected.
7. **`stop()` → `inactive` and `noteAccepted` counting only `sendonly`.** Verified at
   `call-media.ts:853` and `:829`. Both fixed under the layout switches.
8. **The MCP capture of real Teams.** Removed. CLAUDE.md forbids driving a live page with the MCP
   tools, a guest capture cannot answer the ECS question, and it routed around a guard gap. It is
   replaced by the user's own signed-in sanitized HAR (D3), with the guard gap closed (D2).
9. **`answerCall` with `kind === 'call'`.** Verified (`server.rs:14983-14985`): a group invite is
   `CallKind::Call`. First replaced by `fromMixer`, which the client reads from
   `callNotification.mediaContent`; then, in the cross-workstream pass, by video-receive's one
   `is_one_to_one()` predicate over the service's `isMultiParty` (README § 3, collision 6), because
   `fromMixer` says how the media travels, not how many parties the call has. `fromMixer` stays a
   journaled fact.
10. **Answer acceptance is not confirmation; `streams_of` drops sourceId-less streams; the inline
    answer path; no roster on a 1:1.** All verified. Confirmation is S3 from our own streams only,
    parsed without a `sourceId`. The 1:1 limit is stated.
11. **Citation fixes.**
    - Acceptance links merge at `server.rs:14682`, not `:8211`.
    - The answer builder is not `createAnswerAsync`.
    - The msid fixup uses the m-line index, under `enableLocalAddMsidFixup`.
    - `toMsSdp` adds signaling feedback to answers too, so offer-only is a labelled deviation.
    - `maintain-framerate` is Safari's (dropped).
    - The guard pin did not check both alternations (now strengthened).
    - `negotiationTag` may sit at the `mediaContent` level.
12. **BW seed.** It is "first-run 600000", and the client's reinviteless offer carries it, so
    `bw-seed` is a recorded difference and a stackable sub-variant.
13. **Unlisted callers.** `join_meeting`, `place_call`, the probe example and the ~20 test sites
    stay on default wrappers. How experiments attach to sessions is specified, and the answering
    branch refuses them. `reserved` is no longer a client param.
14. **`call-live --share` had no screen rails.** Verified (`call-live.ts:138-146`). The headless rail
    was added, and fresh consent (D5) is required before video reaches the colleague.
15. **R7 and the default-path pending UI had no milestone, and M6 waited on M5.** R7 is dropped
    (§ 3). The pending UI is split: in-flight in M2, awaiting in M5/M6. M6 is conditional, and every
    ladder milestone ships default-off to both installs.
16. **The track-first order sends RTP before the gate.** POST-first is the default, and the client's
    order is a driver-only switch. The reviewer's alternative, "require `write_lock_status=held`
    before attaching", is **rejected**: it proves the token at one instant and cannot pre-check the
    other refusals (not connected, layout). POST-first covers all of them.
17. **Exact-JSON pins for every builder, and inbound fields on their own type.** Both adopted. The
    pins are M1's first commit.
18. **Promotion re-verified audio with silent fake-microphone bytes only.** The user's own listening
    check (D6) and `--tone` audio energy were added.
19. **No in-flight guard.** Verified (`store.ts:2917-2953`). Added in M2, with an RPC-counting spec.
20. **The mock's `call_start_sharing` alignment would silence a leak detector.** Verified
    (`calling.spec.ts:800-808`). Aligned, with a replacement counter asserted at 0.
21. **The behavioural gate loop covered only four methods.** Widened.
22. **Backward compatibility of `sending_confirmed`, and the release-on-timeout scope.** Absent reads
    as confirmed. Release is scoped to calls where a confirmation signal exists (reinviteless in M5,
    the default after M6), not to every call.
23. **There is no dry run in `join-live`.** Added `--check-only`, which reads `known_experiments`
    from the `call_status` reply with no join.
24. **The insecure-origin share control on a phone.** Gated on a fine pointer.
25. **A `sendrecv` camera mid receives.** `isOwnSendMid` keeps those tracks out of the tiles, and
    two-peer spec (5) records the browser's behaviour.
26. **Missing doc updates.** Each is tied to its milestone: the mock sentence, the takeover bullet,
    § 8/8a/8b, § 10.7a, the guard comment, and the `call.ts:136-143` doc (fixed now, in M1).
27. **Experiments read in `answerCall`.** Removed, and the backend refuses them on the answering
    branch.

**Rejected, or accepted with a different form:**

- **"Refuse an update naming a mid the server never negotiated, because the client filters them."**
  The premise is wrong: `filterUnnegotiatedMidsInMediaDescriptions:!1` is the client's default, so
  the client does not filter. The rule is kept as this app's own safety rail (a zeroed section is
  stopped in the browser), and labelled as such.
- **Per-kind caps as a free parameter string.** Accepted in substance and rejected in form; see
  item 2.
- **"Let the guard yield when experiments are set."** Moot: no local refusal is built (D1).
- **`write_lock_status` before `replaceTrack`.** Rejected in favour of POST-first; see item 16.
