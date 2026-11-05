# Ringing, alerts, missed calls and call history

**Workstream:** `ringing-alerts-history` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

Every incoming call should reach the user, and the calls that came and went should be findable
afterwards. This design covers eight things:

1. **An open page rings.** A ringtone this app synthesizes itself, gated on `soundsEnabled` and a new
   Ringtone switch. One tab per browser origin rings. A local Silence stops the sound without declining.
   A hidden tab gets an OS popup. The ring stops on every way out.
2. **The ring stops when the call is over somewhere else on this machine.** This machine runs two
   registered installs that both ring. When the user answers or declines on one, the other is told
   through the shared store. When Teams posts its own "missed" or "ended" line in the thread, the ring
   can also end there. That second path ships behind a switch, in dry-run mode, until it is measured.
3. **Do not disturb rings quietly.** The card stays; there is no sound and no popup.
4. **An outgoing call says "Ringing…"** once the service reports that the callee's device rings.
5. **A Web Push rings the user's phone while the app is closed.** It is a deliberate widening, behind a
   setting that is off by default. A short TTL, one claim per call across both backends, an RFC 8291
   payload, and a "ring over" push that replaces the notification when the ring ends.
6. **Missed-call alerts** (push and page, one setting) built on Teams' own `callMissed` lines.
7. **A read-only Recents list** in the bell panel, built from the call lines already in the store, with
   Call back and Join. It is preceded by a READ-ONLY recon of Teams' `48:calllogs` feed and of the call
   lines themselves.
8. **Calling keyboard chords and accessibility.**

Voicemail is recon-only. Call forwarding is rejected for this round.

**The verified audio path is not touched by M1–M4.** That path is 1:1 both ways and the meeting join.
`handle_incoming_call` keeps its insert-then-emit unchanged. Everything new runs in a task spawned after
that first emit, or in the page. The one state change that could affect a verified call is the
provisional-answer handling (M6b). It waits for a measurement and then sits behind a runtime switch.

## 2. Where we are today

Everything below was re-checked against this worktree (HEAD `c941850`).

**The ring**

| Fact | Where |
| --- | --- |
| An invite is inserted into the plane and emitted once. It plays no sound, raises no popup and sends no push. | `src/bin/server.rs:14946-15036` (busy rule `:15026-15031`, emit `:15035`) |
| `conversation_id` comes from the invite alone: `groupChat.threadId`, `threadId`, then `conversationId`, starting with `19:`. | `src/calling.rs:541-545` |
| `call_id` falls back to `callNotification.conversationId` when `callId` is absent. | `src/calling.rs:535-540` |
| Calling frames are consumed strictly one at a time, so any await in `handle_incoming_call` stalls every later frame. | `src/bin/server.rs:14506-14510`, `:15555-15558` |
| `end_call_locally` ends whatever call the plane holds. It checks no id and sends nothing. | `src/bin/server.rs:15074-15107` |
| `CALL_RING_TIMEOUT` is 60 s. It applies only to `Ringing \| Dialing`. Its doc says answering moves the phase out of those two. | `src/bin/server.rs:759-773`, `:15168-15186` |
| The 60 s timeout is swept by the 20 s keepalive ticker (today; incoming-and-conversation M6 replaces it with the 5 s `CALL_TICK`, which this design then cites, README § 3 collision 4). | `src/bin/server.rs:14533-14546` |
| `call_accept` keeps the phase `Ringing` until the accept POST answers. If the slot is gone by then, `hang_up_orphan` POSTs a hangup the caller is shown. | `src/bin/server.rs:7815-7880`, `:15120-15135` |
| The Answer flow starts with `call_prepare {call_id}` on a Ringing call. It hands back the offer, opens the microphone, and only then calls `call_accept`. | `web/src/lib/store.ts:3097-3117`, `src/bin/server.rs:7486-7501` |
| On an Answer failure, the page reports "The call could not be answered." and calls `hangUpCall`. | `web/src/lib/store.ts:3112-3117` |

**The page**

| Fact | Where |
| --- | --- |
| `onCallState` stores the status, releases media, and reports an ending. It plays no cue. | `web/src/lib/store.ts:2623-2643` |
| `RingingCard` offers Answer (size `sm`) and Decline (`size-9`, aria-label "Decline"). It is `role=alert` and silent. The phone icon pulses unconditionally (`animate-pulse`). | `web/src/components/call-bar.tsx:99-161` |
| `callEndLabel` falls to "The call ended." for `CallEndReasonUnanswered`. | `web/src/lib/call.ts:341-374`, `src/bin/server.rs:15185` |
| The awareness path turns Teams' `Event/Call` lines into banners and `notifyCall`. `started` rings and `ended`/`missed` clear. It is emitted on EVERY received frame, not only on an insert. | `src/bin/server.rs:12185-12226`, `:15341-15350`; `web/src/lib/store.ts:2507-2547` |
| Page call popups use tag `teams-lite-call`. Message popups use `teams-lite-<conv>`, while message pushes use `<conv>`. So a hidden tab and a push on one device can already show two rows for one message: an existing wart this design does not repeat. | `web/src/lib/notify.ts:59-83`, `src/push_policy.rs:308` |
| The awareness card is hidden when the live call names the same conversation. | `web/src/components/incoming-call-banner.tsx:39-43` |

**Store and live feed**

| Fact | Where |
| --- | --- |
| The live ingest loop broadcasts every frame and pushes only under `inserted`. The comment records that gating per-process state on `inserted` is wrong with two backends. | `src/bin/server.rs:15383-15440` |
| A call line keeps `event` (missed, started or ended), duration and participants. An unknown `callEventType` becomes `"ended"`. The raw XML is dropped at parse time (`content: ""`), so no call id survives. | `src/teams_read.rs:1160-1219`, `:1658-1680` |

**Push**

| Fact | Where |
| --- | --- |
| `push_policy` says a system line never notifies. | `src/push_policy.rs:15-16`, `:260-262` |
| The page mirrors that rule. | `web/src/lib/protocol.ts:2432-2449` |
| Every push uses TTL 3600 s and `urgency: high`. | `src/push.rs:83`, `:272-275` |
| `Notification` is `{title, body, url, tag}`. | `src/push.rs:230-241` |
| The service worker knows only that shape. It skips when a window is visible and must show something for every push it receives (`userVisibleOnly`). | `web/public/sw.js:30-71` |
| `deliver_push` refuses read-only before the network. | `src/bin/server.rs:12827-12838` |
| The message push is claimed on `<conversation>/<message>` through `claim_once` (INSERT OR IGNORE into `push_deliveries`, kept 24 h). | `src/bin/server.rs:12935-12944`; `src/store.rs:4121-4126`; `src/bin/server.rs:290` |

**Presence, tokens and the shared store**

| Fact | Where |
| --- | --- |
| Own presence is readable: `fetch_presence` POSTs `getpresence` with a PROFILE token plus skypetoken. The header records that the user's own presence was read back after each Always-available step. | `src/teams_presence.rs:1-35`, `:142-171` |
| `TokenCache::get` acquires on a miss, which can enter the interactive rescue (8 s). `cached_fresh` is private. | `src/auth.rs:254-293`, `:27`, `:600-640` |
| The store's schema is v19, and an `open` re-runs `initialize` (DDL and ANALYZE) whenever `user_version` differs. So a v20 binary and the lagging released v19 binary sharing one store would flip it on every open. | `src/store.rs:590`, `:2026-2033`, `:2011-2022` |

**History, the bell and the line**

| Fact | Where |
| --- | --- |
| There is no Recents. The bell has three feed tabs keyed by `NOTIFICATION_TABS`, which also keys `notifications: Record<NotificationTab, Notification[]>`. | `web/src/lib/protocol.ts:1070-1071`; `web/src/lib/store.ts:690`; `web/src/components/notifications-bell.tsx:64-176` |
| Every `48:` thread is filtered out of the chat list, `48:calllogs` included. | `src/teams_activity.rs:48-55`, `:375` |
| `fetch_activity_stream` is the GET shape for those feeds. | `src/teams_activity.rs:147-175` |
| A call line is a pill (`py-1`, 16 px text, an optional 24 px avatar stack) inside a `my-2` row. It carries no action. | `web/src/components/call-event-line.tsx:26-43`; `web/src/components/system-line.tsx:28-46` |
| The virtualizer's estimate knows only chess rows, time marks and thread foot rows. | `web/src/components/message-pane.tsx:835-848` |

**The bundle**

| Fact | Where |
| --- | --- |
| On the caller side, `callProgress` sets RINGING and resets the client's outgoing establishment timeout. That timeout is 90 s, or 185 s for a 1:1 PSTN call. | `calling-pluginless.js`, `handleCallProgress` / `ZP` |
| Every `mediaAnswer` received while OUTGOING is `provisional:!0` and is not acknowledged. The final answer is the `callAcceptance`. | `calling-pluginless.js` |
| The mock's invite replaces any live call, and the mock never expires a ring. | `web/mock/server.ts:4966-5000` |

## 3. Gaps covered, deferred and rejected

**Covered:** `incoming-ring-alerting`, `ring-alert-open-page`, `web-push-incoming-call`,
`missed-call-notifications`, `dnd-and-silence`, `call-history-view` (local Recents v1 plus a READ-ONLY
`48:calllogs` recon; adoption depends on what the recon finds), `call-line-actions` (Call back and Join on
the newest line; honest end labels), `keyboard-shortcuts-a11y`.

**Deferred or rejected:**

