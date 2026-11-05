# Media robustness, devices and pre-join

**Workstream:** `media-robustness-devices`. **Status:** final design, after two adversarial
reviews (§ 13). **Scope:** keeping a call the user is ALREADY in alive, audible and
controllable, and stopping a meeting join from opening a live microphone nobody has checked.

## 1. Summary

A teams-lite call today survives exactly one thing: nothing going wrong. The phone layout can
push the Hang up button off the screen. A transport failure hangs the call up for both sides and
says nothing. A page that closes or crashes leaves the far side talking to silence. A
calling-socket reconnect that moves the callback address ends the call locally and tells nobody.
The microphone is whatever the OS default is, with no way to change it mid-call. A meeting join
opens the microphone into the room the instant Join is pressed.

This design fixes those in eleven milestones (§ 10). Each milestone ships on its own, and each one
that changes protocol is behind a switch whose OFF position is today's behaviour, so the verified
audio path (a one-to-one both ways and a meeting join) cannot regress by default. The first
milestone changes nothing and only MEASURES what the rest depends on. Four new automatic acts are
proposed, and each one only maintains or ends a call the user is already in. They need the user's
approval before any becomes a default (decision D1, § 12), because CLAUDE.md § Audio calls says
"nothing in this feature ever acts on its own". That rule is already not literally true (§ 6.3), and
the proposed amendment states what is.

## 2. Where we are today

Every line below was re-read on 2026-11-05 at `c941850`.

**The stage on a phone.** The live call's header is one row that does not wrap
(`web/src/components/call-stage.tsx:314-394`). Below `sm` the clock is hidden
(`call-stage.tsx:929`, `hidden … sm:flex`), the title is `flex-1 min-w-0` and shrinks to nothing,
and every control is `shrink-0`. Summing the classes for a connected meeting with camera, share,
Record and chat gives about 429 px. That is 24 padding + 36 mark + 2×12 gaps + 7 round controls
×36 + 40 for Leave + 5 for the separator + 8×6 gaps. The stage root is `overflow-hidden`
(`call-stage.tsx:194`), and Leave is the last child, so on a 390 or 412 px phone Leave is the
control that gets clipped. On 360 px it is clipped even without Record (about 387 px).

There is no `env(safe-area-inset-*)` anywhere on the stage. Controls are 36 px (`StageControl`,
`call-stage.tsx:756`), Leave is 36 px tall (`:897`), the folded window's are 32 px (`size="sm"`,
`:683-700`) in a fixed 44 px bar (`h-11`, `:674`, `MINI_BAR_HEIGHT` in `web/src/lib/call-stage.ts:84`).
On the ringing card, Answer is a `Button size="sm"` and Decline is `size-9`
(`web/src/components/call-bar.tsx:143-156`).

**A transport failure ends the call for everybody, silently.**
- `openCallMedia` (`web/src/lib/store.ts:2842-2846`) reacts only to `connectionState === "failed"`,
  and its reaction is `hangUpCall()`. `"disconnected"` is ignored.
- The simulated media used against the mock gets no `onConnectionStateChange` at all
  (`store.ts:2836-2837`), so this path has never been exercised by a spec.
- `call_hangup` takes no reason and always ends with `CallEndReasonHangup`
  (`src/bin/server.rs:8369-8414`), whose label is the empty string (`web/src/lib/call.ts:345-347`).
  The user is told nothing.

**THE answer that cannot be applied hangs up too** (`store.ts:2672-2684`). A refused or unanswered
offer of ours is taken back after `MEDIA_ANSWER_TIMEOUT_MS`, 35 s (`store.ts:2970-2981`), through
`abandonCallRenegotiation` (`store.ts:2697-2706`), which rolls back, releases captures and posts a
take-back offer.

**The backend never reads a refusal of our own offers.** `paths::CALL_MEDIA_REJECTION`
(`src/calling.rs:105`) is published in every offer we send (`calling.rs:1429`), and nothing in
`server.rs` reads a frame on it. `outgoing_negotiation_at` is cleared in exactly three places:
- the POST's inline answer (`server.rs:8301`)
- an expired glare window (`:14827`)
- an answer frame (`:14913`)

Its own doc says it is also cleared "by a negotiation failure" (`server.rs:2303-2305`), and no code
does that.

**A vanished call keeps its media.** `refreshCallStatus` (`store.ts:3025-3032`) runs on every
socket connect (`store.ts:1698`). It writes `callStatus` directly instead of going through
`onCallState`, which is the one teardown path (`store.ts:2623-2626`). So a backend that restarted
and lost the call leaves the microphone and the peer connection open with nothing to hang up. On a
REJECTED request it writes the unknown status, which would also look like "no call" if it were
folded naively.

**Remote audio relies on `autoplay` alone** (`RemoteAudio.play`, `web/src/lib/call-media.ts:444-466`).
There is no `play()` call whose rejection could be seen.

**The microphone is fixed.**
- `openMicrophone` (`call-media.ts:387-401`) asks for the default input with fixed
  echo cancellation, noise suppression and AGC, mono.
- `startCallMedia` owns the stream (`call-media.ts:309`) and stops it on any failure (`:375-381`).
- `setMuted` loops over that original stream (`:1087-1089`), and the recorder is handed it
  (`audio` getter, `:1131-1135`).
- `web/src` has no `enumerateDevices`, `devicechange`, `setSinkId` or `deviceId`, and no listener
  for the mic track's `mute`, `unmute` or `ended`.

**ICE.** `GATHER_TIMEOUT_MS` is 2500 (`call-media.ts:44`). `waitForIceGathering` resolves only on
`complete` or the timeout (`:407-421`). There is no `restartIce` anywhere.
`call_ice_servers` (`server.rs:15239-15253`) returns STUN only in practice: `relay_config_in_frame`
(`calling.rs:2429`) is tried on every calling frame (`server.rs:14610-14618`) and has never matched
(NATIVE-CALLING.md:945-953).

**No health.** Nothing in `web/src` calls `getStats`. Only the drivers do
(`web/scripts/join-live.ts:859-910`, over `PEER_CONNECTION_PROBE` at `:197`).
`CallMedia.connectionState` is read nowhere (`call-media.ts:1111-1113`).

**Nobody knows which page carries the audio.** `ClientTracker` counts sockets through a
`ClientLease` (`server.rs:1617-1645`). The WebSocket loop takes one lease per connection
(`server.rs:5121`) and also leaves through `?` on any write error (`:5209-5216`). The backend holds
no page identity.

**A moved calling socket abandons the call.** The channel task (`server.rs:14478-14503`) calls
`end_call_locally("CallEndReasonReconnected")` when the surl changes. That sends nothing, so the far
side stays in a dead call, and the page says "The connection moved, so the call ended."
(`call.ts:365-366`). Every reconnect is a fresh allocation, because `connect_once` always POSTs the
nominal `{allocate}?epid=` (`src/trouter.rs:325-332`).

**Links are flattened.** `Links::collect` merges every nested `links` object, and the deepest one
wins (`calling.rs:456-477`). Every frame of the live call is merged into `call.links`
(`server.rs:14682-14691`). Content-sharing frames are the one exception, kept apart because their
`leave` would overwrite the call's (`server.rs:14653-14676`).

**The keep-alive almost never fires.** `keep_call_alive` POSTs `{}` to `Links::keep_alive()`, which
is `["keepAlive"]` only (`server.rs:15194-15210`, `calling.rs:419-421`), and no measured link set
names `keepAlive` (NATIVE-CALLING.md:592-600, :1122-1128). So no keep-alive goes out today.

**Join body.** `endpointState` is sent with no `state` (`calling.rs:1801-1806`).
`CallSession.muted` is documented as "Whether we told the service we are muted"
(`server.rs:2266-2269`), but `call_mute` moves it "whatever the service says"
(`server.rs:8439-8446`). The doc is the thing that is wrong.

**Notices** carry no action (`web/src/lib/notice.ts:46-58`).

**Reloads.** The two in-app reload buttons (`web/src/components/app.tsx:600`, `:612`) say nothing
about a live call. The automatic sign-in reload already counts a live call as work it would lose
(`store.ts:1790-1808`).

**The drivers' `hangUp` returns silently** when it finds no `[data-testid="call-hangup"]`
(`join-live.ts:1253-1258`). It runs in a `finally` with `.catch(() => {})` (`join-live.ts:550-553`).

## 3. Gaps covered, deferred and rejected

Covered (gap ids from round 1, plus the open map issues they absorb):

| Gap / issue | Where |
|---|---|
| Stage header clips Hang up; no safe area; targets below 44 px (map 44, 48, 49) | M1 |
| `refreshCallStatus` bypasses the teardown path (map 32) | M2 |
| A transport failure ends the call with no explanation (map 47) | M2 (the reason), M6 (the recovery) |
| Remote audio `play()` rejection (map 42) | M2 |
| `mobile-call-guards`: wake lock, OS taking the mic, close-tab warning | M2 |
| `call-quality-health`; no in-app stats (map 38) | M3 |
| `device-selection-hot-swap`, `call-settings-devices` (minus ring sound); map 33 | M4 |
| `audio-processing-noise-suppression` (browser constraints only) | M4 |
| `prejoin-and-join-muted` (local pre-join; the roster peek is deferred) | M5 |
| `media-reconnect-ice-restart`, `call-recovery-after-drop-or-reload` | M6 |
| `media-owner-lost`; live call with no local media (map 31); page media ownership (map 20) | M7, M7b |
| `trouter-reconnect-keeps-call` | M8 |
| `turn-relay-and-gather`; gather ceiling (map 34); relayConfig in the wrong place (map 9/63) | M0 (recon), M9 |

Deferred or rejected, with the reason:

- **WASM voice enhancement (`calling-wasmvqe`), REJECTED.** It is Microsoft's binary
  (`runtime.js` names the chunk `calling-wasmvqe`). Shipping it redistributes their model, and
  fetching it from their CDN in the page is a third-party request. M4 ships the browser's own
  `noiseSuppression` and, where `getSupportedConstraints()` reports it, `voiceIsolation`. An open
  model in an AudioWorklet is deferred: it costs phone CPU and nobody has reported the browser
  default as insufficient.
- **The pre-join roster peek (`stream: {}`), DEFERRED.** The client's join builder sends
  `u.payload.stream={}` in place of a `callInvitation` for a roster-only subscription
  (`calling-pluginless.js@1182488`). It is still a request into somebody's meeting, whether others
  see such an endpoint is unmeasured, and it changes the verified join. The local pre-join delivers
  the safety value without it.
- **Automatic rejoin, REJECTED.** Re-entering a meeting is `call_join`. Everybody sees the user
  arrive, so it is offered only as the user's click (a Rejoin action on the ending notice). The
  client's `shouldAutoRejoin` is not copied.
- **Automatic hangup of an ORPHANED call, DEFERRED to decision D2.** An orphaned call is one whose
  owner socket is gone and that nobody reclaims. Whether an iOS standalone PWA keeps playing WebRTC
  audio after its socket was suspended is unmeasured, and an automatic hangup could end an audible
  call. `ORPHANED_CALL_AUTO_END` ships as `None`.
- **QoE telemetry upload, REJECTED.** Health is computed and shown locally. Nothing from `getStats`
  leaves the page.
- **Ring sound, DEFERRED** to `ringing-alerts-history` (`incoming-ring-alerting`).
- **Content-sharing notification links re-point, DEFERRED.** The client also calls
  `contentSharingManager.updateContentSharingNotificationLinksAsync`, whose body is not in the
  snippets read, and screen send is unverified anyway. On a moved socket with a held sharing
  session, the share is ended through the existing `call_stop_sharing` path and the journal says so.
- **Copying `bundlePolicy: "max-bundle"`, REJECTED.** The client builds its peer connection with it
  (`calling-pluginless.js@2817428`, `E=…isTransportUnbundled?"balanced":"max-bundle"`). Chrome would
  then write every non-first section of an OFFER as `a=bundle-only` with port 0, and this service
  reads port 0 as rejected and never revives it (NATIVE-CALLING.md § 10.8).
- **Reconnecting on `disconnected`, REJECTED by client parity.** The client ships
  `reconnectOnDisconnect:!1` and `iceDisconnectedTimeoutMs:3e4`, so `disconnected` only draws
  "Reconnecting…" and a renegotiation waits for `failed` or 30 s of `disconnected`.
- **The multi-page handoff UI ("Take the call here", "In a call on another window"), owned by
  `incoming-and-conversation`** (`multi-page-call-ownership`). M7 delivers the primitives it needs:
  `media_owner`, `media`, `call_media_state` and `call_prepare{rejoin}`.
- **When the Share button is drawn, owned by `video-send`** (`share-button-without-capture-support`).
  M1's layout takes a `canShareScreen` flag and keeps today's condition until that gap decides it.

## 4. Protocol

Bundle offsets are the character position of the quoted text in the 2026-11-05 chunk under
`~/.cache/teams-lite-research/bundle-2026-11-05/`, re-extracted for this document.

Statuses:
- **measured**: this tenant answered it through this app.
- **read-from-bundle**: the client's own code says so; this tenant has not been asked.
- **inferred**: neither.