| Item | Decision and why |
| --- | --- |
| `voicemail` (both entries) | **Deferred.** A READ-ONLY Graph recon (M12) first measures whether Cloud Voicemail items exist on this tenant (class `IPM.Note.Microsoft.Voicemail.UM.CA` is inferred). Playback, if built, goes through the read-only mail path and never marks an item read (§ Mail). Sending a caller to voicemail is outward and belongs to `advanced-call-control` (`outgoing-transfer-consult`). |
| `call-forwarding-settings` | **Rejected this round.** Every write (`changeCallForwardingTargetTo`, `disableCallForwarding`, GraphQL in `calls-main-app.js`) re-routes the user's calls account-wide. There is no sandbox and the REST service behind it is not in the chunks. A deliberate feature would need a READ-ONLY recon of the read, its own `OUTWARD_METHODS` entry, a read-before-write that keeps the prior state (the `forceavailability` lesson), a read-back and a mock. |
| Publish missed-call read state (`markMissedCallsAsRead`, `setCallHistoryConsumptionHorizon`) | **Rejected.** It is outward exactly as `mark_read` is. The seen watermark stays local, and Teams' badge on other devices keeps its count; that limit is stated. |
| Answered-elsewhere naming through the service (`acceptedElsewhereBy`, codes 450/451/452/487) | **Deferred to `incoming-and-conversation`** (`answered-elsewhere`, `ring-cancelled-or-answered-elsewhere`). The service tells only an endpoint that ATTACHED at ring time (NATIVE-CALLING.md § 8a). This design covers what this machine can know itself (§ 4.4) and Teams' own line (§ 4.3). `end_call_locally_for` is idempotent, so both coexist with attach-at-ring. |
| Join an ongoing GROUP-chat call from its line | **Deferred to `meeting-participation`** (`join-ongoing-group-call`). There is no join address for it, and placing a call there rings everybody. |
| `MeetingEventLine`'s external Join link | **Owned by `meeting-awareness`** (their M7 depends on this workstream's line-action vocabulary, § 7.8). |
| Teams ringtone picker (`playAudioFile` / `pauseAudioFile`) | **Rejected.** The files are Microsoft's assets, not in the chunks, and fetching them from a page is a third-party request (the chess.com licensing argument). One synthesized ring and one ringback. |
| Answer or Decline buttons in the service-worker notification | **Rejected.** Decline is `call_hangup` (OUTWARD) and the worker holds no write token. An Answer action cannot open a microphone in a window with no gesture. iOS offers no actions. A tap opens the app on the ringing card. |
| Priority contacts during DND | **Deferred.** The list (`priorityContacts`, `skipPriorityContacts` in `calls-main-app.js`) has no measured read. v1 rings EVERY call quietly in DND and says so. |
| A sixth "Calls" sidebar tab | **Rejected** in favour of a Calls tab in the bell. Six tabs do not fit a 390 px strip of five, and `48:` feeds belong to the bell (`teams_activity.rs:48-55`). |
| Call waiting | **Out of scope** (`advanced-call-control` › `call-waiting`). The second invite is dropped (`server.rs:15026-15031`), and the missed-call alert still covers it. |
| "Forwarded to voicemail…" and "Answered by voicemail" labels | **Deferred.** `forwardingDestinationType` is seen only as `\|\|"user"`, with no `voicemail` literal. The acceptance can come from `KNOWN_BOTS.VOICEMAIL_BOT_ID` = `28:14524421-25b4-403a-a0f7-e62d4379cabc` (`callAcceptedByNGCVoicemail`). Both are journalled (§ 4.5) and labelled only once measured. |

## 4. Protocol

Status vocabulary: **measured** (seen against this tenant), **read-from-bundle**, **inferred**, **local**
(no protocol).

### 4.1 Detect an incoming ring (unchanged), plus two derived fields

- **Method:** service → us, calling trouter socket, `evt: 107`.
- **Link:** the calling worker's surl, registered as `SkypeSpacesWeb_2.6` (`server.rs:14454`).
- **Body** (read by `calling::incoming_call_from_frame`, `calling.rs:514-570`):

  ```json
  {"callNotification":{"from":{"id":"8:orgid:…","displayName":"…"},"callId":"…",
   "groupChat":{"threadId":"19:…"},"callModalities":["audio"],
   "mediaContent":{"blob":"…","contentType":"application/sdp-ngc-1.0"},
   "to":{"endpointId":"00000000-…","participantId":"…"},
   "links":{"attach":"…","progress":"…","reject":"…","mediaAnswer":"cc://ma"}}}
  ```

- **New derived field 1: `service_call_id: Option<String>`.** It comes ONLY from `/callId` or
  `/debugContent/callId`. The existing `call_id` (which may fall back to `/conversationId`) stays exactly
  as it is for the session. A `conversationId` is stable per thread, so a claim keyed on it would swallow
  every later ring in that thread for 24 h.
- **New derived field 2: `alert_thread_id: Option<String>`.** The thread the ring is ABOUT, used ONLY for
  alerts. It is `invite.thread_id`, else the stored 1:1 thread with the caller
  (`Store::one_to_one_thread_with`, both orders of `19:<oid>_<oid>@unq.gbl.spaces`, over the existing
  `one_to_one_parties`, `store.rs:1701-1714`). **`CallSession.conversation_id` is left exactly as the
  service stated it.** A non-empty `conversation_id` would open the call stage's chat panel by default
  (`call-stage.ts:44-48`, `:294-301`), and opening a conversation publishes `mark_read` — a read receipt
  the caller sees (§ A call is a PAGE). An alert-only field changes nothing about the stage. The cost of
  a wrong guess for an invite that carried no thread (a group call, unmeasured) is a mis-targeted alert
  URL and a ring the Teams line cannot end (§ 4.3), never a read receipt.
- **The alert key:** `alert_thread_id`, else `caller_mri`. It is spelled once (`calling::alert_key`) and
  used for the notification tag, the claims and the handled record.
- **Bundle:** NATIVE-CALLING.md § 8a, measured 2026-09-04 ("an invite is `evt: 107` with
  `callNotification` at its root").
- **Status:** measured (invite). Whether a forked 1:1 invite carries `groupChat.threadId`, and whether
  both installs receive the same `callId`, is unmeasured. Both are journalled by M2 (§ 9).

### 4.2 The user's own presence at ring time (DND → quiet)

- **Method:** POST, after the first emit, in a spawned task (`Ctx::settle_ring_quiet`).
- **Link:** `{unifiedPresence}/v1/presence/getpresence/` (`teams_presence::presence_host`,
  `teams_presence.rs:178-185`). Auth: Bearer PROFILE token + `x-skypetoken`.
- **Body:**

  ```json
  [{"mri":"<session.self_mri>"}]
  → [{"mri":"…","presence":{"availability":"DoNotDisturb","activity":"Presenting"}}]
  ```

- **Token: cached only.** A new `TokenCache::cached(scope) -> Option<String>` exposes `cached_fresh`.
  With no fresh PROFILE token (avatars and presence keep it warm on any page connect), the ring is NOT
  quiet and the journal says so. A broker acquisition is never started here and never wrapped in a
  cancelling timeout: dropping `get_token` mid-rescue skips the `StillWaiting` take-back and strands the
  broker's window (`auth.rs:612-635`).
- **Bound:** the HTTP request alone runs under `tokio::time::timeout(1.5 s)`. Dropping an HTTP future is
  safe. A failure or timeout means not quiet.
- **Quiet:** `availability == "DoNotDisturb"` (`teams_presence::is_do_not_disturb`).
- **Wire to the page:** the first emit carries `quiet: "pending"`. The task re-emits `quiet: "dnd"` or
  `quiet: null` only if the plane still holds an incoming Ringing call with this id. The page treats
  `pending` as "no sound and no popup yet" for at most `RING_QUIET_WAIT_MS` (2 s) after it received the
  frame, then rings regardless. A lost re-emit can therefore never silence a ring. An absent field (an
  older backend) means `null`.
- **Bundle:** none for DND semantics. Teams has DND with priority access (`calls-main-app.js`:
  `priorityContacts` ×17, `skipPriorityContacts`).
- **Status:** the read is measured. Whether a manual DND shows through while Always available registers
  an Available endpoint, and whether Teams delivers invites at all in DND, are inferred (M4 live step).

### 4.3 End the ring when Teams posts that the call was missed or ended

- **Method:** live messaging trouter frame (service → us). We send nothing.
- **Link:** the ingest loop, next to the awareness emit (`server.rs:15341-15350`), run on EVERY received
  frame. **Not under `inserted`.** Two backends share the store and both hold their own ring; the one that
  loses the insert race would otherwise ring on (`server.rs:15395-15407` states that lesson for
  broadcasts).
- **Body:** `messagetype: Event/Call`, e.g.

  ```xml
  <partlist alt=""/><callEventType>callMissed</callEventType>
  ```

  The parser gains one key in the stored `system_event`: `"call_event_type": "<raw value>"`, present only
  when the XML carried an explicit `callEventType`. It is additive JSON in an existing column, not a
  schema change, and the existing keys and their meaning are unchanged.
- **Rule** (`ring_ended_by_line`, pure). Every condition must hold:
  - the plane holds an INCOMING call in phase `Ringing`;
  - no Answer is in flight. A new `CallSession.accepting_since_ms` is set by `call_prepare {call_id}` on a
    ringing call and cleared on failure; it counts for 30 s;
  - `alert_thread_id == message.conversation_id`;
  - raw `call_event_type` is exactly `callMissed` or `callEnded` (an unknown type never ends a ring);
  - `started_at_ms − 5 s ≤ compose_time ≤ now + 5 s`.
- **Action:** `end_call_locally_for(call_id, END_REASON_RING_MISSED | END_REASON_RING_OVER)`. It is a new
  variant that checks the id under the same lock, so a check-then-end race cannot reach another call.
  `end_call_locally` keeps its signature and delegates.
- **Why it is a switch, and dry-run first.** The line carries no call id, and when Teams stamps
  `compose_time` (at the start or the end of the ring) is unmeasured. A caller who hangs up and redials
  within ~5 s, with the old line arriving after the new invite, would end an answerable ring. So
  `TEAMS_LITE_RING_ENDS_ON_LINE=1` (an env var read at startup) enables the ending. The precedent this
  first cited, meeting-awareness' `TEAMS_LITE_SERVER_MUTE`, has since moved into the stored
  `call_protocol_set` (README § 3, collision 10); whether this switch moves too is README § 5, D9. With the switch OFF (the default), the same rule writes one
  journal line:

  ```
  [calling] a callMissed line for the ringing call's thread: compose_time is +4210 ms after the ring
  started; the switch is off, so the ring stays
  ```

  That line is the measurement. Once M3 shows whether the XML carries a call id, the rule matches on it
  and the window narrows to it.
- **Bundle:** not a calling-plane frame. The shape is pinned by `teams_read.rs:3866-3876`, and the page
  already clears its awareness banner on these lines (`store.ts:2507-2524`).
- **Status:** the line types are measured as stored shapes. Arrival timing and stamping are inferred and
  measured by the dry run and by M3.

### 4.4 End the ring when the user handled it on another install of this machine

- **Method:** local, through the shared SQLite store. Nothing leaves the machine.
- **Why:** both installs (19420 and 19422) register and ring (§ Running the released build BESIDE). An
  endpoint that never attached is not told when the call is answered elsewhere (NATIVE-CALLING.md § 8a),
  so answering on 19440 leaves 19442's ring running. Once M1 ships, that becomes audible for up to 35 s.
- **Write.** After a successful `call_accept` POST, and after `call_hangup` on a Ringing incoming call,
  the backend writes the setting `call_ring_handled:<alert key>`:

  ```json
  {"outcome":"answered"|"declined","at_ms":<now>,"call_id":"…","service_call_id":"…"|null,"port":19420}
  ```

  The write is best-effort, comes after the POST, ignores errors and cannot change a POST.
- **Read.** `handle_incoming_call` spawns `watch_ring_handled(call_id, alert_key, started_at_ms)` after the
  emit. It reads that key once a second while this call is Ringing (≤ 60 reads). It ends the ring with
  `end_call_locally_for(call_id, END_REASON_ANSWERED_HERE | END_REASON_DECLINED_HERE)` when all of these
  hold:
  - the record's `port` is not ours;
  - `service_call_id` values are equal where both exist, else `at_ms ≥ started_at_ms − 5 s`.

  The journal records which match applied and whether the ids were equal. That line also measures
  whether forked invites share a `callId`.
- **Default ON.** It is driven only by the user's own press on this machine and matched on caller and call
  id, and M1 would otherwise turn an existing leak into an audible fault.
- **Status:** local; the matching semantics are inferred.

### 4.5 Outgoing call: the callee's device is ringing (`callProgress`)

- **Method:** service POSTs our callback.
- **Link:** `{surl}callAgent/{session}/{cause}/call/progress/`. It is already published as
  `links.progress` (`calling.rs:1284`, `:1832`; `paths::CALL_PROGRESS`, `calling.rs:98`). Today the frame
  is received and ignored.
- **Body:**

  ```json
  {"callProgress":{"status":"ringing"|"forwarded","forwardingDestinationType":"user"}}
  ```

- **Handling:** a branch keyed on the frame URL's tail (`calling::callback_path(&frame.url)` ends
  `/call/progress/`) or on `/callProgress`, `/_decoded/callProgress`. It **does not return**: the
  acceptance, renegotiation and answer branches below still run on the same frame. For an OUTGOING call in
  `Dialing` it sets `remote_progress: "ringing" | "forwarded"` and emits. The raw
  `forwardingDestinationType` goes to the journal and the call-live digest.