### 4.1 Reading a refusal of one of OUR offers (the `/call/rejection/` callback)

Today a refused camera, screen or reconnect offer is noticed only by the page's 35 s timer, and
the backend's glare window stays open for its full 35 s.

- **Method.** Backend-internal, in `handle_call_frame`, placed beside the renegotiation and answer
  readers.
- **Link.** Our own callback `{surl}callAgent/{session}/{cause}/call/rejection/`, published in every
  offer (`calling.rs:1429`).
- **Body read** (the shape this app already SENDS on glare, `calling.rs:1493-1506`):

```json
{ "mediaNegotiationFailure": {
    "sender": { "id": "…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "…" },
    "code": 0, "subCode": 0, "phrase": "…" } }
```

- **Effect.**
  - Journal `[calling] the service refused our media offer on /call/rejection/: code=… subCode=… phrase=…`.
  - Clear `outgoing_negotiation_at`.
  - Emit `call_media {call_id, kind: "refused", code, sub_code, phrase}`.

  The page routes it by the pending offer's purpose (§ 5, `call-media.ts`):
  - a `reconnect` offer goes to the reconnect machine;
  - a `send` offer (camera or screen) goes to the existing `abandonCallRenegotiation`, which is the
    same outcome as today's timeout, 35 s sooner.
- **Bundle evidence.**
  - The client's reader: `this.handleMediaNegotiationFailure=r=>{…this.handleMediaNegotiationFailureInternal(r.body.mediaNegotiationFailure,r.origin,n)}`
    (`calling-pluginless.js@1079938`).
  - Its sender: `function EV(e,t){…{payload:{mediaNegotiationFailure:{sender:{id:…`
    (`@1073280`).
  - The path table: `$P={MEDIA_ACKNOWLEDGEMENT:"/call/mediaAcknowledgement/",MEDIA_REJECTION:"/call/rejection/",…`
    (`@926851`).
- **Status.** Read-from-bundle. Whether this tenant ever sends one to teams-lite is unmeasured: every
  rejection on record was a zeroed section inside an ANSWER (NATIVE-CALLING.md § 10.8). The handler
  costs nothing if nothing arrives.

### 4.2 Recovering a failed transport on the SAME page (ICE restart)

- **Method.** `call_offer_media` (existing `OUTWARD_METHODS` entry) with a new optional param
  `reconnect: true`. It changes journaling and state only: `[calling] reconnecting media`, and the
  offer is tagged so § 4.1 and the answer path know what it was.
  - The page runs `pc.restartIce()`, then `setLocalDescription(await pc.createOffer())`, then
    `waitForIceGathering` (a restart gathers again, unlike a renegotiation answer, which skips the
    wait at `call-media.ts:1074-1077`), then `toMsSdp(local, labels)`.
  - It is queued on the existing `negotiating` chain.
- **Link.** `mediaRenegotiation`, falling back to `startOutgoingNegotiation`. Selection is unchanged
  (`server.rs:8243-8255`). It is named on the acceptance of an outgoing call and in the answer to
  an incoming one's acceptance (NATIVE-CALLING.md:596-600, :1122-1128).
- **Body.** The existing `calling::media_offer_payload`, unchanged (`calling.rs:1390-1431`). There is
  deliberately NO `clientContentForMediaController` on an offer: the client's `qH` sends none, and an
  offer body is a measured shape this design does not touch.

```json
{ "mediaNegotiation": {
    "sender": { "id": "…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "…" },
    "callModalities": ["audio"],
    "links": {
      "mediaAnswer": "{surl}callAgent/{session}/{cause}/call/mediaAnswer/",
      "rejection":   "{surl}callAgent/{session}/{cause}/call/rejection/" },
    "mediaContent": { "contentType": "application/sdp-ngc-1.0",
                      "blob": "<toMsSdp(restart offer): same mids, labels, SSRCs and fingerprint; NEW ice-ufrag/ice-pwd and candidates>" } } }
```

  `callModalities` and `sending` are computed exactly as `publishSending` does today
  (`store.ts:3008-3009`): audio plus whatever is still being sent.
- **Callbacks.** The answer comes inline (`answer_sdp`) or on `/call/mediaAnswer/`. A refusal is
  § 4.1. Glare is the existing refusal (`server.rs:14822-14859`) plus the page's backstop.
- **Bundle evidence: the client does NOT ICE-restart.**
  - `restartIce` has zero hits in `calling-pluginless.js` and in `calling-shell.js`.
  - Every client reconnect is a CLONED session negotiated as an ordinary renegotiation:
    `async resetReconnectSession(e){…n=i.clone(t,r);…}` (`@2317668`), then
    `await this.initReconnect(e),await this.reconnectSession.configureModalitiesAsync(…)`
    (`@2312243`).
  - The trigger: `async onIceDisconnect(e){if(this.wasConnected)this.raiseReconnectEvent("Bad",e);else if(!this.settings.reconnectWithNoRelays&&!this.session.hasRelayCandidates()){this.onReconnectFailed("no relays",e);return}…}`
    (`@2306338`). The "no relays" give-up applies only to a call that never connected.
  - The defaults: `reconnectTimeLimit:90*1e3,initialReconnectTimeLimit:0,…maxReconnectDelayTime:5,maxConsecutiveLocalFatalReconnects:2,waitForRelayOnReconnect:!0,relayWaitSaneTimeoutMs:25*1e3,reconnectOnDisconnect:!1,reconnectWithNoRelays:!1,…reconnectOnPcClose:!0,reconnectWakeupTimeLimit:18e4`
    (`@3061913`).
  - `iceDisconnectedTimeoutMs:3e4` (`@3072317`).
  - The light step before a reconnect only waits for the OLD session to come back online
    (`async waitForNetworkAndTryLightweightRecovery`, `@2310588`). It is not an ICE restart.
- **Why a same-page ICE restart and not the client's clone.** A fresh peer connection on the same
  page has to reproduce the call's section layout, and that is exactly what § 4.7 shows to be
  fragile (gapped service mids, an `x-data` section the browser cannot create). An ICE restart
  keeps the peer connection, its transceivers, its mids and its DTLS, and changes only the ICE
  credentials and candidates. The client's clone sends new ICE and a NEW DTLS fingerprint through
  the same door, so the change this offer makes is a subset of one the service already accepts from
  the client. That is an inference, not a measurement.
- **Status.** The door is measured: an outgoing renegotiation on `mediaRenegotiation` is answered
  and its audio section accepted (NATIVE-CALLING.md § 10.8). An ICE-restart offer specifically is
  UNMEASURED. It sits behind the page flag `teams-lite:call-exp:reconnect`, and its first
  measurement is `join-live --reconnect` in the pinned meeting (§ 9).

### 4.3 Ending a call that cannot be kept, saying why

- **Method.** `call_hangup` (existing `OUTWARD_METHODS` entry) gains an optional `reason` from a
  CLOSED set of one value, `"media_lost"`.
  - Anything else is refused at dispatch: `call_hangup: reason must be "media_lost"`.
  - A reason on a ringing incoming call is refused: a ringing call has no media to lose. That
    includes advanced-call-control M4's WAITING call, which is a ringing one.
  - The backend ends with `CallEndReasonMediaLost`, so EVERY page draws the same sentence, not only
    the page that gave up.
  - `reason` is designed together with plane-reliability M3's `intent` and advanced-call-control M4's
    waiting-call `call_id`, as one `call_hangup {call_id, intent, reason}` with one validation table
    (README § 3, collision 4).
- **Body to Teams.** Unchanged, the measured `calling::hangup_payload` (`calling.rs:1522-1540`):

```json
{ "participants": { "from": { "id": "…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "…" } },
  "conversationTransactionEnd": { "reason": "noError", "code": 0, "phrase": "ConversationEndedNoModality" },
  "callTransactionEnd": { "code": 0, "phrase": "LocalUserInitiated" } }
```

  The client's `endCallOnFailure` probably reports a failure code instead. That is not copied,
  because the body above is measured and a failure code is not.
- **Bundle evidence.** `…:(this.logger.safe.info(`[${u.causeId}] terminate a call after wakeup because allowed time limit for reconnect exceeded`),this.endCallOnFailure(u.mediaError,u.causeId))`
  (`calling-pluginless.js@2323379`).
- **Status.** A meeting leave and an outgoing one-to-one hangup are measured (NATIVE-CALLING.md
  § 8, § 8b). An incoming one-to-one hangup is unmeasured and owned by `incoming-and-conversation`.
  The reason changes only the local sentence.

### 4.4 Re-pointing the CONVERSATION's callbacks after the calling surl moved

- **Method.** Backend-internal `Ctx::relink_live_call(new_channel)`, run from the channel task in
  `start_calling` (`server.rs:14478-14503`). Only `TEAMS_LITE_CALL_SURL_MOVED=relink` runs it (§ 6.2).
- **Link.** The CONVERSATION's `notificationLinks`, held in a PINNED field,
  `CallSession.conversation_notification_link: Option<String>`. It is NOT read through the flattened
  `Links`, because a content-sharing session carries a `notificationLinks` of its own:
  - the conversation's: `this.links[I.LINKS.NOTIFICATION_LINKS]=e.links.notificationLinks`
    (`@1345412`);
  - the session's: `this.links[I.LINKS.CONTENT_SHARING_NOTIFICATION_LINKS]=e.links.notificationLinks`
    (`@1036517`).
  
  It is read from the top-level `links` of a conversation-service response: the join answer, the
  create-call answer, and conversation frames. An object that also names a sharing session's own
  links (`contentSharingController`, `updateSessionState`, `sync`) is refused.
  
  The same field and the same builder are what `incoming-and-conversation`'s
  `meeting-keepalive-long-meetings` needs for the conversation keep-alive, so whichever lands first
  owns both and the other reuses them.
- **Body.** `calling::conversation_links_update_payload(local, callbacks_on_new_surl)` re-states
  EXACTLY what the join or invitation already published (`calling.rs:1221-1235` and the
  `roster: {type: "Delta"}` at `:1247-1251` / `:1774-1775`), rebuilt on the new surl with the same
  session id and the same cause id:

```json
{ "participants": { "from": { "id": "…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "…" } },
  "roster": { "type": "Delta", "rosterUpdate": "{newSurl}callAgent/{session}/{cause}/conversation/rosterUpdate/" },
  "links": {
    "conversationEnd":        "{newSurl}callAgent/{session}/{cause}/conversation/conversationEnd/",
    "conversationUpdate":     "…/conversation/conversationUpdate/",
    "localParticipantUpdate": "…/conversation/localParticipantUpdate/",
    "addParticipantSuccess":  "…/conversation/addParticipantSuccess/",
    "addParticipantFailure":  "…/conversation/addParticipantFailure/",
    "addModalitySuccess":     "…/conversation/addModalitySuccess/",
    "addModalityFailure":     "…/conversation/addModalityFailure/",
    "confirmUnmute":          "…/conversation/confirmUnmute/",
    "receiveMessage":         "…/conversation/receiveMessage/" } }
```

  The client's `lt()` mints a fresh cause id per link (`function lt(e,t,i=Ht()){…/${e.sessionId}/${i}${t}`,
  `@931704`). Keeping ONE cause id is teams-lite's own choice, consistent with every link it has
  published so far. The client's `VP(e,lT)` sends some 40 conversation links; teams-lite sends only
  the nine it published at join, because those are the callbacks this tenant has already accepted
  from it.
  
  The response is a conversation response: its links are folded (and the pinned field re-read), and
  its roster goes through `calling::roster_in_frame` (`calling.rs:2037`) to resync what was missed
  during the gap.
- **Bundle evidence.**
  - The trigger: `this.updateLinksToTrouter=(a,o)=>{…this.previousTrouterUrl!==this.currentTrouterUrl?(this.previousTrouterUrl=this.currentTrouterUrl,this.updateConversationLinks(),this.updateKeepAliveUrl(),this.contentSharingManager?.updateContentSharingNotificationLinksAsync(o),…)`
    (`@1292463`).
  - `this.updateConversationLinks=(a=Ht())=>{…this.links.hasOwnProperty(I.LINKS.NOTIFICATION_LINKS)?(…this.sendToUpdateConversationLinks(!0)):this.linkUpdateRequested.conversationLinks=!0}`
    (`@1294245`).
  - `sendToUpdateConversationLinks(e,t=Ht()){…r=e?vG(this):{},…s=this.links[I.LINKS.NOTIFICATION_LINKS];…this.http.sendPostRequest({url:s,payload:r,withoutTrouter:!0,…}).then(a=>{…this.processConversationServiceResponse(a.response,t,2)})`
    (`@1401758`).
  - `function vG(e){…const t={type:"Delta",rosterUpdate:lt(e,lT.CONV_ROSTER_UPDATE)};return{payload:{participants:{from:{id:…}},roster:t,links:VP(e,lT)}}}`
    (`@1194936`).
  - **New finding.** The client's PERIODIC conversation keep-alive is this very body:
    `sendConversationKeepAlive(){…this.sendToUpdateConversationLinks(!0)}` (`@1401234`), every
    `conversationKeepAliveTimeoutSec:2700` (`@982507`). So `vG` to `notificationLinks` is something
    the client sends on every call that lasts about 45 minutes, not a rare body.