- **The acceptance's `acceptedBy.id` prefix** is journalled too. `28:` vs `8:` measures whether a
  voicemail bot answered: `KNOWN_BOTS.VOICEMAIL_BOT_ID`.
- **Bundle** (`calling-pluginless.js`):

  ```js
  handleCallProgress(e){…const i=e.body.callProgress;this.fsmState===I.SIGNALING_FSM_STATE.OUTGOING&&
  (this.telemetryHelper.setTimeToRingDuration(),this.resetCallEstablishmentTimeout(t),
  this.onCallStatusChanged(I.CALL_STATUS.RINGING,t),i.status==="forwarded"&&
  this.signalingSessionCallback.onCallForwarded({destinationType:i.forwardingDestinationType||"user"},t))}
  ```

  `ZP`: `outgoingCallEstablishmentTimeoutSec:90,longerOutgoingCallEstablishmentTimeoutSec:185`.
- **Status:** read-from-bundle; unmeasured on the tenant.
- **Coordination:** the client resets its 90 s establishment bound on every progress.
  `remote_progress_at_ms` is published for `plane-reliability` › `outgoing-ring-timeout-cancel`, which
  owns the outgoing bound.

### 4.6 Outgoing call: a provisional media answer (contingent M6b)

- **Method:** service POSTs `{surl}…/call/mediaAnswer/` while we dial, BEFORE `callAcceptance`.
- **Body:**

  ```json
  {"mediaAnswer":{"sender":{"endpointId":"…"},"mediaContent":{"blob":"…"},"noRingBack":false}}
  ```

- **Bundle** (`calling-pluginless.js`):
  - Every answer in OUTGOING is provisional and is not acknowledged; a retried one is ignored:

    ```js
    handleMediaAnswer(e){…if(this.fsmState===I.SIGNALING_FSM_STATE.OUTGOING){if(Bg(e.headers,
    this.provisionalMediaAnswersSeenSoFar)){…ignoring provisional answer retried by service…return}…
    t.mediaAnswer.noRingBack||(…this.onCallStatusChanged(I.CALL_STATUS.RINGING,i));…
    this.signalingSessionCallback.onAnswer({provisional:!0,renegotiation:!1,…mediaContent:r},i)}
    ```

  - The final answer is the acceptance:

    ```js
    processCallAcceptance … onAnswer({provisional:!1,…callAcceptedByNGCVoicemail:l===I.KNOWN_BOTS.VOICEMAIL_BOT_ID,
    …mediaContent:a},t),…this.onCallStatusChanged(I.CALL_STATUS.CONNECTED,t)
    ```

- **Today:**
  - `media_answer_from_frame` reads `mediaAnswer` AND `callAcceptance` (`calling.rs:695-714`). Any answer
    sets `Connected` and starts `connected_at_ms` (`server.rs:14891-14915`), and every answer is
    acknowledged (`:14929-14935`).
  - The page applies the FIRST answer as `type: "answer"`. A later one is dropped, because the state is
    no longer `have-local-offer` (`call-media.ts:991-994`).
  - If a provisional answer ever precedes the acceptance, today's app shows a running call timer while the
    callee still rings. It applies the provisional SDP as final and drops the acceptance's.