- **Status.** Read-from-bundle. UNMEASURED on this tenant:
  - whether `notificationLinks` is in the join and create-call answers (M0's journal line answers it
    on the next join);
  - whether a nine-link update is accepted (inferred, because the service accepted exactly this set
    at join).

### 4.5 Re-pointing the CALL LEG's callbacks after the surl moved

- **Method.** Same routine, same switch.
- **Link.** A NEW accessor `Links::call_leg()` = `["callLeg"]`, used ONLY by the relink.
  `Links::keep_alive()` is deliberately NOT extended. Extending it would make the 20 s ticker
  (`server.rs:15194-15210`) start POSTing `{}` to `callLeg` on every verified call. `callLeg` is
  also the client's HANGUP link (`this.links[I.LINKS.HANGUP]=e.callAcceptance.links.callLeg,this.links[I.LINKS.KEEPALIVE]=e.callAcceptance.links.callLeg`,
  `@1386852`), and the client's keep-alive body is `{callParticipantUpdate:{}}`, not `{}`. Moving the
  keep-alive to `callLeg` belongs to `incoming-and-conversation`'s `call-keepalive-on-callleg`,
  behind its own switch and measurement. **That milestone (incoming M6) REMOVES `keep_alive()` and
  replaces the 20 s ticker with the 5 s `CALL_TICK`** (README § 3, collision 4), so this design pins
  the BEHAVIOUR, not the accessor: a relink is a ONE-SHOT POST and starts no periodic one, so the
  ticker posts exactly what it posted before the relink. (The pin cannot be "relink never posts to
  the keep-alive door": once incoming M6 lands, the call keep-alive's door IS `callLeg`, which this
  relink posts to once, with its links.) Whichever of the two lands second keeps that pin green
  rather than breaking the other's.
- **Body.** `calling::call_participant_update_payload(callbacks_on_new_surl, true)`, whose links are
  EXACTLY the union of the call callbacks teams-lite already publishes across its invitation,
  acceptance, answer and offer builders. That is fifteen links, `mediaAcknowledgement` included
  (`calling.rs:1140-1147`, `:1184`, `:1284-1288`, `:1368`, `:1429`). A test diffs the list against
  those builders.

```json
{ "callParticipantUpdate": {
    "links": {
      "progress": "{newSurl}callAgent/{session}/{cause}/call/progress/",
      "mediaAnswer": "…/call/mediaAnswer/", "acceptance": "…/call/acceptance/",
      "redirection": "…/call/redirection/", "end": "…/call/end/",
      "mediaRenegotiation": "…/call/mediaRenegotiation/", "transfer": "…/call/transfer/",
      "replacement": "…/call/replacement/", "balanceUpdate": "…/call/balanceUpdate/",
      "retargetCompletion": "…/call/retargetCompletion/",
      "controlVideoStreaming": "…/call/controlVideoStreaming/",
      "updateMediaDescriptions": "…/call/updateMediaDescriptions/",
      "csrcInfo": "…/call/csrcInfo/", "mediaAcknowledgement": "…/call/mediaAcknowledgement/",
      "rejection": "…/call/rejection/" },
    "clientContentForMediaController": { "controlVideoStreaming": "…", "csrcInfo": "…" } } }
```

- **Bundle evidence.**
  - `function pM(e,t){…const i=e.getSharedLineCallContent(),r={};return t&&(r.links=VP(e,$P),r.clientContentForMediaController=e.webRtcSignalingManager.getClientUrls()),…{payload:{callParticipantUpdate:r}}}`
    (`@1186832`).
  - `this.updateKeepAliveUrl=()=>{…this.links.hasOwnProperty(I.LINKS.KEEPALIVE)?(…this.sendToKeepAliveUrl(pM(this,!0))):this.linkUpdateRequested.keepAliveLinks=!0}`
    (`@1293950`).
  - `function VP(e,t){const i={};for(const r of Object.values(t)){const n=r.split("/");i[n[n.length-2]]=lt(e,r)}return i}`
    (`@933673`). Note the client's own `$P` keys `UPDATE_MEDIA_DESCRIPTIONS` without a trailing slash,
    so its `VP` names it `call`. teams-lite writes the explicit name.
- **Status.** Read-from-bundle, UNMEASURED.
  - An outgoing call still DIALING has no `callLeg` yet, and its acceptance would land on the dead
    surl. The client defers (`linkUpdateRequested…=!0`) until a link arrives, which here never will.
    So that case falls back to § 4.6's cancel.
  - An INCOMING call that is still RINGING is never touched (§ 4.6).

### 4.6 What a moved surl does when the call cannot follow it

`hang_up_live_call(reason)`, by phase:
- **Incoming and Ringing: end LOCALLY only**, as today. Rejecting is a decline the caller is SHOWN,
  and on this tenant's forked legs it can stop the ring on the user's other devices too.
  `expire_unanswered_call`'s own rule says a timer must not decide that (`server.rs:15151-15167`).
- **Outgoing and Dialing or Ringing: cancel**, by POSTing `hangup_payload` to `Links::hangup()` when
  the create-call answer named one, with the journal line `[calling] cancelling the ring: the calling
  socket moved and the answer could not reach this machine`. With no link, end locally.
- **Connecting or Connected: hang up**, then `end_call_locally("CallEndReasonReconnected")`. The new
  label is "The connection to Teams moved and the call could not follow it."

The page's `pagehide` hangup (§ 4.9) uses the plain `call_hangup`, with no new body. The client does
the same: `this.windowUninitListener=()=>{…this.callsBeaconedOnLeave=!0,…this.stopCallsWithBeacon()},window.addEventListener("pagehide",this.windowUninitListener),this.pageShowListener=()=>{this.callsBeaconedOnLeave=!1},window.addEventListener("pageshow",…)`
(`@1993958`). Its close speedbump is OFF by default (`enableBrowserCloseSpeedbump:!1`, `@3084136`),
so the client hangs up on `pagehide` with NO prompt.

**Status.** Hang-up and cancel bodies are measured. Every automatic use of them is behind a switch
(§ 6.2) and in decision D1.

### 4.7 Keeping the SAME surl across a calling-socket reconnect (trouter resume)

- **Method.** In `src/trouter.rs`, `Endpoint` gains `resume_allocation`, true for `Endpoint::calling`
  only, and only when `TEAMS_LITE_TROUTER_RESUME=1`. `connect_once` caches `{socketio, connectparams,
  se}` after an allocation. The next attempt POSTs to the cached instance:
  `{socketio}v4/a?epid=<epid>&<connectparams as k=v>&v=v4`, with `X-Skypetoken` and
  `content-length: 0` as today.
- The cache is dropped when `se` is within 1 h, or after a 4xx. Either way the attempt falls back to
  the nominal `{allocate}?epid=` the code uses today.
- **Bundle evidence (`cmd-trouter.js`).**
  - `populateAndCacheReconnectParams(e,t){this.reconnectParams={serviceUrl:this.options.trouterUrl,reconnectUrl:t,...e.connectparams},…}`
    (`@95565`).
  - `sendAllocateRequest(e){…r&&r<=Date.now()+36e5&&(this.logger.warn(`Dropping expired cached connection parameters: …`),this.reconnectParams=c=void 0),…c?.reconnectUrl&&(n=c.reconnectUrl),t=c?{...c,serviceUrl:void 0,reconnectUrl:void 0}:null,n=X(n,"v4a"),…t&&(n+=`&${this.buildQuery(t)}`,t.v||(n+="&v=v4"))`
    (`@70921`).
- **Status.** Read-from-bundle, UNMEASURED. Whether teams-lite's calling surl moves at all today is
  unmeasured too. M0's journal line answers that over a few days of the always-on service before
  anything is built.

### 4.8 Re-establishing media from a page with NO peer connection (rejoin, narrowed)

- **Method.** `call_prepare {call_id, rejoin: true, page}` (existing `MACHINE_METHODS` entry). It
  moves `media_owner` to `page` and answers `{call_id, ice_servers, layout:[{kind, mid, label,
  direction}], muted}`. Then `call_offer_media {reconnect: true, sending: []}` (OUTWARD) with an offer
  from a NEW `RTCPeerConnection` whose transceivers replay `layout` in order.
- **The narrowing.** A fresh peer connection cannot set a mid: `createOffer` numbers transceivers
  `0, 1, 2…`. The measured meeting layout is gapped and service-chosen: `0` audio, `3` application
  sharing, `4` an `x-data` section, with `1` and `2` reserved (NATIVE-CALLING.md:1214-1216).
  Teams-lite answers that `x-data` section REJECTED (NATIVE-CALLING.md:661-663), and the browser
  cannot create one at all. So `can_rejoin_media` is true only when the recorded layout's mids are
  exactly `0…n-1`, every section is a kind the browser can create (`audio` or `video`), none is
  rejected, and nothing is being sent.
  
  In practice that means an outgoing audio call that never renegotiated, and a meeting nobody else
  has published into. The page REFUSES to post a replay whose generated mids differ from the
  recorded ones. Every other call offers Hang up for a one-to-one, and Leave and rejoin (through the
  pre-join, `call_join`, the user's click) for a meeting.
- **`sending` is reset to `[]`.** The new peer connection carries no camera or screen, and publishing
  the old `sending` would tell every page a camera is on.
- **Layout recording.** Every handler that forwards an SDP records `calling::media_layout`:
  `call_place`, `call_accept`, `call_join`, `call_answer_media`, `call_offer_media`, and the answers
  that come back inline or on frames. A section is rejected when its port is 0 on either side.
- **Bundle evidence.** The client's clone is a new session (`@2317668`), and `completeReconnect`
  disposes of the old one. The mechanism is the client's; the narrowing is ours.
- **Status.** Read-from-bundle, UNMEASURED, behind the page flag `teams-lite:call-exp:rejoin`.

### 4.9 Which page carries the audio (local, nothing reaches Teams)

- **This is the ONE page-ownership mechanism** (README § 3, collision 2). incoming-and-conversation M8
  consumes it in `callMediaPlacement` and adds no param of its own.
- **New RPC `call_media_state {call_id, page, holding}`**, a `MACHINE_METHODS` entry (§ 6.1).
  `page` is 32 hex characters minted with `crypto.getRandomValues`, NOT `crypto.randomUUID()`: that
  one exists only in a secure context, and this app is reached over plain http on overlay networks.
  It is kept in `sessionStorage` with incoming-and-conversation § 4.15's duplicate-tab check (a
  duplicated tab copies `sessionStorage`, and mints a fresh id when a live tab answers `taken`).
  `call_prepare` and `call_accept` also take `page`; `call_prepare` records the page that RESERVED the
  call as `media_owner`, so no outward RPC changes shape for this.
- **Precedence.** Only `call_prepare` (a new call, or `{rejoin: true}`) ever MOVES `media_owner`.
  - `holding: true` from the owner re-announces it after a reconnect and clears "lost".
  - `holding: true` from any other page is answered `{accepted: false, media_owner}`, and that page
    stops its own media on the one teardown path. So a laptop waking up can never take the call back
    from a phone that rejoined.
  - `holding: false` from the owner (its `pagehide`) sets `media: "released"` at once.
- **Socket tracking.** A per-connection `PageLease`, beside `_client_lease` (`server.rs:5121`), holds
  the page ids that connection announced. Its `Drop` calls `Ctx::page_socket_closed(ids)`, which
  covers every way out of the loop, the `?` exits at `:5209-5216` included. When the owner's socket
  has been gone longer than `MEDIA_OWNER_GRACE` (45 s, checked by the existing ticker at
  `server.rs:14535-14545`), `media` becomes `"lost"`.
  
  The owner page also stops giving up on its socket while it holds a live call's media (§ 5,
  `ws-client.ts`). The page's own backoff otherwise stops at 30 s (`RECONNECT_GIVE_UP_MS`,
  `ws-client.ts:225`) and waits for a wakeup (`retryNow`, `:349-358`), and a visible page whose
  audio still flows would then read as lost.
- **`call_state` gains** `media_owner` (string or null), `media` (`"held" | "released" | "lost" |
  "unknown"`), `can_reconnect_media` (Connected and a renegotiation link) and `can_rejoin_media`
  (§ 4.8).
- **No automatic hangup of an orphan** (`ORPHANED_CALL_AUTO_END: Option<Duration> = None`), per
  decision D2.
- **Bundle evidence.** There is no Teams equivalent, because the client holds signaling and media in
  one page. Its nearest rule is page-local and runs when the page WAKES: `pcClosedInfo={timestamp,causeId,mediaError}`
  then, on visible, `Date.now()<d.timestamp+…reconnectWakeupTimeLimit?…startReconnect(…):…endCallOnFailure(…)`
  (`calling-pluginless.js@2339435`, `@2323379`; `calling-shell.js@2405773`). M6 copies that rule into
  the owner page.
- **Status.** Local design, nothing to measure at the service. `pagehide` delivery over an open
  WebSocket is unmeasured (`join-live --close-tab`, § 9).

### 4.10 Joining a meeting muted

- **Local (the default and the shipped half).** The pre-join's stream is handed to the call with
  `track.enabled = !muted` set BEFORE `createOffer`, so no audio sample ever leaves. Then
  `call_prepare {meeting, subject, page, muted: true}` sets `CallSession.muted` before the first
  frame, so every page draws the call muted. `CallSession.muted`'s doc is corrected to what the code
  already does: the user's mute state as this machine holds it, with publication tracked by
  `call_mute`'s `told_service`.
- **Announced (page flag `teams-lite:call-exp:announce-muted`, default OFF).**
  `call_join {…, announce_muted: true}` makes `join_payload` add the client's field. Without the flag
  the body is byte-for-byte today's (`calling.rs:1801-1806`):

```json
"endpointState": { "endpointStateSequenceNumber": 0,
                   "state": { "isMuted": true },
                   "endpointProperties": { "additionalEndpointProperties": { "infoShownInReportMode": "FullInformation" } } }
```

- **Bundle evidence.** `this.configureEndpointStateForStartOrJoin=a=>{const o={endpointStateSequenceNumber:this.endpointStateSequenceNumber};let l=!1;if(a&&(a.muted&&(l=!0,Object.assign(o,{state:{isMuted:a.muted}})),…`
  (`@1252068`).
- **Status.** Local is nothing to measure. Announced is read-from-bundle, UNMEASURED; the `--pair`
  rig's roster digest shows whether the meeting reads it (§ 9). Unmuting later is `call_mute`, whose
  body is `incoming-and-conversation`'s (`self-mute-not-visible-in-meetings`).

### 4.11 TURN relay credentials, and a gather rule that waits for them

- **Source.** Unknown. The client receives `relayConfig` in the ACCOUNT CONFIGURATION its host hands
  the call registry (`…this._relayManagerProvider.getOrCreateRelayManager().setDefaultRelayConfig(n.relayConfig),…setClientSupportsGenericTokenApi(…)`,
  `@3113943`), and ALSO from an injected provider (`const t=await this.context.relayConfigProvider()`,
  `@775719`). Its host wires that in as `this.config.relayConfigProvider` (`calling-shell.js@812586`),
  which no downloaded chunk implements.
- **Credentials.** The token fetch supports three token types: `buildHeaders(e,t){…i["api-version"]=2,…case 4:case 8:return i.Authorization=`Bearer ${t}`,i;case 1:return i["X-Skypetoken"]=t,i;…}`
  (`@776549`), plus TRAP credentials through `trapTokenCredentialsProvider` (`@781812`). teams-lite
  already implements the skypetoken GET (`calling::fetch_relay_credentials`, `calling.rs:2406-2424`).
  So M0's recon names WHERE `relayConfig` lives and also which `www-authenticate` a token URL
  answers. No endpoint is guessed.