- **M6 measures, M6b decides.** M6 adds a journal line ("a media answer arrived before the acceptance")
  and the call-live `pathOrder`. M6b, behind `TEAMS_LITE_PROVISIONAL_ANSWERS=1` (env, read at startup,
  OFF), is built ONLY if a run shows a provisional answer:
  - A pre-acceptance answer keeps `Dialing`, sets `answered_provisionally_at_ms`, emits `call_media` with
    `kind: "pranswer"`, and is NOT acknowledged (the client's shape).
  - The acceptance moves to `Connected` and emits its own `call_media` with `kind: "answer"`. The page
    applies `pranswer` then `answer`, which WebRTC allows from `have-remote-pranswer`.
  - The 60 s sweep must not end a call whose acceptance is merely late. A `Dialing` call with
    `answered_provisionally_at_ms` set is bound instead by the client's own 90 s, measured from the latest
    progress or provisional answer. The `CALL_RING_TIMEOUT` doc invariant is restated for it, and a Rust
    test pins that such a call is not swept at 60 s.
  - With the switch OFF, today's behaviour is pinned by a test.
- **Status:** read-from-bundle. A Teams→Teams 1:1 sending a provisional answer is UNMEASURED.

### 4.7 Ring the user's own devices for an incoming call (Web Push)

- **Method:** RFC 8030 POST per stored subscription. The payload is `aes128gcm` (RFC 8291) with VAPID
  (RFC 8292). Headers `ttl: 45` (`push::CALL_RING_TTL_SECS`) and `urgency: high`.
- **Link:** each `push_subscriptions` endpoint, allow-listed by `push::is_supported_endpoint`.
- **Plaintext before encryption:**

  ```json
  {"title":"Incoming call","body":"Lucas Silva is calling","url":"/c/<alert thread>","tag":"teams-lite-call:<alert key>",
   "kind":"call","alert_key":"<alert key>","expires_ms":<rang_at + 35000>,
   "late_title":"Lucas Silva called","late_body":"Tap to open the chat."}
  ```

  For a group, the body is `"Lucas Silva · Design crew"`. `url` is `"/"` when no thread is known. The new
  fields are `Option` with `skip_serializing_if`, so a message payload stays byte-identical. There is no
  Topic header and no text the push service can read.
- **Gates, in order** (`Ctx::push_incoming_call`, inside the same spawned task as § 4.2, after the quiet
  decision):
  1. `read_only()`;
  2. `push_subscriptions_exist`;
  3. `SETTING_CALL_RING_PUSH == "1"`;
  4. `quiet != dnd`;
  5. the plane still holds this call in `Ringing`;
  6. claim;
  7. `deliver_push(…, CALL_RING_TTL_SECS)`, which refuses read-only again before the network.
- **Claim key:**
  - `call-ring/<service_call_id>` when the service named one;
  - else `call-ring/<caller_mri>/<alert key>/<bucket>`, with `bucket = floor(rang_at / 120 s)`. Both
    `<bucket>` and `<bucket−1>` are claimed, and the push goes out only if neither was taken. Two backends
    straddling a boundary therefore push once. The cost is stated: a second real call from the same
    caller within the next bucket does not ring the phone. That happens only when the service names no
    `callId`, which is unmeasured.
- **Tag:** the page's own call popup uses the same tag (§ 5, `notify.ts`). A hidden tab and a push on one
  device replace each other instead of stacking.
- **Expiry:** `expires_ms` is rang_at + 35 s (Teams rings ~35 s, `server.rs:768-770`), under a TTL of
  45 s. A push delivered late shows the late wording. A clock-skew tolerance of 5 s is applied in the
  worker. A notification already shown is corrected by § 4.8, not by `expires_ms`.
- **Bundle:** none. The Teams web client does not ring a closed browser (round-1 search of
  pluginless/shell/app; `IncomingCallNotification` is a signaling handler). This is teams-lite's own design.
- **Status:** the transport is measured for message pushes (`push.rs`, RFC test vector). The call payload
  and its timing are new and unmeasured.

### 4.8 Ring over: replace the ring notification when the ring ends

- **Method:** the same transport, TTL 300 s.
- **Trigger:** `Ctx::ring_left(call, outcome)`, called at the two places an incoming call leaves `Ringing`:
  `call_accept` success, and `end_call_locally_for` when the phase was `Ringing`. It runs on whichever
  backend sees the ring end, only if the ring's claim key exists (new `Store::is_claimed(key)`), and is
  claimed once on `call-ring-over/<ring claim key>`. It runs whatever the Missed-calls switch says.
- **Body:**

  ```json
  {"title":"Lucas Silva called","body":"Tap to open the chat." | "Answered in teams-lite." | "You declined it.",
   "url":"/c/<alert thread>","tag":"teams-lite-call:<alert key>","kind":"ring_over","alert_key":"<alert key>"}
  ```

- **Worker** (`kind: "ring_over"`):
  - If the tag's current notification is a `missed_call`, re-show it unchanged and silently. That
    satisfies `userVisibleOnly` and keeps "Missed call".
  - Else, with a visible client, close any `call` row and show nothing. A visible page is the one case a
    platform tolerates (`sw.js:51-55`).
  - Else, replace it silently (`renotify: false`, `silent: true`).
- **Why:** without this, "{name} is calling" stays on the lock screen after an answer here, an answer on
  native Teams, a decline, or a local expiry. That is a false claim about the present.
- **Status:** local design over the measured transport.

### 4.9 Missed-call alerts (push and page, one setting)

- **Source:** Teams' `callMissed` line (measured stored shape).
- **Decision, spelled once:** `push_policy::missed_call_alert(message, placement, from_me, handled,
  now_ms)`. It is true only when all of these hold:
  - the raw `call_event_type == "callMissed"`;
  - `Placement::Chat`;
  - not `from_me`;
  - not deleted;
  - not stale (`MAX_AGE_MS`);
  - no `call_ring_handled` record for that thread with `at_ms` within 120 s before `compose_time`. A call
    the user answered or DECLINED here is not "missed" to them.
- **The setting:** `SETTING_MISSED_CALL_ALERTS == "1"`, OFF by default. It governs BOTH halves.
- **Push** (in `push_live_message`, under `inserted` and the existing `<conversation>/<message>` claim,
  only when `notification_for` is None):

  ```json
  {"title":"Missed call","body":"Lucas Silva","url":"/c/<conversation>","tag":"teams-lite-call:<conversation>","kind":"missed_call"}
  ```

  TTL is `push::MESSAGE_TTL_SECS`. The worker shows it with `renotify: false` when it replaces a `call`
  row, `true` otherwise.
- **Page: the page is TOLD rather than mirroring.** This is the `mentions_me` pattern: the decision
  depends on a setting and on handled records the page cannot see. `message_json` gains
  `missed_call_alert: true` on such a line. The page's `shouldNotifyMissedCall` requires it plus its own
  rules: not the open conversation, not already known, not stale, chat not muted. ABSENT (an older
  backend) means no alert, the narrow answer for a behaviour that is new.
- **Bundle:** Teams keeps a badge (`calls-main-app.js`
  `fragment ComponentsCallsHeaderEntryCallingBadgeFragment on CallingBadge {id,totalCount,countsByUsers
  {userMri,count,unreadCalls {messageId,activityId,message {id}}…}}`).
- **Status:** inferred. The `from` semantics of a missed line in both call directions are measured by M3,
  and M8 (this feature) ships only after M3.

### 4.10 Recents (local v1)

- **Method:** open RPC `recent_calls {limit?: 1..200, since_ms?}`. Store read only, no network, answered
  read-only too.
- **Query** (`Store::recent_call_events`): `messages` rows over `SELECT_COLS`, mapped through
  `msg_reader`, so the `nicknamed!` ladder and the seal apply:

  ```sql
  WHERE system_event LIKE '%"kind":"call"%' AND system_event NOT LIKE '%"meeting":true%' AND deleted = 0
  ORDER BY compose_time DESC LIMIT ?
  ```

  The key order is stable because `serde_json` is built without `preserve_order` (`Cargo.toml:18`), so a
  `Value::Object` serializes with sorted keys and no spaces.
- **No index and no schema bump.** A bump flips `user_version` between the v20 staged pair and the lagging
  v19 released build on every open (§ 2). The scan over ~12k rows is expected to be single-digit
  milliseconds (unmeasured). If a measurement says otherwise, a partial index rides the next schema bump
  another workstream makes.
- **Row:**

  ```json
  {"id":"…","conversation_id":"…","title":"…","kind":"one_on_one|group|…","compose_time":…,"event":"missed|ended|started",
   "duration_seconds":…,"participants":[…],"participant_mris":[…],"is_self":…,"peer_mri":"…",
   "handled_here":"answered"|"declined"|null}
  ```

  - `title`: `Store::conversation_context(id, self_mri)`, which resolves a 1:1 through the user's own
    nickname for the other party.
  - `peer_mri`: the 1:1's other party (`one_to_one_parties`).
  - `handled_here`: from the latest `call_ring_handled` record of that thread within 120 s, so a declined
    call reads "Declined" rather than a red "Missed call".
- **Bundle:** Teams' own items carry `callDirection`, `isMissed`, `isDeclined`, `isAcceptedElsewhere`,
  `durationInMs`, `participantList` and more (field names of `CallHistoryItemSummaryQuery` in
  `calls-main-app.js`, the client's GraphQL schema, not a wire shape).
- **Status:** local.

### 4.11 Teams' call log (`48:calllogs`) and the call lines: READ-ONLY recon

- **Method:** GET.
- **Link:**

  ```
  {chatService}/v1/users/ME/conversations/48%3Acalllogs/messages?pageSize=30&view=msnp24Equivalent
  ```

  with `authentication: skypetoken=…`. It is the request `teams_activity::fetch_activity_stream` makes
  (`teams_activity.rs:147-175`), spelled in a new GET-only module `src/teams_calllogs.rs` that shares
  that URL builder. A verb-scan test in the module refuses any other verb, the pattern `gitlab.rs` and
  `mail.rs` hold.
- **What `examples/call_history_recon.rs` prints** (shapes only):
  - for `48:calllogs`: top-level keys, `messagetype` counts, `properties` key names, and enum-like scalar
    values. "Enum-like" is defined and unit-tested: ≤ 24 chars, `[A-Za-z_]+`, no digits, `@`, `:` or
    spaces.
  - for the 10 newest 1:1 conversations' `Event/Call` lines: the `callEventType` counts, the XML element
    names (does a `callId` exist?), `from` relative to self (self / other / empty), and the line's
    `compose_time` relative to the neighbouring messages.

  It never prints a name, number or body.
- **Passive measure:** a once-per-process journal line when a live frame arrives on any `48:` feed. It
  measures whether `48:calllogs` is pushed live.
- **Bundle:** `calling-app.js` `const oe="48:calllogs"` … `Te=Ne=>Ne?.extendedCallType?.isOneToOnePstn?oe:Ne?.conversationId||""`.
  That snippet sits in the Copilot/transcript data-channel hook. It names a chatService conversation used
  for 1:1 PSTN call context and is **weak evidence** that this is a readable history feed. Adoption is
  fully contingent on the recon.
- **Status:** the request shape is measured for activity streams; the message shape is unmeasured.

### 4.12 Call back and Join from a line or a Recents row

- **Call back:** the existing `call_prepare {conversation}` (MACHINE) + `call_place {call_id, sdp}`
  (OUTWARD) → `invitation_payload` (`calling.rs:1237`). Measured (NATIVE-CALLING.md § 8b).
- **Join:** the existing `MeetingJoinButton` `{kind:"thread"}` → `call_prepare {meeting}` + `call_join`
  (OUTWARD), via `MeetingJoin::from_thread_id` (`calling.rs:1689`). **Unverified** (§ Joining a meeting),
  so the line's Join ships only after `cd web && bun run join-live -- --from-chat` has passed.

### 4.13 Voicemail: READ-ONLY recon (the feature stays deferred)

- **Method:** GET, Graph, broker token, over the `src/mail.rs` path. The request lives in a GET-only
  function there, so the module's existing verb scan covers it.
- **Link:**

  ```
  /me/messages?$filter=singleValueExtendedProperties/any(ep:ep/id eq 'String 0x001A' and ep/value eq
  'IPM.Note.Microsoft.Voicemail.UM.CA')&$select=id,receivedDateTime,hasAttachments&$top=10
  ```

  then `/me/messages/{id}/attachments?$select=contentType,size` (no `name`, which can carry a caller's
  number).
- **Bundle:** `calls-main-app.js` `voicemail`, `CallLogVoicemailSuccess {voicemailAudio {contentType,
  contentBytes, isProtectedVoicemail}}`; `calling-pluginless.js` `voicemailItemId`,
  `"StartCallToVoiceMail"`.
- **Status:** inferred.

### 4.14 Rejected: publishing missed-call read state

```graphql
mutation AppsCallsMainMarkAllCallsAsReadMutation {markMissedCallsAsRead {... on ActivityReadStatusUpdateError {errorFaced}}}
mutation ComponentsCallsMainEntrySetCallHistoryConsumptionHorizonMutation {setCallHistoryConsumptionHorizon}
```

These come from `async-apps-calls-main.js` and `calls-main-app.js`. The REST behind them is not in the
chunks. Rejected as an outward write.

### 4.15 Coordination: the callee's own `callProgress` after attach-at-ring (owned by `incoming-and-conversation`)

```js
this.signalingAgentConfig.sendProgressFromCC||this.sendProgress(t)
sendProgress(e,t=Ht()){…url:e.response.callInvitation.links.progress,…payload:zV(this)…}
function zV(e){…{payload:{callProgress:{sender:{…},status:"ringing",phrase:"ringing"}}}}
```

Read-from-bundle, not implemented here.

## 5. Changes by file

### Backend

**`src/calling.rs`**
- `IncomingCall.service_call_id` (only `/callId`, `/debugContent/callId`).
- `pub fn alert_key(thread: Option<&str>, caller_mri: &str) -> String`.
- `pub struct CallProgress { status, forwarding_destination: Option<String> }` and
  `call_progress_from_frame`.
- `answer_says_no_ring_back` (M6b).
- `accepted_by_prefix` (journal only).
- Constants `END_REASON_RING_MISSED = "CallEndReasonRingMissed"`, `END_REASON_RING_OVER =
  "CallEndReasonRingOver"`, `END_REASON_UNANSWERED = "CallEndReasonUnanswered"` (hoisted from
  `server.rs:15185`), `END_REASON_ANSWERED_HERE = "CallEndReasonAnsweredOnThisMachine"` and
  `END_REASON_DECLINED_HERE = "CallEndReasonDeclinedOnThisMachine"`.
- `CALL_NOTIFICATION_TAG_PREFIX = "teams-lite-call:"`.
- Tests:
  - a fixture per progress status, wrapped (`_decoded`) and unrelated frames;
  - `service_call_id` is None when only `conversationId` is present;
  - the constants and the tag prefix are named in `web/src/lib/call.ts` / `notify.ts`, through
    `include_str!` (the `the_page_knows_the_name_of_an_unreachable_ending` pattern, `calling.rs:2730`).

**`src/teams_read.rs`**
- `parse_call_event` adds `"call_event_type"` (raw, when explicit).
- Fixtures for callMissed, callEnded, an unknown type and an implicit `<ended/>`.

**`src/teams_presence.rs`**
- `pub fn is_do_not_disturb(&Presence) -> bool` + a test.
- The header gains one sentence: the user's own presence is also read at ring time.

**`src/auth.rs`**
- `pub fn cached(&self, scope) -> Option<String>` on `TokenCache`. It never acquires; there is a test that
  it does not.

**`src/push.rs`**
- `CALL_RING_TTL_SECS = 45` and `RING_OVER_TTL_SECS = 300`.
- `Notification` gains `kind`, `alert_key`, `expires_ms`, `late_title` and `late_body` (`Option`,
  `skip_serializing_if`, `#[derive(Default)]`).
- A byte-identity test for a message notification.

**`src/push_policy.rs`**
- The module doc gains one sentence: a system line never notifies, except a MISSED CALL when the user asked
  for missed-call alerts.
- Pure functions:
  - `IncomingRing<'a>`, `ring_notification`, `ring_over_notification(outcome)` and
    `ring_test_notification()` (its own constructor; copy "This is how a call rings on your devices.");
  - `ring_claim_keys(service_call_id, caller_mri, alert_key, rang_at_ms) -> Vec<String>`;
  - `missed_call_alert(…)`.
- `notification_for` is unchanged, and `a_system_line_never_notifies` (`:460`) stays green.

**`src/store.rs`**
- `recent_call_events(since_ms, limit)` (through `msg_reader`, with a rename test).
- `one_to_one_thread_with(self_mri, peer_mri)`.
- `is_claimed(key) -> bool`.
- **No schema change.**

**`src/teams_calllogs.rs` (new, M3) + `src/lib.rs`**
- `CALLLOGS_THREAD`, `fetch_raw_page` (GET), and the verb-scan test.

**`src/bin/server.rs`**
- `CallSession` (`:2229`) gains:
  - `alert_thread_id`, `service_call_id`, `quiet: Option<&'static str>`, `accepting_since_ms`;
  - `remote_progress`, `remote_progress_at_ms`, `answered_provisionally_at_ms` (M6b);
  - `ring_pushed: bool` (local hint only; the claim is the authority).
- `json()` (`:2341`) adds `alert_thread_id`, `quiet` and `remote_progress`.
- `handle_incoming_call` (`:14946`):
  - unchanged up to and including its emit, except that the session is built with `alert_thread_id`
    (a store read before the insert, like the existing `display_name_for_mri` read at `:14963-14968`) and
    `quiet: Some("pending")`;
  - after the emit, it spawns `settle_ring_quiet` → `push_incoming_call`, and `watch_ring_handled`.
- `call_prepare {call_id}` (`:7486-7501`) sets `accepting_since_ms`.
- `call_accept` (`:7815`):
  - after a successful POST, writes `call_ring_handled` (answered) and calls `ring_left(Answered)`;
  - on failure, clears `accepting_since_ms`.
- `call_hangup` (`:8369`) on a Ringing incoming call writes the record (declined) after its POST.
- `end_call_locally_for(call_id, reason)`; `end_call_locally` delegates. `ring_left` runs when the phase
  was `Ringing`.
- `handle_call_frame` (`:14601`):
  - the progress branch (non-returning);
  - the `accepted_by_prefix` journal line;
  - "a media answer arrived before the acceptance" (M6);
  - the M6b branches behind `PROVISIONAL_ANSWERS` (`OnceLock<bool>` from the env).
- Ingest loop (`:15341`), on every received frame: `end_ring_on_call_line(&m)` → `ring_ended_by_line`
  → `end_call_locally_for` (switch) or the dry-run line.
- `push_live_message` (`:12887`): the missed-call branch.
- `message_json` adds `missed_call_alert` on call lines.
- Settings:
  - `SETTING_CALL_RING_PUSH = "call_ring_push"` and `SETTING_MISSED_CALL_ALERTS = "missed_call_alerts"`,
    in `set_settings` (`:7293`) and `settings_json` (`:10224`). Only `"1"` is trusted, and both are OFF.
  - Key family `call_ring_handled:<alert key>`.
- `push_test` (`:5378`) takes an optional `kind: "call"`.
- New open RPC `recent_calls`.

**`examples/call_history_recon.rs` (new, M3)** and **`examples/voicemail_recon.rs` (new, M12)**
- READ-ONLY, shapes only (§ 4.11, § 4.13). The user runs them, or the agent with the user's go-ahead.

### Web

**`web/src/lib/ringtone.ts` (new) + test**
- Pure half:
  - `RING_RECIPE`: a soft two-note figure, 3 s period, peak gain ≤ 0.3, synthesized on the
    `chess-sound.ts:264-296` pattern, no file;
  - `RINGBACK_RECIPE`;
  - `RING_SOUND_MAX_MS = 35_000` and `RING_QUIET_WAIT_MS = 2_000`;
  - `ringDecision({call, soundsEnabled, ringtoneEnabled, silencedCallId, live, visible, primed, quietSince})`
    → `{sound, popup, flashTitle, state}`, where `state` is `"sound" | "silent" | "blocked" | "pending" |
    "none"`.
- Browser half, `class Ringer`:
  - `prime()` on the first pointerdown/keydown creates and resumes one `AudioContext`, silently.
  - `start(callId)` schedules the WHOLE ring on the audio clock behind one gain node (no timers, which
    hidden tabs throttle). `stop()` disconnects at once.
  - **A hidden document DOES play this**, which deliberately inverts the chess-sound rule
    (`chess-sound.ts:355-356`); the doc says so.
- **Election:** Web Locks name `teams-lite-ringer`, **per origin**. 19440, 19442 and the tailnet name each
  elect their own.
  - A focused, visible tab requests with `steal: true`; any other tab queues.
  - A holder whose lock was stolen (`AbortError`) stops and re-queues WITHOUT steal.
  - On grant, `ringDecision` is re-run after a microtask (pending socket frames land first), and the lock
    is released at once if the ring is no longer wanted.
  - Without `navigator.locks`, only a focused, visible tab rings.
- `navigator.vibrate` only on a coarse pointer and only after `prime()` saw a gesture (Chrome requires
  sticky activation).

**`web/src/lib/store.ts`**
- One `syncRinger()`, driven from STATE: `callStatus`, `live`, `soundsEnabled`, `ringtoneEnabled`, the
  silenced id, visibility, focus and pagehide. It covers `refreshCallStatus` (`:3024-3031`) and the
  `disconnected` handler (`:2372-2375`).
- `silenceRing()`: localStorage `teams-lite:silenced-call=<id>` (the storage event reaches same-origin
  tabs). No RPC.
- The hidden-page OS popup comes from the calling plane, once per call id, and closes when the ring ends.
- Title flash.
- `ringtoneEnabled` beside `applyPersistedSounds` (`:8160`).
- The message handler: `shouldNotifyMissedCall` → `notifyMissedCall` + `playCue("droplet")`.
- `recentCalls`, `loadRecentCalls` (reloaded on a live message carrying a call `system_event`),
  `callsSeenAt` (localStorage `teams-lite:calls-seen-ms`), `markCallsSeen` and `missedCallsUnseen`.

**`web/src/lib/notify.ts`**
- `notifyIncomingCall(caller, label, conversationId, alertKey)`: tag
  `` `${CALL_NOTIFICATION_TAG_PREFIX}${alertKey}` ``, `renotify: true`. It keeps the `Notification`
  handle, or uses `registration.getNotifications({tag})` on the worker path, so
  `closeCallNotification(alertKey)` can close it.
- `notifyCall` (awareness) uses the same tag when it knows the conversation.
- `notifyMissedCall(who, label, conversationId)`.
- Still never asks permission (`notify.ts:15-19`).

**`web/src/lib/call.ts` + test**
- `ActiveCall` gains `alert_thread_id?`, `quiet?: "pending" | "dnd" | null` and
  `remote_progress?: "ringing" | "forwarded" | null`.
- `callPhaseLabel`: dialing + ringing → "Ringing…"; forwarded → "Forwarded…".
- The five end-reason constants and `callEndLabel` cases (§ 7.4).
- `CALL_NOTIFICATION_TAG_PREFIX`.

**`web/src/lib/protocol.ts`**
- `shouldNotifyMissedCall`.
- `RecentCall`.
- `AppSettings.call_ring_push` / `missed_call_alerts` and the patch fields.
- `NotifiableMessage.missed_call_alert?`.
- `NOTIFICATION_TABS` is UNCHANGED. A new `BellTab = NotificationTab | "calls"` lives in the bell.

**`web/src/lib/call-history.ts` (new) + test**
- `recentCallLabel`: "Missed call", "Declined", "Call ended · 12 min", "Call started".
- `missedSinceSeen`: counts `event == "missed" && !is_self && handled_here == null`.
- `callBackOffered`: `one_on_one` and `conversationCallAction` (`call.ts:210`) can call.
- `newestCallLineIds`.
- `MEETING_ONGOING_GUESS_MS = 2 h`, stated as a guess.

**`web/src/lib/sounds.ts`**
- `RINGTONE_STORAGE_KEY`, `DEFAULT_RINGTONE_ENABLED = true` and `coerceRingtoneEnabled`.

**`web/src/lib/ws-client.ts`**
- `recentCalls()` (open), the settings mapping, and `pushTest(kind?)`.

**`web/src/lib/call-shortcuts.ts` (new, pure) + `web/src/components/app.tsx`**
- The M11 chords, decided by a pure function (§ 7.9).

**`web/src/components/call-bar.tsx`**
- `RingingCard` gets Silence, quiet/silenced subtitles, `data-ringing`, the aria labels (§ 7.1),
  `motion-safe:animate-pulse`, and 44 px targets on a coarse pointer (`useCoarsePointer`, `platform.ts:115`).
- No focus move.

**`web/src/components/incoming-call-banner.tsx`**
- The dedupe compares `liveCall.conversation_id ?? liveCall.alert_thread_id`. It is display only and opens
  nothing.

**`web/src/components/settings-pane.tsx`**
- `NotificationSettings` (`:916`): the Calls card.
- `SoundsSettings` (`:1108`): the Ringtone sub-switch.

**`web/src/components/notifications-bell.tsx` + `call-history-list.tsx` (new)**
- The Calls tab (§ 7.7).
- Rows are a SPLIT of two `DropdownMenuItem`s (main and call), never a nested button.

**`web/src/components/system-event-line.tsx`, `call-event-line.tsx`, `message-pane.tsx`**
- `SystemEventLine` takes an optional context `{conversation, isNewestCallLine, hasLaterEnd}` from
  `message-pane` only. `conversation-chat-panel.tsx:194` passes none, so the stage's chat column stays
  action-free.
- **Every call-line row gets a fixed height, `CALL_LINE_ROW_PX = 44`.** `estimateSize` returns it
  (+ `TIME_MARK_ROW_PX` when marked) for call-event rows, as it does for chess rows.
- The action sits inline in the pill with ink no taller than the avatar stack, and its target is grown to
  the row's 44 px.
- Moving the action from one line to the next therefore changes no row height.

**`web/public/sw.js` + `web/src/lib/service-worker.test.ts` (new)**
- `handlePush` branches on `kind`: `call`, `ring_over`, `missed_call`, and anything else (unchanged).
- The test runs `sw.js` in a node `vm` with a fake `self`.

### Mock, specs and scripts

**`web/mock/server.ts`**
- `MockCall` gains `quiet`, `alert_thread_id` and `remote_progress`.
- The `call_invite` hook gains:
  - `quiet: "dnd" | "pending"`;
  - `expire: true`, which ends the ring as `CallEndReasonUnanswered`;
  - mirroring the backend's BUSY rule: an invite while a call is live is dropped and answered
    `{ok: false, busy: true}`.
- `call_place` broadcasts `remote_progress: "ringing"` at `MOCK_CALL_ANSWER_MS / 2`.
- `/__test/emit` live messages accept a `system_event` with `call_event_type`. The mock mirrors
  `ring_ended_by_line` and marks `missed_call_alert` exactly as the backend does. **Its switch defaults
  to the BACKEND's default (off: the dry-run journal line only)**, because a mock whose default is the
  opposite of the backend's lets a page that only works with the switch on pass every spec, which is
  the "a mock that accepts what the backend refuses hides the bug" rule. The specs that test the
  ending turn it on per test through the `call_invite` hook (`ring_ends_on_line: true`) and reset it
  with `reset: true`; the default-off case is asserted too (the line arrives and the ring stays).
- `recent_calls` answers from stored system events.
- Settings echo the two keys.
- `push_test` accepts `kind: "call"`.
- `reset: true` clears all of it.

**`web/e2e/ringing.spec.ts`, `call-history.spec.ts` (new), `helpers.ts`**
- `emitCallInvite(page, conv, {quiet, expire})` and `emitCallLine(page, conv, type, {is_self})`.

**`web/scripts/preview.ts`**
- `--ring` (§ 8).

**`web/scripts/call-live.ts`**
- The digest gains `pathOrder` (frame path tails in arrival order, consecutive repeats collapsed),
  `remoteProgress`, `forwardingDestination` and `acceptedByPrefix`.
- A `--from-line` mode presses the newest call line's Call back in `AUTHORIZED_CALL_CONVERSATION`, after
  re-reading the button's `data-conversation-id` and the composer's.
- Same rails, no new target, already allowlisted (`guard-live-automation.sh:516`).

## 6. Gating and consent

**No new `OUTWARD_METHODS` or `MACHINE_METHODS` entry.** So `every_gated_method_is_named_in_the_automation_guard`
(`server.rs:17954`) and the guard alternation (`guard-live-automation.sh:981`) are unchanged.

**Changed, and already gated:**
- `set_settings` (MACHINE) gains `call_ring_push` and `missed_call_alerts`. They decide which devices this
  machine buzzes, which is why `set_settings` is token-gated.
- `push_test` (MACHINE) gains `kind: "call"`. It is the user's own press, and its copy says "your devices"
  because it reaches every subscription.

**New open read:** `recent_calls`. It is store only and answered read-only, like `notifications`.

**Backend acts that need no RPC, and why each is allowed:**

| Act | Why it is allowed |
| --- | --- |
| The own-presence read | An open read from a cached token. It publishes nothing. |
| Ending a ring locally (§ 4.3, § 4.4) | It sends nothing. The reject link stays the user's Decline (the argument at `server.rs:15160-15166`). It never runs while an Answer is in flight, so it cannot cause `hang_up_orphan` to post. The Teams-line half is behind an env switch and dry-run first. |
| The `call_ring_handled` record | Local, written after the user's own press. A read-only backend never answers or declines, so it never writes one. |
| The ring, ring-over and missed-call pushes | They reach ONLY the user's own subscribed devices and post nothing to Teams. `deliver_push` refuses read-only before the network. Calling never registers read-only. Both settings are OFF until the user turns them on: widening the delivery policy is "a product decision" (§ Push notifications). Each is claimed once across both backends. |
| The push payload | Stays RFC 8291-encrypted and carries a name only. |

**Purely local:**
- the ringtone, the ringback, Silence (it posts no `call_hangup`), the title flash, and the missed-call
  seen watermark;
- the hidden-page popup. Its permission is granted only from the user's own press (`notify.ts:15-19`).

**Outward, and the user's own click:**
- Call back (`call_prepare` + `call_place`) and Join (`call_join`);
- the chords (`call_accept`, `call_hangup`, `call_mute`), with the limits in § 7.9.

**Automation:**
- `call-live.ts` gains a mode and digest fields, but no target.
- The two recons are READ-ONLY examples that go through crate GET functions protected by verb scans. They
  are not shaped to avoid the guard: an example spelling the messages endpoint is read as a send
  (`guard-live-automation.sh:792-801`), and the GET-only module is what makes the read provably a read.
- Env switches (`TEAMS_LITE_RING_ENDS_ON_LINE`, `TEAMS_LITE_PROVISIONAL_ANSWERS`) are set in the user's
  own environment. Restarting the send-capable backend with them is the user's step.

**Rejected outward writes:** `markMissedCallsAsRead`, `setCallHistoryConsumptionHorizon`, the call
forwarding mutations, and redirect-to-voicemail.

## 7. UI and copy (including mobile)

**7.1 Ringing card**

`call-bar.tsx`: full width on a phone, `w-80` bottom-right on desktop, above the composer.

- **Title:** the name, or "Unknown caller".
- **Subtitle:**
  - "Incoming call";
  - with DND: "Incoming call · Do not disturb";
  - after Silence: "Incoming call · Silenced here".
- **Buttons:**
  - [Answer].
  - [Silence], with `BellOffIcon`, aria-label and title "Silence — it keeps ringing on your other devices".
    Hidden when quiet or already silenced.
  - [Decline], with aria-label "Decline — {name} is told".
- **Accessibility:** `role=alert`, aria-label "Incoming call from {name}". It does NOT take focus: a
  focused Answer would let a stray Enter open the microphone.
- **Mobile:** ≥ 44 px targets on a coarse pointer.
- **Motion:** the phone pulse is `motion-safe` only.
- **Sound:**
  - Plays while ringing, if sounds AND the ringtone are on. One tab per origin.
  - Stops at 35 s or on any ending.
  - Waits up to 2 s while `quiet` is `pending`.
- **Audio blocked:** if the browser has seen no gesture since load (right after an in-app update reload),
  `data-ringing="blocked"` raises the OS popup even on a visible page. Settings › Sounds states this
  limit.
- **Phone:** Android vibrates while ringing after a gesture. iOS relies on the card and the push.

**7.2 OS popup** (page open, tab hidden)

- "Incoming call" / "{name} is calling" or "{name} · {group}".
- One row (the shared tag), closed when the ring ends.
- Title flash: "Incoming call — {name}".

**7.3 Outgoing stage**

- "Calling…" becomes "Ringing…" on `remote_progress`, and "Forwarded…" when the call was forwarded.
- A soft local ringback plays only while "Ringing…" is shown (sounds on).

**7.4 Notices** (`lib/notice.ts`)

| Ending | Notice |
| --- | --- |
| RingMissed | "Missed call from {name}." |
| RingOver | "{name}'s call is over." |
| Unanswered (incoming) | "{name}'s call stopped ringing." |
| Unanswered (outgoing) | "{name} did not answer." (group: "Nobody answered.") |
| AnsweredOnThisMachine | "Answered in another teams-lite window." |
| DeclinedOnThisMachine | nothing (an ending the user caused says nothing) |

**7.5 Push** (phone, app closed)

- Ring: "Incoming call" / "{name} is calling". Delivered late: "{name} called" / "Tap to open the chat."
- Ring over: "{name} called" / "Tap to open the chat.", "Answered in teams-lite." or "You declined it."
- Missed: "Missed call" / "{name}" or "{name} · {group}".
- Test: "Incoming call" / "This is how a call rings on your devices."
- A tap opens `/c/<conversation>` on the ringing card, where Answer is the user's press.

**7.6 Settings**

The Notifications › Calls card is machine-wide, under the device switch. Intro: "Your devices with
notifications on can ring for calls while teams-lite is closed."

| Control | Copy |
| --- | --- |
| Switch "Ring for incoming calls" | "A call notification on every device above. Open it to answer here. Your Teams app may ring too." |
| Switch "Missed calls" | "A notification on your devices and in this window when a call nobody answered is missed." |
| Button "Send a test call notification" | → "Sent to {n} device(s)." |

With no device subscribed, both switches are disabled with "Turn on notifications on a device first."

Sounds › "Ringtone": "Rings in this window while a call is coming in. Off: the call still shows, silently.
A window you have not clicked since it loaded cannot ring; it shows a notification instead." It is
disabled with "Turn sounds on to hear the ringtone." while sounds are off.

**7.7 Bell › Calls tab**

The bell panel is 22rem, clamped to the window by the shared `DropdownMenuContent` rule.

- **Each row is TWO menu items side by side, separated by an inset rule** (the send pill's shape).
  - The main item: avatar, name, "Missed call" (red) / "Declined" / "Call ended · 12 min" / "Call
    started", and a relative time. Selecting it opens the conversation at the line
    (`requestScrollToMessage`).
  - On 1:1 rows, a 44 px call item with aria-label "Call {name}", disabled with `callUnavailableReason`.
- **Keyboard:** arrows walk both items. Enter on the main item never calls.
- **Badges:** the bell badge is `notificationsUnread + missedCallsUnseen`, and its aria-label names both.
  The Calls tab shows its own count. Opening the bell marks the FEEDS seen, as today. Missed calls are
  marked seen only when the Calls tab is shown, and the bell opens on Calls when only missed calls are
  unseen.
- **Empty state:** "No calls yet" / "Calls in your chats show up here."
- **Footnote:** "From the call notes Teams posts in your chats."

**7.8 Call line in a chat**

- The NEWEST call line of a 1:1 carries an inline "Call back" (it carries `data-conversation-id`).
- The newest "Call started" line of a `19:meeting_` thread with no later "ended" within
  `MEETING_ONGOING_GUESS_MS` (2 h, a guess) carries the app's own "Join here". It ships after `join-live
  --from-chat` passes.
- No group line carries an action.
- The stage's chat column carries none.
- This is the one Join vocabulary `meeting-awareness` reuses on its own lines.

**7.9 Keyboard chords** (M11)

| Chord | When it acts |
| --- | --- |
| `Ctrl/Cmd+Shift+S` Answer | While ringing only. Inert in editable fields (TipTap binds `Mod-Shift-s` to strike, `rich-editor.tsx:61`) and in modals (`aModalIsOpen`). |
| `Ctrl/Cmd+Shift+D` Decline | While ringing only. Same exclusions. |
| `Ctrl/Cmd+Shift+M` Mute | In a live call. MUTE acts anywhere, the composer included (the safe direction). UNMUTE by chord acts only outside editable fields and modals. |
| `Ctrl/Cmd+Shift+H` Hang up | Only while the stage is FULL (not folded), outside editable fields and modals. |

- Never single keys.
- `aria-keyshortcuts` and titles ("Answer (Ctrl+Shift+S)") on the buttons.
- Browser accelerators (Chrome on Linux/Windows: `Ctrl+Shift+M` profile, `Ctrl+Shift+D` bookmark all tabs;
  Firefox: `Ctrl+Shift+H` history) are **unmeasured**. An e2e proves which reach the page in Chromium, and
  a chord that does not is dropped rather than fought.

All copy is English.

## 8. Tests, mock and captures

**Rust unit**

- **push_policy**
  - ring/ring-over/test notifications: names, tags, `expires_ms`, late wording;
  - `ring_claim_keys`: service id, and two buckets without one;
  - `missed_call_alert` admits `callMissed` in a chat only. Refused: from_me, stale, deleted, a channel,
    an unknown/ended/started/meeting line, and a line within 120 s after a declined or answered record.
  - Payload stays < `MAX_PAYLOAD_BYTES` with 200-char names.
  - `a_system_line_never_notifies` stays green.
- **push.rs:** message byte identity; `30 ≤ CALL_RING_TTL_SECS ≤ CALL_RING_TIMEOUT`.
- **calling.rs:** the § 5 cases; `alert_key`.
- **teams_read.rs:** the `call_event_type` fixtures.
- **auth.rs:** `cached` never acquires.
- **server.rs**
  - `ring_ended_by_line`: incoming vs outgoing; another thread; an unknown type; a redial-shaped line
    composed before `started_at − 5 s`; **a frame this backend lost the insert race for still ends its
    ring**; an Answer in flight refuses; switch off → dry-run only.
  - `watch_ring_handled`: own port ignored; ids equal / ids absent + window; a record older than the
    ring.
  - `end_call_locally_for` refuses another id.
  - `the_ring_push_is_gated_and_claimed_before_delivery`: a bounded scan of `push_incoming_call` for
    `read_only()`, `push_subscriptions_exist`, `SETTING_CALL_RING_PUSH`, quiet, the Ringing re-check and
    `claim_once` before `deliver_push`. The window is bounded at a marker the body cannot contain, both
    ends are asserted, and it is proven by mutation.
  - `handle_incoming_call` awaits nothing between its insert and its emit (a source scan).
  - The progress branch does not return: a frame carrying `callProgress` and `callAcceptance` still
    reaches the acceptance branch.
  - Settings round-trip, both keys OFF by default.
  - `recent_calls` answers read-only.
  - M6b: switch off pins today's Connected-on-any-answer; switch on keeps Dialing, sends `pranswer`, does
    not acknowledge, and is not swept at 60 s.
- **store.rs:** `recent_call_events` filters (meeting markers, deleted rows, non-call lines), order,
  limit, and a renamed peer shown by the nickname; `one_to_one_thread_with` in both orders; `is_claimed`.
- **teams_calllogs:** the verb scan. **call_history_recon:** the enum-like printer filter.

**Web unit**

- `ringtone.test.ts`: recipe bounds; the full `ringDecision` table including `pending` expiry; the
  scheduler with a fake `AudioContext`; the election with a fake `navigator.locks` (focused steals, stolen
  re-queues without steal, a grant that is no longer wanted releases).
- `call.test.ts`, `call-history.test.ts`, `protocol.test.ts` (`shouldNotifyMissedCall` requires
  `missed_call_alert`).
- `service-worker.test.ts`: call, late call, ring_over (replace / keep missed / visible-close), missed
  (replaces a call row without renotify), message unchanged.
- `call-shortcuts.test.ts`.

**E2E** (`ringing.spec.ts`; every spec resets `call_invite {reset:true}`)

Visibility and focus are driven per page by an `addInitScript` override (headless pages all report
`visible`). `window.Notification` is stubbed to record construction and close.

1. An invite sets `data-ringing="sound"`. Answer, Decline, `call_end`, reset, a socket drop and an
   `expire` each clear it. RPCs are counted from `page.on('websocket')` frames.
2. A hidden page gets exactly ONE popup, closed at the ring's end.
3. Silence: no `call_hangup` frame; "Silenced here"; a second same-context page goes silent.
4. Two pages: exactly one rings; focusing the other transfers it; the stolen holder stops.
5. Sounds or ringtone off → `silent`, the card shows.
6. `quiet: "pending"` → no sound for ≤ 2 s, then sound; `quiet: "dnd"` → subtitle, no popup, no sound.
7. Outgoing: "Calling…" → "Ringing…" → connected.
8. A live `callMissed` line with `missed_call_alert` in a hidden conversation → one popup; none without the
   flag; none for `is_self`.
9. A ringing invite + a live missed line for its thread → the card goes, notice "Missed call from …".
10. Answer pressed while a missed line lands → the call connects (the accept-in-flight guard).
11. `expire` → "{name}'s call stopped ringing."
12. An invite while a call is live is refused (busy mirror).

`call-history.spec.ts`:
- The Calls tab lists the seeded lines (`seedCallEvents`, `mock/server.ts:1514`) and a live missed line.
- The counts show and clear.
- A row opens the thread at the line.
- Enter on a row sends no `call_prepare`.
- The call item and the line's Call back each send exactly one `call_prepare` + `call_place`.
- Two call lines' boxes are measured before and after a newer line arrives: equal heights.
- No action in the stage's chat column.

Gate: `history.spec.ts` and `calling.spec.ts` stay green. The latter is the regression gate for the
verified path against the mock, and judged against a baseline run on master.

**Preview:** `cd web && bun run preview -- --out /tmp/ring --ring --dpr 3` captures:
- the ring card in both themes, silenced, DND, and at a phone's width;
- outgoing "Ringing…" and the missed-call notice;
- the bell Calls tab in both themes, with one row cropped;
- the 1:1 call line with Call back;
- Settings › Notifications › Calls and Sounds › Ringtone.

The existing `--call` capture still runs.

## 9. Live verification plan

| Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- |
| M1 + M2: the open page rings and stops; the incoming answer still works | None receives a call, so this is the user's arrangement with the colleague who agreed to be called (the `AUTHORIZED_CALL_CONVERSATION` partner). App open in a hidden tab. | The colleague calls; the user answers once, then lets it ring once while the colleague hangs up. | (a) The ring is audible within ~1 s of `[calling] ringing: a call from …`, and the popup shows. (b) After Answer, **both people hear each other for ≥ 20 s**. (c) In the let-it-ring case, the dry-run line reports the missed line's `compose_time` relative to the ring, and the journal records whether the invite carried a thread and a `callId`. |
| M2 cross-install | Both installs (19420/19442) up (the user's own start); the same colleague | The colleague calls; the user answers on 19440 | 19442's journal: `ended … CallEndReasonAnsweredOnThisMachine` within ~1 s, which match applied, and whether the service ids were equal. The 19442 page stops ringing. |
| M2 switch on | `TEAMS_LITE_RING_ENDS_ON_LINE=1`, set by the user | The colleague rings and hangs up | `CallEndReasonRingMissed` within seconds of the line. Only then does the default flip. |
| M3 recon | `cargo run --example call_history_recon` (READ-ONLY) | The user, or the agent with their go-ahead | Recorded in NATIVE-CALLING.md: `48:calllogs` status and keys; `callEventType` values; whether a `callId` element exists; `from` semantics for missed and ended lines in both directions. |
| M4 DND | The user sets DND in their own Teams client; the colleague rings | The user and the colleague | "Incoming call · Do not disturb", no sound and no popup; the quiet decision in the journal. No invite at all is a measurement too. |
| M5/M6 | `cd web && bun run call-live -- --hold 20` (sanctioned, pinned, hangs up on every path) | The script; the colleague answers | `pathOrder` shows `/call/progress/` and where `/call/mediaAnswer/` falls relative to `/call/acceptance/`; the stage reads "Ringing…" before connected. M6b is built only if a provisional answer is seen, and proven by the same run with `TEAMS_LITE_PROVISIONAL_ANSWERS=1`, held past 60 s, with both people hearing each other. |
| M7 test push | Settings › "Send a test call notification" | The user | "Incoming call / This is how a call rings on your devices." on the phone; "Sent to 1 device." |
| M7 real ring push | The user turns the switch on; PWA closed; the colleague rings | The user and the colleague | One claim and "1 delivered" with both installs up; the tap opens the ringing card; time-to-Answer is recorded; when the ring ends, the row reads "{name} called". |
| M8 missed | "Missed calls" on; the colleague's unanswered call | The colleague; the user observes | One push per call, which replaces the ring row; the Calls tab count is 1. A call the user declined here produces no missed alert. |
| M9 Call back | `cd web && bun run call-live -- --from-line` | The script; the colleague | The same signaling as a menu-placed call; the script hangs up. |
| M9 Join on line | `cd web && bun run join-live -- --from-chat` passes first | The script | The thread join is verified, then the line's Join ships. |
| M12 voicemail | `cargo run --example voicemail_recon` (READ-ONLY) | The user, or the agent with their go-ahead | A count and attachment `contentType`, or zero (stays deferred). |

## 10. Milestones

Each milestone is shippable alone and listed in order.

1. **M1 — An open page rings.**
   - Scope: `ringtone.ts`, `syncRinger`, Silence, the popup, the RingingCard a11y, the Ringtone switch,
     the Unanswered labels, mock expire/busy, e2e and preview. Web plus the mock; no backend change.
   - Effort: M. Risk: low for the audio path; the UX risk is a ring that does not stop.
   - Deps: none. `incoming-and-conversation` › `ring-cancelled-or-answered-elsewhere` is the eventual full
     fix.
2. **M2 — The ring stops when this machine knows it is over.**
   - Scope: `alert_thread_id`, `service_call_id`, `end_call_locally_for`, the accept-in-flight guard, the
     handled record and its watcher (default on), and the Teams-line ending (env switch, dry-run).
   - Effort: M. Risk: low to medium (a false end loses an answerable ring; mitigated by id matching, the
     switch and the dry run).
   - Deps: `incoming-and-conversation` (its `callEnd` after attach-at-ring flows through
     `end_call_locally_for`), `plane-reliability` (the sweep).
3. **M3 — READ-ONLY recon of `48:calllogs` and the call lines.**
   - Effort: S. Risk: low. Deps: none.
   - Feeds M2's default flip, M8 and M10b.
4. **M4 — Do not disturb rings quietly.**
   - Scope: a spawned, cached-token presence read; `quiet` pending/dnd.
   - Effort: S. Risk: low.
   - Deps: `meeting-awareness` › `presence-in-call` (share one own-presence helper).
5. **M5 — Outgoing "Ringing…"** (`callProgress`, non-returning).
   - Effort: S. Risk: very low.
   - Deps: `plane-reliability` › `outgoing-ring-timeout-cancel` (consumes `remote_progress_at_ms`).
6. **M6 — Measure the provisional answer** (journal line and `pathOrder`).
   - Effort: S. Risk: none.
   - **M6b (contingent):** provisional answers as `pranswer` behind `TEAMS_LITE_PROVISIONAL_ANSWERS`, with
     the sweep rule. Effort: S–M. Risk: medium. Deps: `media-robustness-devices` and `video-send` (both
     touch `call-media.ts`'s answer application).
7. **M7 — Ring push and ring-over push** (setting off).
   - Effort: M. Risk: medium (a product widening, a possible double ring with native Teams, and a cold
     PWA start).
   - Deps: M2 (`ring_left`), M4 (quiet). `incoming-and-conversation` › `answered-elsewhere` improves the
     ring-over wording later.
8. **M8 — Missed-call alerts** (push and page, one setting, the page told).
   - Effort: S. Risk: low to medium. Deps: M3 (the `from` semantics), M2 (the handled record).
9. **M9 — Recents in the bell, and actions on call lines.**
   - Effort: M. Risk: low.
   - **This milestone OWNS the one Join vocabulary on call and meeting lines.** `meeting-awareness` M7
     and `meeting-participation` M10 consume it and land after it, which breaks the cycle this
     milestone and meeting-awareness M7 had (each named the other as its dependency).
   - Deps: a passing `join-live --from-chat` for the line's Join (README § 4 step 4, after
     incoming-and-conversation M0 and video-send M1). `meeting-participation` ›
     `join-ongoing-group-call` is not a dependency: group lines stay action-free until it lands, and
     it adds their actions.
10. **M10 — Adopt `48:calllogs`** — only if M3 justifies it; sized from the measured shape.
    - Effort: S–M. Risk: low (read-only).
11. **M11 — Calling keyboard chords and a11y.**
    - Effort: S. Risk: low to medium.
    - Deps: `meeting-participation` (a raise-hand chord is theirs), `video-send` (a camera chord is
      theirs).
12. **M12 — Voicemail recon.**
    - Effort: S. Risk: low. Deps: none. The READ-ONLY recon needs nothing from `advanced-call-control`,
      which defers transfer to voicemail and rejects redirecting a ring to voicemail (its § 3); if the
      recon ever justifies a voicemail feature, that feature is where the dependency belongs.

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A ring that never stops (answered elsewhere, caller cancelled, socket dropped). The service does not tell an endpoint that did not attach. | One state-driven `syncRinger`; every exit enumerated in e2e; a 35 s sound cap; stop on disconnect and pagehide; § 4.4 for this machine's other install; § 4.3 behind its switch; attach-at-ring (`incoming-and-conversation`) for the rest. |
| The Teams-line rule ends a NEW ring with an OLD call's line (redial). | Raw `call_event_type` only, the [−5 s, +5 s] window, an incoming Ringing call in the same alert thread, no Answer in flight, id-checked ending, behind an env switch with a dry run that measures the timing first. |
| The two backends race the insert. | The ring-ending runs on every received frame; only pushes stay under `inserted` plus a claim. |
| A fallback thread id changes the stage or publishes a read receipt. | `alert_thread_id` is alert-only; `conversation_id` is untouched; the stage reads only `conversation_id`. |
| The presence read stalls the calling frames or strands a broker window. | It runs after the emit, in a spawned task; a cached token only; only the HTTP request is timed out. |
| Autoplay blocks the ring after a reload. | `prime()` on the first gesture; `blocked` forces the OS popup; the limit is stated in Settings. |
| Several tabs ring, or two origins of one install, or two installs. | Web Locks per origin elect one ringer each. Two origins and two installs ring separately; that is stated, and § 4.4 stops the second install once handled. |
| A ring push outlives the ring on the lock screen. | The ring-over push, whatever the Missed-calls switch says; `expires_ms` at the ring length; a late wording. |
| Both backends push one ring. | `call-ring/<service_call_id>`, or two-bucket keys without one; pinned by a scan test. |
| A cold PWA start misses the ring window. | The push arrives in seconds; a failed Answer is reported; timing is measured in the M7 live step; answering from a cold phone is not promised. |
| A missed alert for a call the user declined here, or for their own outgoing call. | The handled-record window; `from_me`; M8 ships only after M3 measured `from` for both directions; off by default. |
| M6b breaks the verified outgoing path. | Contingent on a measurement; a runtime switch, off; the sweep rule and its test; proven past 60 s by `call-live` before any default change. |
| A Call back mis-tap in a scrolling menu rings a person. | A separate menu item rather than a nested button; Enter on the row never calls; 1:1 only; the newest line only. |
| A call line's action moves the history. | A fixed `CALL_LINE_ROW_PX` for every call line, known to `estimateSize`; a height-equality e2e; `history.spec.ts` in the gate. |
| A chord unmutes or hangs up by accident. | Chords only; unmute and hang-up excluded from editable fields and modals; hang-up only on the full stage; accelerators measured before shipping. |
| A recon prints personal data. | A defined, tested enum-like filter; no attachment `name`; user-run. |
| Shared mock state accumulates across the full E2E run. | Specs target their own conversations, reset every hook they arm, and are compared with a master baseline. |

## 12. Open questions and decisions the user must make

1. **Defaults.** Should "Ring for incoming calls" and "Missed calls" default ON for a machine with a
   subscribed device? Recommended yes after the M7/M8 live steps; they ship OFF.
2. Should DND also silence the missed-call alert, or only the ring? Proposed: the ring only, as message
   pushes ignore DND today.
3. Should Silence apply across every page of this backend (phone + laptop) through a backend flag, or stay
   per browser origin as proposed?
4. When should `TEAMS_LITE_RING_ENDS_ON_LINE` default on? Proposed: after the dry-run timing and the M3
   recon show the window is safe, and one live missed call ends correctly.
5. Does the service post `callProgress` to our progress link, and does a provisional answer precede the
   acceptance on a Teams→Teams 1:1? (M5/M6 `pathOrder`.)
6. Does a forked 1:1 invite carry `groupChat.threadId`, and do both installs receive the same `callId`?
   (M2 journal.)
7. Does a `callMissed` line carry any call id, and is `compose_time` the ring's start or its end? (M3,
   dry run.)
8. Is `48:calllogs` readable with the skypetoken, is it pushed live, and what does an entry carry? (M3.)
9. In DND, does Teams deliver invites at all, and does a manual DND show through while Always available
   registers? (M4.)
10. Does the tenant have Cloud Voicemail with UM-class items? (M12.)
11. On iOS, does a page opened from a notification tap get user activation? The design assumes not.
12. Teams' keyboard map is in `services-io-browser-keyboard-shortcut-lazy-chunk`, which was not
    downloaded. Fetch it to align the chords?

**Docs to update with the code:**

- **CLAUDE.md**
  - § Audio calls: ringing, Silence, DND, the two ring-ending paths and their switch, "Ringing…", labels,
    line actions.
  - § Push notifications: the ring, ring-over and missed widenings, settings, tags, claim keys, the late
    rule, and that the page is told `missed_call_alert`.
  - § Automation safety: `--ring`, the call-live mode and fields, the env switches.
  - § A call is a PAGE: the ringing card's states and the no-focus rule.
  - § Joining a meeting: a third join surface (the line).
  - § THE SOUNDS come from chess.com: the ringtone deliberately plays in a hidden document.
- **NATIVE-CALLING.md:** § 2.2 / § 8b (callProgress, frame order), § 8a (DND, `callId` across endpoints),
  and a new section for the recon.
- **Doc comments:**
  - `src/push_policy.rs` module doc;
  - `src/push.rs` TTL rationale;
  - `src/teams_presence.rs` header;
  - the `CALL_RING_TIMEOUT` doc (M6b);
  - `src/teams_activity.rs` `is_system_feed_thread` doc;
  - the `web/public/sw.js` header.

## 13. Review notes

**Accepted and changed** (each re-checked against the code and the bundle):

**Ending the ring**

- **`inserted` gating** (both reviews). Confirmed at `server.rs:15395-15407` and by the upsert's `WHERE`.
  The ring-ending now runs on every received frame, and the unit case is inverted.
- **The 30 s look-back and redials** (both). Confirmed: there is no call id in the stored line, and an
  unknown type becomes "ended" (`teams_read.rs:1187-1194`). The rule now requires the raw type and a
  [−5 s, +5 s] window, is id-checked, and sits behind an env switch with a dry run. M3 measures the call
  id and the stamping.
- **An Answer in flight → `hang_up_orphan` posts** (review 2). Confirmed (`server.rs:7860-7880`;
  `store.ts:3112-3117`). Added `accepting_since_ms`, set at `call_prepare {call_id}`, plus
  `end_call_locally_for`.
- **The other install keeps ringing after an answer** (review 1). Added § 4.4, default on, and argued
  why (§ 4.4).

**The thread fallback and the presence read**

- **The fallback `conversation_id` publishes a read receipt** (both). Confirmed (`call-stage.ts:294-301`).
  It is now `alert_thread_id`, alert-only.
- **The DND await stalls sequential frames and can strand the broker** (both). Confirmed
  (`server.rs:14506-14510`; `auth.rs:261-272`, `:612-635`). The read now runs after the emit, from a cached
  token only, with `quiet: "pending"` bounded on the page. Review 1 proposed timing out the token
  acquisition too; that half is **rejected** for review 2's reason (§ 4.2).

**Pushes**

- **The ring notification is never replaced, and its late branch is nearly dead** (both). Added the
  ring-over push (§ 4.8), with `expires_ms` at the ring length. The worker logic respects
  `userVisibleOnly`.
- **The page missed popup is not a mirror** (both). The decision is spelled once in Rust and the page is
  TOLD (`missed_call_alert`), under one setting renamed `missed_call_alerts`.
- **The claim key from `/conversationId` and the minute bucket** (both). Added `service_call_id` and the
  two-bucket claim.
- **Declined here, then a callMissed line** (review 2). Suppressed through the handled record; Recents
  says "Declined".

**Outgoing calls**

- **M4b's premises** (both). Every OUTGOING answer is provisional in the bundle, the acceptance carries the
  final one, and the sweep invariant breaks. M6b is now contingent, a runtime env switch, `pranswer`, no
  acknowledgement, and its own sweep rule and test. Today's possible timer-while-ringing defect is stated.
- **"Forwarded to voicemail"** (review 1). Confirmed: no literal. Dropped. While re-checking, the bundle
  shows voicemail is known from the acceptance (`VOICEMAIL_BOT_ID`), now journalled.
- **The progress branch must not return** (review 2). Done, with a test.

**UI and history**

- **`NOTIFICATION_TABS` typing, nested buttons and badge consistency** (both). Added `BellTab`, split menu
  items, and a stated badge rule.
- **Row height of the line action** (both). The suggested per-row constant alone still changes a row's
  height when the action moves to a newer line. The design uses a fixed `CALL_LINE_ROW_PX` for EVERY call
  line instead, plus an equality e2e.
- **Schema bump, `PINNED` and the mixed-build flip** (both). Confirmed (`store.rs:2026-2033`). There is no
  bump and no index: a `LIKE` scan through `msg_reader`.
- **Names in Recents** (review 1). Through `conversation_context` and the `nicknamed!` ladder, with a
  rename test.

**Recon, tests and docs**

- **`48:calllogs` evidence** (review 1). Confirmed as Copilot/transcript context. Reworded as weak evidence.
- **The guard framing and the voicemail `name`** (review 2). Reworded: the GET-only verb scan is the
  reason. `name` dropped. "Enum-like" defined.
- **Chord claims** (both). The "measured" claim is dropped. Unmute and hang-up are restricted, mute works
  in the composer, and accelerators are measured by an e2e first.
- **Web Locks** (both). Per-origin scope stated; re-queue on steal; re-check on grant; vibrate after a
  gesture; tests drive visibility explicitly.
- **The 4 h heuristic and "Call in progress"** (review 1). Recents never claims "in progress". The line's
  Join uses a stated 2 h guess and waits for `join-live --from-chat`.
- **The mock cannot exercise expiry or busy** (both). Added the `expire` and busy mirrors.
- **"M1–M3 do not touch the audio path"** (both). Now true by construction: insert-then-emit is unchanged
  and source-scanned. The live step adds "both hear each other ≥ 20 s".
- **Inconsistencies** (review 1): `data-ringing` everywhere; `ring_test_notification`; the popup dedupe
  via `alert_thread_id`. **Docs list** (review 2): the § Joining a meeting, `CALL_RING_TIMEOUT`,
  `teams_presence` header and chess-sound items added; test copy says "your devices".

**Rejected or narrowed:**

- **Review 2 asked to put the presence read and the fallback behind a runtime switch until a live
  incoming-answer step.** Narrowed instead. The fallback no longer reaches `conversation_id`. The presence
  read runs after the emit and changes only the page's sound and popup. So neither touches the verified
  path, and a switch would only delay a harmless feature. The live step still runs before anything
  defaults on.
- **Review 1 suggested a const switch for § 4.3.** An env switch is used instead, so the proof runs the
  shipped binary (review 2's point). The cross-install half (§ 4.4) is deliberately NOT switched off, for
  the reason given there.
- **Review 2 suggested making the mute chord mute-only.** Narrowed instead: MUTE acts everywhere, while
  UNMUTE and hang-up are restricted. Teams' own command is a toggle, and the unsafe direction is what is
  fenced.