- **Gather rule (backend switch `TEAMS_LITE_CALL_TURN=1`, default OFF).** The switch decides whether
  `call_ice_servers` hands TURN to the page at all. With TURN servers, the page resolves the gather
  wait on the FIRST component-1 `typ relay` candidate or on `complete`, with a 20 s ceiling. With no
  TURN, today's 2.5 s and `complete` rule is unchanged.
  
  That is the client's rule for a one-to-one session with ICE servers. For a MEETING the client
  stops at the first candidate of any kind (`iceHostCandidateOnly`), and teams-lite keeps its own
  rule there, because it works.
  - `if(!this.relayCandidateGathered&&b.candidate.indexOf("typ relay")>-1){…this.relayCandidateGathered=!0}…k=this.iceHostCandidateOnly||l.length===0;if(!b&&y||k){…"Not waiting for ICE candidates, hoping that host/prflx is enough"…this.completeCandidateGathering(t)}else if(this.relayCandidateGathered){…this.completeCandidateGathering(t)}`
    (`@2821967`, where `l` is the peer connection's `iceServers`, `@2817178`).
  - `this.iceHostCandidateOnly=this.multiParty&&Rg("iceHostCandidateOnly",…)` (`@2792937`), with the
    default `iceHostCandidateOnly:!0` (`@3061749`).
  - The ceiling: `setupIceGatheringTimeout(e){…let t=…webrtcIceGatheringTimeoutIncreased?…relayWaitSaneTimeoutMs:…webrtcIceGatheringTimeoutMs;if(this.useStun&&…webrtcIceGatheringStunTimeoutMs&&(t=…StunTimeoutMs)…`
    (`@2753027`). Here `useStun` is the client's `forceStun` mode, which this app does not have, so
    the 5 s STUN figure does not apply.
  - The defaults: `webrtcIceGatheringTimeoutMs:2e4,webrtcIceGatheringStunTimeoutMs:5e3` (`@3059854`).
- **Status.** Token GET is read-from-bundle and implemented; the source is unknown (M0 recon first);
  the gather rule is read-from-bundle, behind the switch.

### 4.12 Devices, health and speaking-while-muted (local)

- **Switching devices.** `RTCRtpSender.replaceTrack` on the audio and camera senders needs no
  renegotiation for the same codec. Speakers use `HTMLMediaElement.setSinkId` on every remote
  element, feature-detected, falling back to `navigator.mediaDevices.selectAudioOutput` on Firefox.
  - The client enumerates on change: `…mediaDevices.ondevicechange=()=>{n.safe.info("enumerating due to ondevicechange callback"),Nt.enumerateDevices()…}`
    (`@2132302`).
  - Its sink switch: `…audioSetSinkIdWorkaround&&this.stream&&!this.stream.active?(this.audio.srcObject=null,await this.audio.setSinkId(e),this.setAudioSource(this.stream)):await this.audio.setSinkId(e)`
    (`@2172062`).
  - Its devices panel draws `renderSpeakers`, `renderMicrophones` and `isInPrejoin`
    (`calling-devices-panel.js@4795`).
- **Health.** `pc.getStats()` every 5 s while connected and visible (`statsIntervalSec:5`,
  `@3070504`). The thresholds are teams-lite's own (§ 5, `call-health.ts`), not the client's.
- **Speaking while muted.** An `AnalyserNode` on a CLONE of the mic track: the sent track is disabled
  when muted, and a disabled track feeds silence to Web Audio. The clone never leaves
  `call-health.ts`, and `CallAudio.microphone` stays the sent track (a test pins it).
  - The 2-of-3 window at a normalized level of 0.1 is teams-lite's OWN rule. The client's shipped
    defaults leave its detector INACTIVE: `Jt` is a mean
    (`function Jt(e){return e.reduce((t,i)=>t+i,0)/e.length}`, `@736673`), compared as
    `Jt(this.speechWindow.items.map(n=>n?1:0))>this.settings.speakingWhileMutedSpeechThreshold`
    (`@1538432`) against a threshold of `1` (`@3061913`), and a mean of 0/1 values is never above 1.
  - It is NOT run on iOS WebKit (`readPushEnvironment().appleMobile`, `web/src/lib/push.ts:125-126`)
    until a real-phone check shows that an `AudioContext` with a mic source does not reroute an
    ongoing call's audio.
- **Wake lock.** The client re-requests on visibility: `document.visibilityState==="visible"&&(…(!this.wakeLock||this.wakeLock?.released)&&this.requestWakeLock())`
  (`calling-shell.js@2405773`).

### 4.13 Pre-join roster peek: deferred

The join POST with `"stream": {}` and no `callInvitation` (`@1182488`). It is read-from-bundle and
deliberately not built (§ 3).

## 5. Changes by file

Grouped by file, with the milestone that brings each change in brackets.

**Backend: `src/calling.rs`**
- [M0] `surl_digest(surl)`: 8 hex of SHA-256 of the whole surl, never the id. It sits beside the
  existing `surl_shape` (`calling.rs:268`).
- [M5] `join_payload(…, announce_muted: bool)` adds `endpointState.state.isMuted` only when asked.
  Its test asserts byte-for-byte equality when false.
- [M6] `media_negotiation_failure_from_frame(frame) -> Option<NegotiationFailure{code, sub_code, phrase}>`.
- [M7b] `MediaSection {kind, mid, label, direction, rejected}` and `media_layout(sdp)`, over the
  same reader `media_sections` uses (`calling.rs:727`).
- [M8]
  - `CallbackBase::with_surl(&self, surl)`, which keeps `session_id` and `cause_id`.
  - `Links::call_leg()`.
  - A pinned `conversation_notification_link(response)` reader, shared with
    `meeting-keepalive-long-meetings`.
  - `conversation_links_update_payload(local, callbacks)` and `call_callback_links(callbacks)`, the
    15-link union, pinned against the builders.
  - `call_participant_update_payload(callbacks, with_links)`, where `false` yields
    `{callParticipantUpdate:{}}` for `call-keepalive-on-callleg` to reuse.
- [M8]: `Links::keep_alive()` is not EXTENDED by this design (incoming-and-conversation M6 may remove
  it; § 4.5).

**Backend: `src/trouter.rs`**
- [M8] `Endpoint.resume_allocation`, and a pure `allocate_request_url(endpoint, epid, cached, now)`
  with tests (cached URL, expired `se` falls back to nominal, a 4xx clears the cache).

**Backend: `src/bin/server.rs`**
- [M0] Journal lines, all diagnostic:
  - every calling-socket callback publication gets `surl unchanged` or `surl moved (a1b2c3d4 → e5f6a7b8)`;
  - `call_join` and `call_place` say whether their answer named a top-level `notificationLinks`;
  - the first `relayConfig` ever seen names its key PATH only.
- [M2] `call_hangup` gains `reason` (closed set `{"media_lost"}`, refused otherwise, and refused on
  an incoming ringing call). It ends with `CallEndReasonMediaLost`.
- [M2] `CallSession.muted`'s doc corrected.
- [M5] `call_prepare` accepts `muted` and `page` on every shape. `call_join` accepts `announce_muted`.
- [M6]
  - `handle_call_frame` reads § 4.1 and emits `call_media {kind: "refused"}`.
  - `call_offer_media` accepts `reconnect` (journal, state).
  - `call_state` gains `can_reconnect_media`.
  - Every SDP-forwarding handler records `media_layout` (used by M7b).
- [M7]
  - `CallSession` gains `media_owner`, `media_owner_connected`, `media_owner_absent_since`,
    `media_released`.
  - `call_state` gains `media_owner` and `media`.
  - New `call_media_state` arm, added to `MACHINE_METHODS` and with a `machine_effect` arm
    (`server.rs:610-719`): "decides which open page this machine treats as carrying the call's audio".
  - A `PageLease` (a `Drop` guard) per WebSocket connection, filled from the `page` param of
    `call_prepare` and `call_media_state`.
  - `Ctx::page_socket_closed`.
  - The ticker checks `MEDIA_OWNER_GRACE` (45 s). `ORPHANED_CALL_AUTO_END = None`.
- [M7b] `call_prepare {call_id, rejoin, page}` answers the layout and refuses (no connected call, or
  an unreplayable layout). The `can_rejoin_media` rule.
- [M8]
  - `TEAMS_LITE_CALL_SURL_MOVED` = `end-locally` (default) | `hang-up` | `relink`.
  - `hang_up_live_call` (§ 4.6) and `relink_live_call` (§ 4.4 and § 4.5).
  - `CallSession.conversation_notification_link`.
  - A held sharing session is ended through the `call_stop_sharing` path on a relink.
- [M9] `TEAMS_LITE_CALL_TURN=1` gates TURN in `call_ice_servers`, and `relayConfig` is read from
  wherever M0's recon names.

**Backend: `examples/relay_config_recon.rs` (new, M0)**
- READ-ONLY, in the shape of `examples/calling_endpoint_recon.rs`. It prints the key PATHS matching
  `/relay|turn|trap|ice/i`, never values, of every JSON this app already GETs with its session (the
  authz directory and the calling endpoints `calling::endpoints` reads).
- For any `tokenUrl` found, it makes one unauthenticated GET and prints its `www-authenticate` scheme
  names only.
- No POST, no registration, no lifecycle call. It names no send endpoint, so
  `guard-live-automation.sh`'s example rules (`:751-802`) let it run.

**Web: `web/src/lib/store.ts`**
- [M0] Wire `onConnectionStateChange` to `simulatedCallMedia` as well, in a commit with no behaviour
  change, so the mock can drive the transport.
- [M2]
  - `refreshCallStatus` folds a SUCCESSFUL answer through `onCallState`. A REJECTED request leaves
    `callStatus` as it is while a call is live, and keeps today's unknown status otherwise.
  - The `failed` hangup passes `reason: "media_lost"`.
  - The screen wake lock is held while connected and visible, re-requested on `visibilitychange`,
    and released in `stopCallMedia`.
  - `beforeunload` is armed only while this page holds a live call's media and
    `navigator.userActivation.hasBeenActive`.
  - The ws client keeps retrying while this page holds media.
  - Mic-track `mute` / `unmute` / `ended` become `callMicInterrupted`.
- [M3] Stats polling (5 s, paused when hidden), `callHealth`, `speakingWhileMuted`.
- [M4]
  - `openCallMedia` gets the microphone from `call-devices.ts` and passes it to `startCallMedia`.
  - `switchCallDevice(kind, id)`.
- [M5]
  - `requestMeetingJoin(meeting, subject)` opens `preJoin`.
  - `joinMeeting(meeting, subject, {muted, announceMuted, microphone})`.
  - The pre-join stream is released on Cancel, unmount, a conversation change, the page being hidden,
    and after 120 s idle.
- [M6]
  - The transport handler feeds `CallReconnect` instead of calling `hangUpCall`.
  - `call_media {kind: "refused"}` is routed by offer purpose.
  - `boundTheMediaAnswer` hands a reconnect offer's timeout to the machine, never to
    `abandonCallRenegotiation`.
  - `reconnectCallMedia()`.
  - `lastJoin` is remembered for the ending notice's Rejoin.
- [M7]
  - `pageId`.
  - `call_prepare` carries `page`.
  - `call_media_state` on (re)connect while holding media.
  - `pagehide`: always `holding: false`. With `teams-lite:call-exp:pagehide-hangup` on and
    `event.persisted === false`, `call_hangup` instead.
  - Stop own media when `media_owner` moves to another page, or when a claim is refused.
  - `rejoinCallMedia()` [M7b].

**Web: `web/src/lib/call-media.ts`**
- [M2] `RemoteAudio.play` calls `audio.play()` explicitly after `srcObject` and reports
  `NotAllowedError` through `onPlaybackBlocked`. Mic-track listeners.
- [M3] `getStats()` passthrough.
- [M4] `CallMedia` holds the CURRENT sent track(s):
  - `setMuted`, `stop()` and `audio` all read it.
  - `startCallMedia({microphone?})` OWNS the stream it is given exactly as it owns the one it opens.
    Its catch still stops it (`:375-381`), and `adoptCallMedia`'s refusal still stops it through
    `media.stop()`.
  - `replaceMicrophone(stream)` sets `enabled = !muted` on the new track BEFORE `replaceTrack`, then
    stops the old one and fires `onAudioChange` (the recorder re-points).
  - `replaceCameraTrack(track)` and `setOutputDevice(id)`, applied to existing and future elements.
- [M6]
  - `restartIce(): Promise<string>`, queued on `negotiating`.
  - The pending offer carries a purpose (`send` | `reconnect` | `takeback`).
  - `rollbackPendingOffer()`.
- [M7b] `startCallMedia({layout})` refuses (a typed error) when the generated mids differ from the
  layout.
- [M9] The gather rule of § 4.11, keyed on whether the ICE servers include `turn:` / `turns:`.

**Web: new pure modules**
- `web/src/lib/call-devices.ts` [M4]:
  - Prefs in localStorage `teams-lite:call-devices`: `{microphoneId, speakerId, cameraId,
    joinMeetingsMuted, noiseSuppression}`.
  - `micConstraints(prefs)`. With NO stored microphone the constraints are byte-for-byte
    `openMicrophone`'s (pinned by a test). A stored id is `deviceId: {exact}`, and the fallback is
    "no deviceId constraint", never "default" or "first", because Firefox and Safari have no
    "default" id and ids are empty before permission.
  - `canChooseSpeaker()`.
  - The effectful `CallDevices`: enumerate on `devicechange`, open, a track `ended` opens the
    preference-or-unconstrained device and says which.
  - `simulatedCallDevices()` (two microphones, two speakers, one camera, no `getUserMedia`),
    selected only when `backendIsMock`.
- `web/src/lib/call-health.ts` [M3]:
  - `callHealth(prev, next, {muted, phase})` over digested stats → `{level: good|poor|bad|reconnecting|unknown, rtt_ms, loss_pct, sending, report_from_far_side, path}`.
  - Thresholds, teams-lite's own: good below 300 ms RTT and 3 % loss; bad above 600 ms or 10 %, or
    no outbound packet increase over two samples.
  - INBOUND silence is never a fault: an empty meeting receives 0 B (NATIVE-CALLING.md:540-546).
  - The `SpeakingWhileMuted` window.
- `web/src/lib/call-reconnect.ts` [M6], a pure machine:
  - Inputs: transport state, online/offline, visibility, answer applied, refused, timed out, local
    fatal, clock.
  - Outputs: show reconnecting, attempt, give up.
  - Constants, with their client sources: `RECONNECT_WINDOW_MS = 90_000`,
    `DISCONNECTED_AS_FAILED_MS = 30_000`, `MAX_RECONNECT_DELAY_S = 5`, `WAKEUP_LIMIT_MS = 180_000`,
    and `MAX_CONSECUTIVE_LOCAL_FATAL = 2`.
  - "Local fatal" is DEFINED here as the browser throwing while building or applying the restart
    offer or its answer (`createOffer`, `setLocalDescription`, `setRemoteDescription`). That is the
    client's class of the same name: `if(e.type===B.RENEGOTIATION_ERROR.localFatal){…const n=++this.localFatalAttemptCounter,s=this.settings.maxConsecutiveLocalFatalReconnects,a=n<=s;…}`
    (`@2307978`). A refusal or a timeout is not local fatal; it backs off within the window.
  - A call that never connected is never reconnected (`initialReconnectTimeLimit:0`).
- `web/src/lib/call-experiments.ts` [M0], one module of page switches, all default OFF:
  - `teams-lite:call-exp:reconnect` [M6]
  - `reconnect-anytime` [M6, drivers only: the button shows on a healthy call]
  - `rejoin` [M7b]
  - `pagehide-hangup` [M7]
  - `announce-muted` [M5]
  - `relay-only` [M9]
  
  Drivers set them in their own profile with `addInitScript`. Flipping a default is a code change
  after a measurement.

**Web: components and other lib**
- `web/src/lib/call-stage.ts` [M1]: `stageControlLayout(width, {canSendMedia, canShareScreen,
  hasChat, canRecord, coarse})` → `{header, bar | null, overflow}`. Invariant: `hangup` is in
  `header` and the header's summed width fits. Widths are constants mirrored from the component.
  Mini-window coverage too.
- `web/src/components/call-stage.tsx` [M1, M3, M4, M6, M7]:
  - Below `PANEL_BESIDE_PX` (640) the header holds the mark, the title over the subtitle, the health
    bars, Fold and an icon-only Leave.
  - Mute, Camera, Share, People and Chat go to a bottom bar (`call-stage-controls`).
  - Record, Devices and Call health go behind More.
  - Header `pt-[env(safe-area-inset-top)]`, bar `pb-[env(safe-area-inset-bottom)]`.
  - On a coarse pointer the stage controls are 44 px.
  - In the mini bar the ink stays 32 px and the target grows by a pseudo-element
    (`after:-inset-1.5`, the dialog-close technique), with the gap raised to 12 px so targets never
    overlap. `MINI_BAR_HEIGHT` stays 44.
  - `data-testid="call-hangup"` stays on the always-visible Leave at EVERY width.
  - The stage root gains `data-meeting-thread` / `data-join-url` (for re-proof mid-call),
    `data-health`, `data-microphone` and `data-media`.
- `web/src/components/call-bar.tsx` [M1]: Answer and Decline at 44 px on a coarse pointer.
- `web/src/components/call-prejoin.tsx` [M5], new:
  - A dialog on desktop and a full-height sheet below 640 px.
  - Join now carries `data-join-url` / `data-meeting-thread` from the pre-join state.
  - The speaker Test tone is an `OscillatorNode`, never a fetched asset.
- `web/src/components/meeting-join-button.tsx:78` and `conversation-menu.tsx:743` [M5]:
  `requestMeetingJoin` in place of `joinMeeting`. Their attributes (`meeting-join-button.tsx:73-74`,
  `conversation-menu.tsx:731-734`) stay. This covers the calendar, the incoming-call banner
  (`incoming-call-banner.tsx:138`) and Join with a link (`join-with-link-dialog.tsx:133`).
- `web/src/components/call-devices-menu.tsx` and `call-health-popover.tsx` [M3, M4], new.
- `web/src/components/call-settings.tsx` [M4], new, placed beside `AlwaysAvailableSettings` and
  `CallRecordingsSettings` (`settings-pane.tsx:128-129`).
- `web/src/components/app.tsx:600, :612` [M2]: while this page holds a live call's media, each
  reload button carries one more line: "This ends the call's audio on this window."
- `web/src/lib/call.ts` [M2, M7]:
  - `ActiveCall` gains `media_owner`, `media`, `can_reconnect_media` and `can_rejoin_media`.
  - `CallPreparation` gains `layout` and `muted`.
  - `callEndLabel` adds `CallEndReasonMediaLost` and rewords `CallEndReasonReconnected`.
  - New `callMediaHere(call, pageId)`.
- `web/src/lib/notice.ts` [M6]: an optional `action: {label, onClick}` (sonner's own `action`),
  used once.
- `web/src/lib/ws-client.ts`:
  - [M2] `callHangup(callId, reason?)`; a give-up suspension while holding media.
  - [M5] `callPrepare` takes `muted` and `page`; `callJoin` takes `announceMuted`.
  - [M6] `callOfferMedia(…, {reconnect})`.
  - [M7] `callMediaState` on `writeRequest` (it is gated).

**Mock: `web/mock/server.ts`.** It mirrors every refusal and adds hooks, all reset by
`call_invite {reset: true}`:
- **Refusals.**
  - [M2] `call_hangup` with a bad `reason`, or with any reason on a ringing incoming call.
  - [M7b] `call_prepare {rejoin}` with no connected call, or with an unreplayable layout.
  - [M6] `call_offer_media {reconnect}` when not connected.
  - [M7] `call_media_state` for an unknown call, and a non-owner `holding: true` answered
    `{accepted: false}`.
- [M5] `call_prepare` echoes `muted`.
- **Hooks.**
  - [M6] `{kind:"call_transport", state:"disconnected"|"failed"|"connected"}`, a mock-only event the
    simulated media turns into `onConnectionStateChange`.
  - [M6] `{kind:"call_reconnect", refuse:"post"|"frame"}`: the next reconnect offer is refused by a
    failed POST, or by a `/call/rejection/` frame.
  - [M7] `{kind:"call_owner", lost:true}`.
  - [M4] `{kind:"call_devices", unplug:<id>}` / `{plug:<id>}`.
  - `{kind:"call_log"}` returns the call RPCs received, to assert that the pre-join posts nothing and
    that a device switch is not a renegotiation.

**Drivers**
- `web/scripts/join-live.ts`:
  - [M5] The proof is ALSO re-read from the pre-join's Join now, immediately before that click.
  - `hangUp` THROWS when it finds no Leave.
  - A new `assertNoLiveCall` opens a fresh page, re-proves the stage's `data-meeting-thread`, presses
    Leave, and fails loudly if the stage is still there. It runs after `--rejoin` and `--close-tab`.
  - Modes, all on the same pinned meeting, each setting only the flags it measures, in its own
    profile, with Join muted stated explicitly:
    - `--muted`, and `--announce` with it [M5]
    - `--reconnect` [M6]: sets `reconnect` + `reconnect-anytime`, re-proves `data-meeting-thread` /
      `data-call-id`, presses Reconnect audio, and prints `local ufrag changed: yes/no`, never the
      value.
    - `--rejoin` [M7b]: CDP `Page.crash`, no `pagehide`.
    - `--close-tab` [M7]: `page.close()` with `pagehide-hangup` on.
    - `--hold <s>` with `data-health` beside its own stats [M3].
    - Gather duration [M0].
- `web/scripts/call-live.ts` [M6]: `--reconnect` against the authorized one-to-one, same rails. It
  runs only after the meeting run passed, and only once the person has been told the call may drop.
- `web/scripts/preview.ts`: extend `--call` (§ 8).

**Guard: `.claude/hooks/guard-live-automation.sh` and its `.test.py` [M7]**
- Add `call_media_state` to BOTH alternations of the script scan (`:981`), with block and allow
  cases. The existing `every_gated_method_is_named_in_the_automation_guard` test
  (`server.rs:17954`) pins it.
- [M9, only if decision D6 picks the agent-observed capture] add `teams.microsoft.com` and
  `teams.cloud.microsoft` to `guard-prod-chat-target.sh`'s live set (`:76`) first, so only the
  observing tools work there.

## 6. Gating and consent

### 6.1 Methods

| Method | Gate | Change | Why |
|---|---|---|---|
| `call_media_state` (new) | `MACHINE_METHODS` | new; `machine_effect` arm; both guard alternations | It posts nothing to Teams, but it changes what every page is told about the call and whether a rejoin is offered. |
| `call_hangup` | `OUTWARD_METHODS` (unchanged) | `reason ∈ {"media_lost"}` | The closed set means a client can state exactly one end reason, and only for a call that had media. The body to Teams is unchanged. |
| `call_offer_media` | `OUTWARD_METHODS` (unchanged) | `reconnect` | A reconnect offer is the same act as any offer: it says what this machine sends. |
| `call_prepare` | `MACHINE_METHODS` (unchanged) | `page`, `muted`, `rejoin` | `rejoin` moves `media_owner` and hands out the relay credentials, which is the reason it is gated today. |
| `call_join` | `OUTWARD_METHODS` (unchanged) | `announce_muted` | It narrows what the join publishes. |

A read-only backend has no calling registration (`calling_available`, `server.rs:14415`), so relink,
ownership and every internal hangup never run there, and the new params are refused at dispatch.
The mock mirrors each refusal.

### 6.2 Switches: every protocol change and every new automatic act is OFF by default

| Switch | Where | OFF (default) | ON |
|---|---|---|---|
| `teams-lite:call-exp:reconnect` | page | `failed` hangs up at once, now with "The call's connection was lost." | renegotiate, 90 s window, then hang up |
| `teams-lite:call-exp:reconnect-anytime` | page, drivers only | Reconnect audio shown only while health is bad or reconnecting | shown on a healthy call |
| `teams-lite:call-exp:pagehide-hangup` | page | `pagehide` sends `holding: false` only | `persisted === false` sends `call_hangup` |
| `teams-lite:call-exp:rejoin` | page | owner-lost offers Hang up / Leave and rejoin | adds Use audio here where `can_rejoin_media` |
| `teams-lite:call-exp:announce-muted` | page | a muted join is local only | adds `endpointState.state.isMuted` |
| `teams-lite:call-exp:relay-only` | page, drivers only | ICE policy `all` | `iceTransportPolicy: "relay"` |
| `TEAMS_LITE_CALL_SURL_MOVED` | backend env | `end-locally` (today) | `hang-up` or `relink` (§ 4.6, § 4.4, § 4.5) |
| `TEAMS_LITE_TROUTER_RESUME=1` | backend env | nominal allocate | resume allocation, calling socket only |
| `TEAMS_LITE_CALL_TURN=1` | backend env | STUN only to the page | TURN + the relay gather rule |

Backend env switches are set by the user on their own unit. They are never set by tooling: starting
or restarting a send-capable backend is theirs (§ Automation safety).

### 6.3 The rule, as it really is, and the amendment proposed (decision D1)

CLAUDE.md § Audio calls says "each one carries out one click the user just made — nothing in this
feature ever acts on its own", and NATIVE-CALLING.md § 7 says a call is never placed and never
answered by anything automatic. The second statement is true. The first is already not literally
true. These automatic outward acts ship today:

- a hangup when the transport fails (`store.ts:2842-2846`);
- a hangup when THE answer cannot be applied (`store.ts:2672-2684`);
- a hangup or decline when answering fails (`store.ts:3110-3116`, `plane-reliability`'s
  `answer-failure-declines`);
- answering the service's own renegotiation offers and subscribing to what they carry
  (`answerRemoteOffer`, `store.ts:2720`; `call_answer_media`, `call_subscribe`);
- the take-back offer after an unreadable answer (`store.ts:2697-2706`);
- the glare refusal POST (`server.rs:14836-14859`);
- `hang_up_orphan` (`server.rs:15120`, the consequence of the user's own hangup);
- the keep-alive ticker.

The proposed wording: "Nothing in this feature STARTS a call, rings anybody, answers, unmutes, or
publishes new media on its own. It may maintain or end a call the user is already in, in the cases
listed here, and each is behind a switch until it has been measured." The NEW acts this design adds
to that list:

1. **Relink**: re-point a live call's callbacks after the calling socket moved (§ 4.4, § 4.5).
2. **Reconnect**: renegotiate the existing audio after the transport failed, with the same mute
   state and the same `sending` (§ 4.2). It REPLACES today's immediate automatic hangup.
3. **Give up**: hang up a call that cannot be kept, either because the reconnect window is exhausted
   (replacing today's immediate hangup) or because the socket moved and the relink was impossible or
   failed (replacing today's silent local end). It never declines a ringing incoming call. A dialing
   outgoing call is cancelled, and that is stated in the journal.
4. **Close**: hang up when the window carrying the call's audio is closed (`pagehide`,
   `persisted === false`).

   What this does NOT have: a prompt that confirms it on the platform this app is read from. WebKit
   does not fire `beforeunload` on iOS, Chrome on Android does not prompt on a tab close, and the
   client's own speedbump is off by default (`@3084136`). The `beforeunload` prompt teams-lite arms
   (M2) is a desktop nicety and not a consent argument.

None of the four rings anyone, unmutes, opens a camera or a screen, or joins anything. The user
decides whether each may become a default (D1). Until then each one ships OFF.

## 7. UI and copy

All copy is final English and every string is exact.

**Stage on a phone (below 640 px).**
- The header holds the call mark, the title over the subtitle, the health bars, Fold, and Leave as
  an icon only: 44 px, red, always visible, `data-testid="call-hangup"`.
- The bottom bar (`call-stage-controls`) holds Mute, Camera (only with `can_send_media`), Share (the
  `canShareScreen` flag from `video-send`), People, Chat and More.
- More holds "Record this call" (with `RECORD_HINT` as its title), "Devices" and "Call health".
- Safe areas apply top and bottom, and every target is 44 px on a coarse pointer.
- The health bars use hugeicons glyphs; check the exact names in the installed version before use.
- Desktop keeps today's header, adds Devices and the bars, and leaves `StageClock` unchanged.

**Subtitle states.**
- "Reconnecting…": the transport is disconnected, or a reconnect is in flight.
- "Audio is playing on another window.": the media is elsewhere.

**Ownership banner in the stage.**
- When `media: "released"`: "The window carrying this call's audio was closed."
- When `media: "lost"`: "The window carrying this call's audio stopped answering."
- Buttons: "Use audio here" (only with `can_rejoin_media` and the rejoin flag), and "Hang up" for a
  one-to-one or "Leave and rejoin" plus "Leave" for a meeting.

**Mic and playback lines** (one quiet line under the subtitle, never a notice).
- "Your device paused the microphone." (the track's `mute` event)
- "Your microphone was disconnected. Switched to <label>." (`ended`, and the fallback worked)
- "Your microphone was disconnected. Choose another in Devices." (no fallback; the Mute button shows
  muted)
- A blocked playback draws a "Tap to hear the call" button over the stage.

**Speaking while muted.** A small pill reading "You're muted." with an "Unmute" button. At most once
per 30 s, dismissable, never a notice, and never on iOS until measured.

**Health popover.**
- Heading: "Call health".
- Rows:
  - "Network: Good" | "Network: Poor" | "Network: Bad"
  - "Round trip: 84 ms"
  - "Packet loss: 2.1 %"
  - "Your audio: reaching the call" | "Your audio: no report from the call yet"
  - "Path: direct" | "Path: relay over UDP" | "Path: relay over TCP"
  - "Microphone: <label>"
- "Reconnect audio" appears only with `can_reconnect_media`, the reconnect flag, and health bad or
  reconnecting (or the drivers-only flag).
- Caption: "Measured on this device. Nothing here is sent anywhere."

**Devices menu.**
- Sections "Microphone", "Speaker" and "Camera" (Camera only while sending). Each lists device labels
  with "System default" first.
- Speaker is NOT drawn where `setSinkId` is unsupported (iOS Safari), because a control that changes
  nothing reads as a bug.
- Firefox shows "Choose a speaker…" (`selectAudioOutput`) until outputs are listed.

**Pre-join** (meetings only; placing a one-to-one and answering a ring do not get one). It is reached
from the calendar, the conversation menu, the incoming-meeting banner and Join with a link.
- Title: "Join <meeting>".
- "Microphone" select with a live level meter.
- "Speaker" select with "Test", a short local oscillator tone through the chosen output.
- A "Join muted" switch.
- Note: "Your microphone is on for this check only. Nothing is sent until you join."
- Buttons: "Join now" (primary) and "Cancel".
- After 120 s idle the microphone is released and the meter says "Microphone paused — tap to check
  again."
- On a phone it is a full-height sheet with the buttons in the bottom safe area.
- Errors reuse `callFailureMessage`, the insecure-origin sentence included.

**Endings.**
- `CallEndReasonMediaLost`: "The call's connection was lost."
- `CallEndReasonReconnected`: "The connection to Teams moved and the call could not follow it."
- For a MEETING, either notice carries a "Rejoin" action. It opens the pre-join pre-filled with the
  mute state the call had, which keeps the join proof in one place (decision D5).

**Settings › Calls.**
- "Microphone", "Speaker" and "Camera" (each "System default" by default).
- "Join meetings muted" (a switch; its default is decision D3).
- "Noise suppression": "Standard", "Voice isolation" (only where supported) or "Off".
- Footnote: "These choices are kept in this browser."

**Reload buttons while holding media.** The extra line "This ends the call's audio on this window."
(`app.tsx:600`, `:612`).

**Close tab (desktop).** The browser's own generic leave prompt, while this page carries the audio of
a live call, never while ringing. The wake lock is invisible.

## 8. Tests, mock and captures

**Rust (`cargo test`).**
- Every new body shape:
  - `conversation_links_update_payload`: `participants.from`, the Delta roster on the NEW surl,
    exactly the nine conversation links, the same session and cause.
  - `call_participant_update_payload` with and without links.
  - `call_callback_links` equals the union of the builders.
  - `CallbackBase::with_surl`.
  - `Links::call_leg`.
  - a relink is one-shot: it posts once to `call_leg()`'s target and once to the conversation's
    `notificationLinks`, and arms no timer. (It asserts that behaviour, NOT the contents of
    `Links::keep_alive`, which incoming-and-conversation M6 removes; README § 3, collision 4.)
  - The pinned `notificationLinks` reader refuses a content-sharing session's (a fixture with both).
  - `surl_digest` never contains the allocation id.
  - `media_layout` over captured SDP fixtures, with gapped mids and rejected `x-data` flagged
    unreplayable.
  - `join_payload` with `announce_muted = false` is byte-for-byte unchanged.
  - `media_negotiation_failure_from_frame`.
  - `trouter::allocate_request_url`: cached, expired, 4xx.
- Source scans:
  - `a_moved_surl_never_declines_a_ringing_call` (the moved-socket path cannot reach `reject` or
    `rejection_payload`).
  - `a_moved_surl_hangs_up_or_relinks_only_when_switched` (the default still reaches
    `end_call_locally` alone).
  - `the_keep_alive_ticker_skips_nothing_new`: after a relink, the ticker posts nothing it did not
    post before (whichever ticker holds then: the 20 s one today, `CALL_TICK` after incoming M6).
- `call_hangup` refuses any reason but `media_lost`, and refuses a reason on a ringing incoming call.
- `call_media_state` is in `MACHINE_METHODS`, has a `machine_effect` arm and is refused read-only.
- A non-owner claim is refused, and only `call_prepare` moves the owner.
- `PageLease` drop marks the owner absent.

**Web unit (`bun run test`).**
- `stageControlLayout`: across 280–1440 px and every flag combination, `hangup` is in the header and
  the header fits; mini-window targets do not overlap.
- `callHealth` over `getStats` fixtures digested from `readMediaStats`' shapes
  (`join-live.ts:859-910`), including "an empty meeting receives 0 B and that is not a fault".
- The `SpeakingWhileMuted` window.
- `call-reconnect`:
  - `disconnected` alone does not attempt;
  - `failed` attempts;
  - backoff runs 1…5 s;
  - the window is 90 s;
  - two local fatals give up;
  - refused and timed-out back off;
  - never-connected never reconnects;
  - waking after 180 s gives up.
- `micConstraints` with no preference equals `openMicrophone`'s constraints exactly.
- `callEndLabel` for the new reasons, and `callMediaHere`.
- `CallAudio.microphone` is the sent track, never the analyser's clone.

**No-tenant browser spec `web/e2e/webrtc-reconnect.spec.ts`** (the `webrtc-glare.spec.ts` pattern:
`about:blank`, two peer connections, canvas and oscillator tracks).
- (a) `replaceTrack` mid-call fires no `negotiationneeded`, and the far side's inbound packets keep
  rising.
- (b) A hot-swap while muted leaves no audio level at the far side; unmute, and audio flows.
- (c) `restartIce` → `createOffer` → `toMsSdp` / `fromMsSdp` yields a new ufrag and pwd with the same
  mids and labels, and audio resumes after the far side's answer.
- (d) A fresh peer connection replaying `0` audio reproduces it. Replaying the measured `0, 3, 4`
  layout is REFUSED before anything is posted.

**Mock + E2E.**
- `calling.spec.ts`: the header at 360, 390, 412 and 1280 (Leave's box inside the viewport, the bar
  present on phones, 44 px targets, `call-hangup` unique per surface).
- `call-devices.spec.ts`: picking a microphone changes the stage's `data-microphone` and puts no
  renegotiation in `call_log`; the unplug hook draws "Switched to…"; Speaker is hidden when
  `setSinkId` is stubbed away.
- `call-reconnect.spec.ts` (`page.clock`):
  - `disconnected` draws "Reconnecting…";
  - with the flag, `failed` puts `call_offer_media {reconnect: true}` in `call_log`, then connected;
  - both refuse hooks, after 90 s, give `call_hangup {reason: "media_lost"}`, the notice and Rejoin,
    and Rejoin opens the pre-join;
  - with the flag OFF, `failed` hangs up at once with the reason (the regression guard, first written
    against today's code in M0).
- `call-prejoin.spec.ts`:
  - Join here opens the pre-join, and `call_log` stays EMPTY until Join now;
  - Join muted draws the stage muted from the first frame, and `call_prepare` carried
    `muted: true`;
  - Cancel, Escape, a route change and hiding the page each stop the simulated microphone.
- `call-owner.spec.ts`:
  - the owner-lost hook draws the banner;
  - two pages: the non-owner says "Audio is playing on another window.";
  - an old owner reconnecting after a rejoin is refused, and stops its media.
- `call-teardown.spec.ts`:
  - a vanished call (a successful `call_status` with no call) stops the simulated media and a
    running recording;
  - a REJECTED `call_status` during a live call leaves the media running.
- `mobile.spec.ts`:
  - a stubbed `navigator.wakeLock` is requested on connected and released on end;
  - a synthetic `mute` on the simulated microphone draws the line;
  - `beforeunload` is armed only while holding media.

**Specs that change because of the pre-join** (updated in M5's PR, through a new
`joinThroughPrejoin` helper in `web/e2e/helpers.ts`): `calling.spec.ts` (14 presses of
`meeting-join-here`), `calendar.spec.ts`, `incoming-call.spec.ts`, `mobile.spec.ts`, and the `--call`
flow in `preview.ts`.

**Captures** (`cd web && bun run preview -- --out /tmp/call --call`, `--dpr 3` for the bars).
- The phone header and bottom bar at 360, 390 and 412 in both themes.
- More, Devices, the health popover (poor), Reconnecting, both ownership banners.
- The pre-join in both themes and at a phone's width.
- Settings › Calls in both themes.

## 9. Live verification

Every step uses a sanctioned driver or the user's own click. Measurements land in NATIVE-CALLING.md
as they are taken.

| # | Step | Driver | Who acts | Success signal |
|---|---|---|---|---|
| 1 | Does the calling surl move on reconnect? | none: M0's journal line on the always-on service over several days | the user reads `journalctl --user -u teams-lite-backend` | a count of `surl unchanged` vs `surl moved`; if it never moves, M8 shrinks to the hangup case |
| 2 | Does the join answer carry `notificationLinks`? | `cd web && bun run join-live` (unchanged baseline) | agent | M0's journal line names it present or absent |
| 3 | Gather duration today | `cd web && bun run join-live` with M0's gather timing | agent | the milliseconds to `complete` and the candidate types, as shapes |
| 4 | Where `relayConfig` comes from | `cargo run --example relay_config_recon` (read-only) | agent | a key path, or nothing; then D6 |
| 5 | Leave is never clipped | preview captures, then the user's phone in a real call | user (looks) | Leave tappable with camera, share and Record drawn |
| 6 | Join muted, local | `cd web && bun run join-live -- --muted` | agent | the pre-join appears; the proof is re-read on Join now; `call.muted` is true from the first frame; outbound packets flow with no level; hang-up on exit |
| 7 | Join muted, announced | `bun run join-live -- --muted --announce` and `bun run join-live -- --pair` | agent; the user starts both installs (`systemctl --user enable --now teams-lite.target` / `teams-lite-app.service`) | the receiver's roster digest shows our endpoint `isMuted: true`, and the join is not refused; then flip the flag's default |
| 8 | Health matches reality | `bun run join-live -- --hold 60` | agent | `data-health` is good while the driver's stats show outbound packets rising and a selected pair; no false "bad" for 0 B inbound |
| 9 | ICE restart in a meeting | `bun run join-live -- --reconnect` | agent | journal `[calling] reconnecting media`, then an answer with `audio mid=0 … accepted`; `local ufrag changed: yes`; packets rising for 60 s; phase stays connected. A refusal is § 4.1's line and the call is NOT dropped by the run |
| 10 | ICE restart in a one-to-one | `bun run call-live -- --reconnect --hold 120`, only after #9 passed | agent drives; the authorized person was told beforehand the call may drop, and confirms they still hear the call | the same journal lines; no drop; then flip the reconnect default (after D1) |
| 11 | A real network change | `bun run join-live -- --hold 300` with the reconnect flag | the user toggles Wi-Fi on the machine (their action) | Reconnecting… then connected within 90 s; the journal says `surl unchanged`, or (with the user's `TEAMS_LITE_CALL_SURL_MOVED=relink`) a relink |
| 12 | Owner lost, then rejoin | `bun run join-live -- --rejoin`, in an EMPTY meeting state, so the layout is replayable | agent | the second page shows the banner within about 45 s; Use audio here; the reconnect answer is accepted; the second page's stats flow; `assertNoLiveCall` ends clean |
| 13 | Closing the window ends the call | `bun run join-live -- --close-tab` | agent | `the call is over: CallEndReasonHangup` and the service's `/call/end` within seconds of `page.close()` (desktop Chromium only); `assertNoLiveCall` ends clean |
| 14 | iOS background and lock | the user's OWN iPhone, placing, answering or joining a real call | user | recorded: whether audio continues while backgrounded or locked, whether the socket drops, whether `pagehide` fires on app switch or lock, whether the wake lock holds; this decides D2 and D4 |
| 15 | Resume allocation | the user sets `TEAMS_LITE_TROUTER_RESUME=1` on their own unit | user | over days, `surl moved` falls to about 0 |
| 16 | Relay | after #4, `bun run join-live -- --hold 60` with the `relay-only` flag and the user's `TEAMS_LITE_CALL_TURN=1` | agent + user | the selected pair is `relay/tcp` or `relay/tls`; setup time recorded |
| 17 | Device hot-swap | the user's own click in a real call | user | switching mic and speaker, and unplugging a headset, keep the call audible; no renegotiation line in the journal |

## 10. Milestones

Each milestone ships alone. Effort is S (under a day), M (a few days) or L (a week or more).

1. **M0: Measure before changing.**
   - Scope:
     - wire the simulated media's transport callback;
     - a flag-off regression spec pinning today's `failed` hangup;
     - the journal lines (surl digest, `notificationLinks` present, first `relayConfig`);
     - gather timing in join-live;
     - `relay_config_recon.rs`;
     - `call-experiments.ts`.
   - Effort S. Risk: none; nothing reaches anybody.
   - Dependencies: none.
2. **M1: Hang up is always reachable on a phone.**
   - Scope: `stageControlLayout`, the phone header, bar and More, safe areas, 44 px coarse targets
     (stage, mini, ringing card), and specs at 360, 390, 412 and 1280.
   - Effort S. Risk: layout only.
   - Dependencies: `video-send` (`share-button-without-capture-support`) supplies the share flag (soft).
3. **M2: Teardown, a failure that says so, phone hygiene.**
   - Scope: the success-only `refreshCallStatus` fold, `call_hangup {reason}`, explicit `play()`,
     mic-track lines, the wake lock, `beforeunload`, reload-button copy, the socket kept trying while
     holding media.
   - Effort M. Risk: low; the hangup body is unchanged.
   - Dependencies: `plane-reliability` (`backend-restart-mid-call` is the backend half of a vanished
     call; soft).
4. **M3: Call health, Reconnecting, speaking while muted.**
   - Effort M. Risk: none outward; CPU bounded by 5 s and pausing when hidden; off on iOS.
   - Dependencies: none.
5. **M4: Devices and Settings › Calls.**
   - Scope: the current-track `CallMedia`, `micConstraints` (byte-for-byte without a preference),
     `replaceTrack`, `setSinkId`, `devicechange`, noise suppression, simulated devices.
   - Effort M. Risk: low protocol risk; the default path is unchanged.
   - Dependencies: `incoming-and-conversation` (`mute-enforced-on-media-page` touches `setMuted`;
     coordinate on the current-track mute).
6. **M5: Pre-join for meetings, and Join muted.**
   - Scope: `call-prejoin.tsx`, `requestMeetingJoin`, the muted join (local), the announce flag,
     join-live proving on Join now, `--muted`, and the spec updates.
   - Effort M. Risk: medium; the sanctioned join flow changes and is updated in the same PR.
   - Dependencies: `incoming-and-conversation` (`self-mute-not-visible-in-meetings`, for unmuting
     visibly in a meeting).
7. **M6: Reconnect instead of hanging up** (flag).
   - Scope: § 4.1's handler, offer purpose, `call-reconnect.ts`, `restartIce`, the wakeup rule,
     Reconnect audio, the Rejoin action, `webrtc-reconnect.spec.ts`, `join-live --reconnect`, then
     `call-live --reconnect`.
   - Effort L. Risk: high; the ICE restart is unmeasured, but OFF is today's behaviour.
   - Dependencies: M0 (relay recon); `plane-reliability` (`signaling-post-timeouts`, so a reconnect
     POST is bounded); `video-send` (shares the offer purpose and the refusal handler).
8. **M7: Who carries the audio.**
   - Scope: `pageId`, `call_media_state` (MACHINE + guard), `PageLease`, `MEDIA_OWNER_GRACE`, `media`
     in `call_state`, the precedence rule, `pagehide` (`holding: false`; the hangup behind its flag),
     the ownership banner, `--close-tab`, `assertNoLiveCall`.
   - Effort L. Risk: medium; no automatic orphan hangup, and the `pagehide` hangup is behind a flag.
   - This is the ONE page-ownership mechanism (README § 3, collision 2): the `page` param on
     `call_prepare`, `call_accept` and `call_media_state`, minted with `getRandomValues`, and the
     `media_owner` / `media` state.
   - Dependencies: `incoming-and-conversation` (`multi-page-call-ownership`, its M8, consumes the
     primitives and adds no param of its own; `mute-enforced-on-media-page` uses `media_owner`);
     `plane-reliability` (`backend-restart-mid-call`).
9. **M7b: Use audio here for replayable layouts** (flag).
   - Scope: `media_layout` recording, `call_prepare {rejoin}`, the layout-replay peer connection with
     its mid check, `--rejoin`.
   - Effort M. Risk: high and narrow; the new session is unmeasured, and only audio-only `0…n-1`
     layouts qualify.
   - Dependencies: M7.
10. **M8: The calling socket moved.**
    - Scope: the `TEAMS_LITE_CALL_SURL_MOVED` switch (`hang-up`, then `relink`), the
      phase-correct hangup, the pinned `notificationLinks`, both relink bodies, the roster resync, the
      share ended, and the resume allocation behind its own switch.
    - Effort L. Risk: high; the bodies are read, not measured, and OFF is today's behaviour.
    - Dependencies: M0's surl measurement decides whether it is worth building;
      `incoming-and-conversation` (`meeting-keepalive-long-meetings` shares the pinned field and the
      conversation-links builder; `call-keepalive-on-callleg` owns the keep-alive accessor);
      `plane-reliability` (`registration-ttl-extension`, re-registration on reconnect).
11. **M9: Our own TURN relay, and a gather rule that waits for it** (switch).
    - Scope: read `relayConfig` from wherever M0 names, the relay-first gather rule, the relay-only
      flag.
    - Effort M. Risk: medium; no endpoint is guessed, and OFF is STUN-only, as today.
    - Dependencies: M0's recon result (and D6 if it finds nothing).

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| An ICE-restart offer is refused and a WORKING call drops (the service has ended calls a second after a body it disliked, NATIVE-CALLING.md:1647-1666) | It is automatic only on `failed`, where the alternative is today's hangup. The manual button shows only when health is bad (drivers excepted). It is measured in the pinned meeting first, and on the one-to-one only after that, with the person warned. |
| A refused reconnect wastes the window | § 4.1 reports it at once, instead of after 35 s. The machine backs off within 90 s. |
| A reconnect offer meets a service renegotiation (glare) | The existing backend refusal (`server.rs:14822-14859`) and the page backstop. The offer is queued on `negotiating`. |
| `refreshCallStatus` tears down a live call on a transient error | Only a SUCCESSFUL answer is folded; a test pins that a rejection leaves media running. |
| Device code changes the verified mic open | With no stored preference, the constraints are byte-for-byte today's (pinned); `setSinkId` is never called without a chosen speaker; join-live and call-live run once before M4 merges. |
| Hot-swap while muted leaks audio | `enabled = !muted` is set before `replaceTrack`; `setMuted` reads the current track; the no-tenant spec (b). |
| The pre-join leaves the mic open | Release on Cancel, unmount, conversation change, hidden and 120 s idle. A handoff only through `startCallMedia`, whose catch and `adoptCallMedia`'s refusal release it. A spec per exit. |
| An old owner takes the call back after a rejoin | Only `call_prepare` moves the owner; a non-owner claim is refused and that page stops. |
| "Owner lost" fires for audible audio | It is a banner only; the owner keeps its socket trying; 45 s grace; the wording says "stopped answering", not "closed"; no automatic hangup (D2). |
| `pagehide` hangs up on a reload or a tab switch | Behind a flag, `persisted === false` only, measured on desktop (#13) and iOS (#14). The in-app reload buttons say what they cost. |
| A rejoin replays the wrong layout | Only `0…n-1` audio or video layouts with nothing rejected qualify, and the page refuses a mismatch before posting. |
| Relink POSTs to a sharing session's `notificationLinks` | A pinned field read from a conversation response, never the flattened `Links`; a test with both present. |
| Relink silently starts a keep-alive on `callLeg` | This design does not extend `Links::keep_alive()`; relink has its own `call_leg()`; a test pins the BEHAVIOUR (a relink is one-shot and arms no timer; the ticker posts nothing new after it), so it survives incoming M6 removing the accessor. |
| An automatic hangup declines a ringing incoming call | `hang_up_live_call` ends a ringing incoming call locally only; a source scan pins it. |
| Trouter resume breaks messaging | It applies to the calling endpoint only, behind an env switch, with fallback to the nominal allocate. |
| TURN slows every call setup | Behind a switch; resolves on the first relay candidate; 20 s only as a ceiling; measured in #16. |
| Drivers leave an orphan in the pinned meeting | `hangUp` throws when it finds no Leave; `assertNoLiveCall` runs after the crash modes; `call-hangup` stays on Leave at every width. |
| The analyser reroutes iOS audio or leaks into a recording | Off on iOS; the clone is private; `CallAudio.microphone` is pinned to the sent track. |
| An agent-driven capture of the real client records credentials | Not run unless D6 chooses it, and then observe-only, with the browser guard widened first and values redacted. |

## 12. Open questions and decisions for the user

**Decisions the user must make.**
- **D1. The four automatic acts and the reworded rule (§ 6.3).** Approve each one (relink, reconnect,
  give up, close) to become a default after its measurement, or keep any of them off for good. The
  existing automatic acts are recorded either way.
- **D2. An orphaned call.** The owner page is gone without `pagehide` and nobody reclaims it, while
  it holds the one call slot and every later incoming call is refused as busy. Choose:
  - (a) nothing, as today;
  - (b) stop keep-alives for it, which is passive and lets the service end the leg on its own clock;
  - (c) hang up after 180 s, the client's wakeup limit.
  
  Recommended: (b) after measurement #14 shows iOS audio does not survive a suspended socket, and (c)
  only after (b) is observed.
- **D3.** The default for "Join meetings muted". Recommended ON: the pre-join shows the switch, and an
  unexpected live microphone is the hazard this design removes. Drivers state it explicitly either
  way.
- **D4. A reload of the call's window.** End the call (`pagehide-hangup` ON), or leave it to the
  owner-lost banner and rejoin where the layout allows (OFF, the default)?
- **D5.** Rejoin after a dropped meeting: through the pre-join (proposed: one more tap, and one place
  that proves the target), or one tap straight in with the last mute state?
- **D6. If the read-only recon finds no `relayConfig`.** Recommended: the user opens
  teams.microsoft.com in their own browser, searches the network responses for `relayConfig`, and
  reports the request's PATH only. The alternative is an agent-observed capture with the browser
  guard widened first. An anonymous-guest capture (the § 2.5 precedent) may not receive the
  ACCOUNT's relay configuration at all.
- **D7.** Not changed here, and noted: CLAUDE.md § A call is a PAGE opens the chat panel by default
  on every screen, and on a phone that panel covers the whole call (map issue 50).

**Open questions that only a measurement answers.**
- Does this tenant's join or create-call answer carry the conversation's `notificationLinks`? (#2)
- Does the calling surl move across reconnects today, and does resume keep it? (#1, #15)
- Is an ICE-restart renegotiation accepted in a meeting, and in a one-to-one? (#9, #10)
- Is a new-session renegotiation of a replayable layout accepted? (#12)
- Does the service ever POST a `mediaNegotiationFailure` to our `/call/rejection/`? (any M6 run)
- Is a nine-link conversation update accepted? (M8 with relink on)
- Is the `pagehide` WebSocket send delivered on desktop, and does `pagehide` fire on iOS app switch
  or lock? (#13, #14)
- Where does the host get `accountConfiguration` and `relayConfigProvider`? (#4, D6)

## 13. Review notes

Two adversarial reviews: code and protocol accuracy, and project rules, safety and regressions.
Every claim was re-checked against the code at `c941850` and the 2026-11-05 bundle.

**Accepted and fixed:**
- **No handler for a refusal on our `/call/rejection/` callback; `outgoing_negotiation_at` is never
  cleared by one.** Confirmed: no reader in `server.rs`, three clear sites only. This led to § 4.1,
  offer purpose tagging, and a reconnect refusal or timeout that never goes to
  `abandonCallRenegotiation`. Also confirmed the client reads `body.mediaNegotiationFailure` on that
  path (`@1079938`).
- **`call_hangup` takes no reason, so the MediaLost notice could never appear.** Confirmed. Fixed by
  a closed one-value `reason`, refused on a ringing incoming call; every page reads the reason from
  the backend.
- **The gather rule misread the client.** Confirmed in detail: a meeting is host-candidate-only, and
  otherwise the client completes on the first relay candidate or end-of-candidates, with a 20 s
  ceiling; the 5 s figure is `forceStun`'s. Replaced by § 4.11's rule behind `TEAMS_LITE_CALL_TURN`,
  with the "≤2.5 s" risk text removed. TURN in the offer is now behind a switch too (review 2).
- **Rejoin cannot reproduce gapped mids, and `x-data` is almost always rejected.** Confirmed against
  NATIVE-CALLING.md:661-663 and :1214-1216. Rejoin is narrowed to `0…n-1` audio or video layouts,
  with a pre-post mid check, and split into M7b. "Reload" is a rejoin case only when the
  `pagehide` hangup is off, which is now decision D4.
- **`notificationLinks` collides with the content-sharing session's.** Confirmed (`@1036517` vs
  `@1345412`; `Links::collect` and `mine` flatten and merge every frame). Now a pinned field from a
  conversation response, with a test.
- **Extending `Links::keep_alive()` changes every verified call's ticker.** Confirmed. Relink has its
  own `Links::call_leg()`; the keep-alive stays `incoming-and-conversation`'s, with the client's
  `{callParticipantUpdate:{}}` body, behind their switch.
- **`pagehide` has no confirming prompt on iOS, and the client's speedbump is off by default.**
  Confirmed (`@3084136`; `@1993958` hangs up on `pagehide` with no prompt). The consent claim is
  dropped; the hangup is behind a flag and `persisted === false`; `pagehide` otherwise sends
  `holding: false`; the reload buttons say what they cost.
- **CallDevices owning the mic broke `adoptCallMedia`'s and the start error's release, and
  `setMuted` and the recorder still read the original stream.** Confirmed (`call-media.ts:375-381`,
  `:1087-1089`, `:1131-1135`). `CallMedia` keeps ownership and tracks the current track;
  `enabled = !muted` comes before `replaceTrack`; a no-tenant spec covers it.
- **Ownership precedence was undefined, and the socket gives up at 30 s.** Confirmed
  (`ws-client.ts:225`, `:349-358`). Only `call_prepare` moves the owner; non-owner claims are refused;
  the owner keeps retrying; the banner wording distinguishes released from lost.
- **The offer body wrongly listed `clientContentForMediaController`.** Confirmed (`calling.rs:1405-1431`).
  Corrected.
- **Speaking-while-muted was presented as the client's.** Confirmed: `Jt` is a mean, so the shipped
  detector is inactive. Now labelled as teams-lite's own rule.
- **`maxConsecutiveLocalFatalReconnects` has a specific input.** Confirmed (`@2307978`). "Local
  fatal" is now defined.
- **The call-leg relink body missed `mediaAcknowledgement`, and the cause id is a choice.** Confirmed
  (`@926851`, `@931704`). The body is now the 15-link union pinned against the builders, and the
  choice is stated.
- **The TURN source may be a provider or a non-skypetoken credential.** Confirmed (`@775719`,
  `@776549`, `@781812`, `calling-shell.js@812586`). The recon also reports `www-authenticate`.
- **The recon should come before the reconnect work.** Accepted: the recon and every measurement
  moved to M0. The client waits for relays on reconnect (`waitForRelayOnReconnect:!0`), and #9
  records whether relays were present.
- **There is no `play()` to check.** Confirmed. The fix is an explicit `play()` with its rejection
  handled.
- **44 px targets would overfill the mini bar.** Confirmed (`call-stage.tsx:674-700`). The ink stays
  32 px, the target grows by a pseudo-element, and the gap is 12 px.
- **`sending` on rejoin.** Accepted: it is reset to `[]`.
- **Citations: the glare refusal and the WebSocket loop exits.** Accepted: the glare refusal is
  `server.rs:14822-14859`, and page tracking is a `Drop` guard.
- **Review 2's blocker: four automatic acts need the user's decision, and two shipped unswitched.**
  Accepted. § 6.2 puts every one behind a switch default OFF, § 6.3 records the automatic acts
  already shipped and proposes the wording, D1 asks, and NATIVE-CALLING.md § 7 joins the docs to
  update.
- **A moved surl could decline a ringing incoming call.** Accepted: ringing incoming calls end
  locally only, dialing is an explicit cancel, and a source scan pins it.
- **Folding `refreshCallStatus` naively would tear down media on a transient rejection.** Confirmed
  (`store.ts:3025-3032`, `:1698`). Only success is folded.
- **Device defaults must not change the verified mic open.** Accepted: byte-for-byte constraints with
  no preference, an unconstrained fallback, and no `setSinkId` unless chosen.
- **The manual Reconnect on a healthy call can drop a working call.** Accepted: shown only when
  health is bad, with a drivers-only flag; the meeting run comes before the one-to-one, and the person
  is warned.
- **Drivers can leave an orphan.** Accepted: `hangUp` throws, `assertNoLiveCall` runs, and
  `call-hangup` stays on Leave at every width.
- **A browser capture of the real client has no rails.** Accepted: D6 prefers the user's own devtools
  search; an agent capture would be observe-only with the guard widened first; the guest-precedent
  caveat is stated.
- **`machine_effect` is the real function name** (`server.rs:610`). Corrected.
- **The mock path is not wired.** Confirmed (`store.ts:2836-2837`). Wired first in M0, with the
  regression spec.
- **Printing the ufrag.** Accepted: drivers print "changed: yes/no" only.
- **Pre-join mic release beyond Cancel, and a synthesized Test tone.** Accepted.
- **Specs broken by the pre-join, and join-live's default mute.** Accepted and listed (§ 8). Drivers
  state Join muted explicitly.
- **Docs gaps** (§ Recording a call, NATIVE-CALLING.md § 7, the iOS step being the user's own phone,
  Rejoin skipping the pre-join). Accepted.
- **Keeping an orphan alive with keep-alives.** Accepted as option (b) of D2.

**Rejected or corrected in the reviewers' own claims:**
- **"`CallSession.muted` means what we told the service, so a local muted join misstates it".**
  Partly rejected. The code already moves `muted` "whatever the service says" (`server.rs:8439-8446`),
  so the field already means the local state, and it is the DOC that is stale. The doc is corrected;
  the field is not split.
- **"The client's `onIceDisconnect` gives up with no relays".** Narrowed. That give-up is in the
  `else` branch of `if(this.wasConnected)`, so it applies only to a call that never connected
  (`@2306338`). The recon still moved earlier, for the relay-wait reason.
- **"Rejoin can only follow a crash, a kill or another device".** True only with the `pagehide`
  hangup ON. With it OFF (the default), a reload is exactly an owner-lost case, so it is kept as
  decision D4 rather than removed.

**Found in verification, beyond both reviews:**
- The client's periodic conversation keep-alive IS the `vG` links update to `notificationLinks`,
  every 2700 s (`@1401234`, `@982507`), so the relink's conversation body is routine for the client.
- The client's wakeup rule is PAGE-local (on becoming visible after the peer connection closed while
  hidden; `@2339435`). It is copied into the reconnect machine rather than into a backend timer.
- The header width was recomputed from the classes: about 429 px with camera, share, Record and chat,
  and about 387 px without Record (clipped at 360 px).
- The automatic sign-in reload already treats a live call as work it would lose
  (`store.ts:1790-1808`); only the manual buttons needed copy.

**Docs to update when each milestone lands:**
- CLAUDE.md § Audio calls (§ 6.3's wording after D1; ownership; devices; local health; the new end
  reasons).
- CLAUDE.md § A call is a PAGE (the phone layout and its invariant).
- CLAUDE.md § Joining a meeting (the pre-join, Join muted, the proof on Join now).
- CLAUDE.md § Recording a call (the recorder re-points on a device swap).
- CLAUDE.md § Automation safety (`call_media_state`, the driver modes, the switches, the captures).
- NATIVE-CALLING.md:
  - § 7 (consent, matching D1);
  - § 6 (the surface table);
  - § 8 "Still open" (relayConfig from the account configuration or a provider);
  - a correction that the client never ICE-restarts;
  - the relink and resume protocols;
  - each measurement in § 9 as it is taken.
- The `call-media.ts` module header (who owns the microphone; the gather rule; why `max-bundle` is
  not copied).
