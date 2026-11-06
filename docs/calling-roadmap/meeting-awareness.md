# Meeting awareness: recording, roster, chat, lobby, server mute, presence

**Workstream:** `meeting-awareness` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

The call stage, the People panel and the call notices should state only what is true about the
meeting the user is in. This design covers six things. (a) Whether the meeting is being recorded,
transcribed or compliance-recorded; the app never claims that it is not. (b) Which chat belongs
to the meeting, however it was joined. (c) Who is in it: names through the rename ladder, roles,
mute, raised hands and the lobby, with nobody dropped silently. (d) Whether the meeting muted the
user's microphone. (e) Why the user is waiting, or why they were let go. (f) Two smaller items: a
"meeting has started" prompt fed by the chat-side call marker Teams already posts, and a measured
answer on whether colleagues see the user as "In a call".

Almost all of it is parsing of frames teams-lite already receives. **The design adds no new
outward RPC.** It changes nothing on the wire of the verified audio path: the join, accept and
media bodies, the capability masks and the callbacks published all stay as they are. Three
behaviour changes sit near that path:
- the lobby reading, which moves from a frame-wide string walk to this endpoint's own state;
- named endings for meetings only;
- local enforcement of a server mute. This one ships **behind a switch that is off by default**
  and is turned on only after a live measurement.

Every reading that is still a guess is measured first: through the sanctioned drivers, through
the user's own clicks, and through two new READ-ONLY recon examples.

## 2. Where we are today

All references below were re-checked against this worktree (HEAD `c941850`) and the
2026-11-05 bundle.

| Area | Today | Where |
| --- | --- | --- |
| Meeting chat for link joins | `call_prepare` sets `conversation_id: meeting.thread_id.clone()`. A `/meet/{code}` link has no thread, so the value stays `None` for the whole call. The comment says the thread is "then whatever the service says", but nothing reads the service's answer. | `src/bin/server.rs:7435-7445`; `src/calling.rs:1609-1633` |
| The join answer | Kept only for one journal line. Its links are merged, and `lobby_state_in_frame` is run over it. | `src/bin/server.rs:7766-7803` |
| Links | `Links::merge` only inserts: "a link the new frame does not mention stays valid". So the merged table can never lose `unmute`. | `src/calling.rs:446-452` |
| Lobby | `lobby_state_in_frame` walks every `callState`/`state`/`participantState`/`status` string in any frame. It is not scoped to this endpoint and matches client-side FSM strings (`ConnectedForRosterOnly`). The handle_call_frame branch turns an `Admitted` reading during `Connecting` into `Connected`. | `src/calling.rs:1906-1929`; `src/bin/server.rs:14746-14768` |
| Connected transition | The media answer sets `Connected` unless `in_lobby` is set ("THE answer is what makes a call a call"). | `src/bin/server.rs:14889-14910` |
| Roster | `RosterMember{mri, display_name, in_lobby, present, streams}`. It keeps only `8…`/`4…` mris, so apps (`28:`) are dropped. `in_lobby` is read from `state` strings, which on this tenant are only `active`/`inactive`. `present` is `!inactive`. Deltas are merged by mri with no version check. | `src/calling.rs:1989-2072`, `2152-2172` |
| Roster in call_state | `others`, `other_mris`, `waiting_in_lobby` (always 0 in practice) and `publishing` (this endpoint's own streams are filtered out, so the user's own server mute is never visible). Names are the roster's own `details.displayName` and do not go through the rename ladder. Nothing is bounded. | `src/bin/server.rs:2341-2405` |
| People panel | Blank names are dropped (`if (!name) return;`), which also makes the count wrong. Only the user's own row has a mute glyph. | `web/src/lib/call-stage.ts:222-249`; `web/src/components/call-stage.tsx:579` |
| Chat panel decision | Made ONCE, when the call's id first appears, and never re-derived. | `web/src/components/call-stage-context.tsx:54-74` |
| Endings | `CallEnded{call_id, code, phrase}`. `subCode` is dropped. Unknown phrases become "The call ended." | `src/calling.rs:570-604`; `web/src/lib/call.ts:341-376` |
| Join refusal | `post_signal` returns `anyhow!("calling: {status} from … — {body} [{headers}]")`. `call_join` runs `end_call_locally("CallEndReasonJoinFailed")` and then returns `Err`. The page's `joinMeeting` catch then reports `callFailureMessage(error)` under the same `CALL_NOTICE` id, which **replaces** "The meeting could not be joined." with the raw refusal text. | `src/calling.rs:2236-2271`; `src/bin/server.rs:7756-7764`; `web/src/lib/store.ts:2641`, `3077-3093` |
| Mute | `call_mute` flips local `muted`, then posts `{muteUnmute:{sender, muted, scope:"Myself"}}` to the merged `mute`/`unmute` link. That is not the client's self-mute (`updateEndpointState({state:{isMuted}})`). | `src/bin/server.rs:8423-8458`; `src/calling.rs:1559-1567` |
| confirmUnmute | Published as a callback, with no handler. | `src/calling.rs:128`, `1232` |
| Recording notices | `RichText/Media_CallRecording` with `RecordingStatus` `Initial`/`ChunkFinished` is mapped to `Pending` and dropped (`return None`). Only `Success` survives, as a video card. The test fixture's `Initial` is 5 s before its `Success` (16:01:39 vs 16:01:44), and its `sender_mri` is the organizer captured from a bare contacts URL. | `src/teams_read.rs:1318-1366`, `1780`, `4031-4060` |
| Meeting call marker | The JSON marker `{callId, meetingOrganizerId, …}` is parsed into `{event:"started", meeting:true}`. `call_event_json` deliberately returns `None` for it. | `src/teams_read.rs:1125-1136`, `3925-3940`; `src/bin/server.rs:12185-12205` |
| MeetingStart (evt 128) | Forwarded raw on `call_signal` only. | `NATIVE-CALLING.md:614-618` |
| Meeting activity line | The Join is an external `<a target=_blank>`. Its docstring still says "teams-lite has no calling stack". | `web/src/components/meeting-event-line.tsx:32-75` |
| Mock | Refuses a `/meet/` link that the backend accepts. The roster is three names. | `web/mock/server.ts:10235`, `4698` |
| join-live digest | `SIGNAL_PROBE` keeps only `call_signal` frames and responses containing `"links"`, so the join ANSWER never reaches the page. The call_join answer carries only link names. | `web/scripts/join-live.ts:231-248`; `src/bin/server.rs:7803` |
| Presence | The only write is the Always-available endpoint registration (Available/Available, the only value the service accepts), refreshed every 120 s, during calls too. | `src/teams_presence.rs:94-98`, `190-197`; `src/bin/server.rs:749`, `12421` |
| Stale comment | "Calling is off until the user turns it on". | `web/src/lib/call.ts:11-13` |

## 3. Gaps covered, deferred and rejected

**Covered**
- `meeting-chat-for-link-joins`: the thread is learned. The chat **tab** appears; the panel stays closed (§ 7).
- `lobby-being-held-and-meeting-outcomes`: scoped lobby, named meeting endings, a Rejoin limited to a lobby timeout.
- `external-tenant-meetings`: refusal mapping, an External badge when both tenant ids are known. No join-payload change.
- `roster-fidelity`: client Delta semantics, all presence modalities, kinds, roles, mute, hands, lobby, the rename ladder, bounded JSON.
- `server-mute-and-attendee-restrictions`: display by default; enforcement behind a switch. The approve/reject half is rejected (below).
- `recording-transcription-indicator`: compliance (hedged, sticky) and cloud recording (gated on M0).
- `meeting-start-reminders` and `meeting-start-reminders-and-join-entrypoints` (merged): the chat-marker prompt, and "Join here" on meeting activity lines.
- `presence-in-call`: measured, then a decision.

**Deferred or rejected**

| Item | Decision | Why |
| --- | --- | --- |
| `confirmUnmute` approve/reject | REJECTED here | It is an ORGANIZER-side request. `handleUnmuteConfirmRequest` files `approveUnmute`/`rejectUnmute` under `a.from.id`, the participant asking. The current client's callback is empty: `onUnmuteRequested:n=>{}` (calling-pluginless.js). Approving it would open somebody else's microphone, which belongs to `meeting-participation` › organizer-controls. This workstream logs one shape-only line. |
| `consentToRecording` / `denyConsentToRecording` publish | DEFERRED | Outward `publishState`. No explicit-consent policy has been measured. The M0 digest records `selfParticipant.enforceConsentToJoin`. Building it would need an OUTWARD_METHODS entry, a guard name and a press. |
| `externalRecording` banner | DEFERRED | It is a GraphQL participant field in `external-recording-banner.js`, and `calling-pluginless.js` never names it. Build it only if M0 finds a propertyBag key. |
| Paged roster (`getAllParticipants`/`searchParticipants`) | DEFERRED | It needs POSTs and is low value. The JSON is bounded and the omission is stated (§ 4.5). |
| Clearing the `serverMuteUnmute` capability bit | REJECTED | 73463 was captured off the real client. The rule "Revisit them together with the join payload, never alone" is in `src/calling.rs:35-42` (the round-1 design wrongly attributed it to CLAUDE.md). |
| Publishing "In a call" presence | REJECTED as a write | The endpoint accepts Available/Available only, `forceavailability` is banned and scanned for, and no calling chunk holds a presence publish (`Ca.Xb.InACall` is an app-state enum in calling-app.js). M7 measures; the user decides. |
| Web Push for "meeting started" | DEFERRED | Widening `push_policy` is a product decision (§ Push notifications). |
| Calendar-driven local reminders | DEFERRED | Outlook already reminds on every device. The calendar is read-only, and no dismiss or snooze will ever exist. |
| Rejoin after `Reconnected` | DEFERRED to `media-robustness-devices` › `call-recovery-after-drop-or-reload` | The calling registration just moved, so a rejoin races it. Recovery is that workstream's design. |
| Auto-opening the chat panel for a link join | OPEN QUESTION (§ 12) | It would publish a read receipt with nobody having asked. |

## 4. Protocol

Status vocabulary: **measured** (seen on this tenant), **read-from-bundle** (2026-11-05 client
code), **inferred** (neither).

### 4.1 Learn the meeting's chat thread (and its subject, and the conversation command links)

- **Method:** none outward. Parse the join answer (`joined.raw`) and every
  `/conversation/conversationUpdate/` callback (`calling::paths::CONVERSATION_UPDATE`, already
  published).
- **Body:** the frame body IS the conversation object:

```text
{ conversationController, sequenceNumber?, subject?,
  links: { leave, mute, unmute, admit, admitAll, updateEndpointState, publishState, … },
  state: { conversationType, isMultiParty, isHostless, isMeetingActivated },
  activeModalities: { call: {…}|null,
                      groupChat: { threadId: "19:meeting_…@thread.v2", messageId: "0",
                                   groupId?, backroomThreadId?, streamThreadId?,
                                   lobbyThreadId?, groupCallMessageId? } },
  complianceRecordingContent?, selfParticipant?, subscriptionDetails?: { selfParticipant? },
  roster? }
```

- **Rules:**
  1. **Meetings only.** These fields are learned only when `call.kind == Meeting`. An incoming 1:1 whose
     invite carried no thread must not gain a chat, a history GET or a recording destination
     mid-call (review fix).
  2. `activeModalities.groupChat.threadId` is accepted only when it starts with `19:`. It fills
     `conversation_id` only when that is `None`. If it disagrees with the address joined, journal the
     mismatch and keep ours. `subject` only replaces the `"Meeting"` fallback title.
  3. **Sequence gate, mirroring the client.** Let `n = sequenceNumber`, or `-1` when it is absent. The join
     answer is always applied. A later frame with `n` below the last applied value is ignored for
     the fields of this section only (journal one line). The existing `Links::merge`, the
     acknowledgement and every other branch of the same frame are untouched.
  4. **Conversation command links are REPLACED, not merged.** A new `conversation_links:
     calling::Links` is built from the TOP-LEVEL `links` object of each conversation object that
     passes the gate, and it replaces the previous set wholesale. That is how the client makes a
     withdrawn `unmute` take effect. `CallSession.links` (the merged table everything POSTs from)
     is left exactly as it is. `conversation_links` is read only by § 4.6.
     **As implemented** by incoming-and-conversation M3 (whoever landed first built it):
     `calling::ConversationLeg` — `from_response`, the sequence-gated `apply`, and the `unmute()`
     accessor § 4.6 reads — kept on the call as `CallSession.conversation.leg` and learned for
     every call kind off the place and join answers and every `conversationUpdate` frame. This
     workstream's "meetings only" rule keeps applying to the chat-thread and history fields.
- **Bundle evidence (calling-pluginless.js):**

```text
handleConversationUpdate(e){const t=e.body,i=gi(e);…this.processConversationServiceResponse(t,i,2)}
… a=e.sequenceNumber>=0?e.sequenceNumber:-1;if(s&&a<this.updateSequenceNumber){r.info(`seqNum=${a} is less than latest seqNum=…`);return}
… this.links[I.LINKS.MUTE]=e.links.mute||null,this.links[I.LINKS.UNMUTE]=e.links.unmute||null,this.links[I.LINKS.ADMIT]=e.links.admit||null,…
updateGroupChatIds(e,t){…this.groupId=e.groupId,this.threadId=e.threadId,this.teamsMessageId=e.messageId,this.backroomThreadId=e.backroomThreadId,this.streamThreadId=e.streamThreadId,this.lobbyThreadId=e.lobbyThreadId,this.groupCallMessageId=e.groupCallMessageId
```

- **Status:** `groupChat` in an update is read-from-bundle. The `activeModalities` container is
  measured: `call` is null in the answer and an object one second later (NATIVE-CALLING.md:562-565).
  The ~37 answer links, including `unmute`, are measured (NATIVE-CALLING.md:224-228). Whether
  `groupChat` arrives on this tenant is verified by M1's journal line.

### 4.2 Know that THIS endpoint is held in the lobby

- **Method:** none outward. Parse `/call/acceptance/` (already acknowledged), `/conversation/rosterUpdate/`,
  and `selfParticipant` / `subscriptionDetails.selfParticipant` in conversation objects.
- **Signals, in order:**
  - (a) `callAcceptance.controllerName == "lobby"` means Waiting.
  - (b) Our own endpoint (`participants[<our mri>].endpoints[<our endpointId>]`) carries `lobby` and no
    `call`: Waiting. It carries `call`: Admitted. The `call` modality wins, which is the client's
    `a&&(c=!1)`.
  - (c) The same test on `selfParticipant`.

  A lobby endpoint on ANOTHER participant never moves our state.
- **Not signals:** `ConnectedForRosterOnly` and `Connected`. These are the client's own FSM state, set
  after its own subscribe-only POST. The legacy walk stays only as a **journal-only** fallback
  (`[calling] legacy lobby string seen: <key>`) until M2's live lobby run. Then it is deleted.
- **Phase rule (review fix).** `Admitted` only clears `in_lobby`. `Connected` is set from it only on a
  real `Waiting → Admitted` transition, and only if a media answer has already been applied while
  waiting (new `media_answered: bool`). Otherwise the media answer sets `Connected` as it does today.
  On the organizer path, a roster frame showing our `call` endpoint before the media answer therefore
  leaves the phase at `Connecting`.
- **Bundle evidence:**

```text
processCallAcceptance(e,t){…this.applyLobbyStateIfNeeded(t,"processCallAcceptance",e.callAcceptance.controllerName)
applyLobbyStateIfNeeded(e,t,i){if(i!==I.MISC.LOBBY_CALL_CONTROLLER)return!1;…"Controller name is lobby so putting user in lobby"
MISC:{CONVERSATION:"conversation",LOBBY_CALL_CONTROLLER:"lobby",LOBBY_PARTICIPANT_STATE:"lobby"}
static isInLobby(t,i){for(const r of t)if(r.endpointId===i&&r.isLobby)return!0;return!1}
e.subscriptionDetails?.selfParticipant){const n=Za.fromRoster(e.subscriptionDetails.selfParticipant,!0);…updateLocalParticipantFromSubscribe
```

- **Status:** read-from-bundle. teams-lite has only joined meetings it organized (`lobby=false`,
  NATIVE-CALLING.md:520). The real client's own lobby capture (NATIVE-CALLING.md:338-343) recorded
  no frame shapes.

### 4.3 Say why a MEETING ended

- **Method:** none outward. Parse `/call/end/` and `/conversation/conversationEnd/` (already published).
- **Body:** `{callEnd|conversationEnd:{code, subCode, phrase, resultCategories?}}`, also under `transactionEnd`.
  `CallEnded` gains `sub_code`.
- **Mapping:** applied **only when `call.kind == Meeting`** (review fix). A 1:1 or group call keeps
  today's phrase-based reading, because no far-side-hangup subCode is measured, and NM maps `0/5000`
  for every call type.

  | code / subCode | Name (`calling::END_REASON_*`) | Sentence |
  | --- | --- | --- |
  | 0 / 5000, 0 / 5300 | `CallEndReasonRemovedOrEnded` (until the M2 measurement splits it) | "You are no longer in the meeting: someone removed you, or it was ended for everyone." |
  | 0 / 5854 | `CallEndReasonDeniedInLobby` | "You were not let into the meeting." |
  | 0 / 5855 | `CallEndReasonTimedOutInLobby` | "Nobody let you in before the lobby timed out." + Rejoin |
  | 403 / 5724 | `CallEndReasonPolicyB2b` | "A policy stops your account from joining this organization's meeting." |
  | 403 / 5723 | `CallEndReasonPolicyAnonymous` | "This meeting does not let guests join." |
  | 403 / 5810 | `CallEndReasonInformationBarrier` | "An information barrier policy stops you from joining this meeting." |
  | 403 / 5448 | `CallEndReasonPolicy` | "A Teams policy stops you from joining this meeting." |
  | 403 / 10005 | `CallEndReasonBlocked` | "The organizer's settings block your account from this meeting." |
  | 413 / 5807 | `CallEndReasonLobbyFull` | "The meeting's lobby is full." |
  | 413 / other | `CallEndReasonMeetingFull` | "The meeting is full." |
  | anything else | phrase, as today | "The call ended." (meeting: "The meeting ended.") — **no Rejoin** |

- **Why 5000/5300 is not "removed":** a participant whose organizer ended the meeting for everyone
  may receive the same code. `ConversationEndForAllInitiated` is a phrase the client SENDS. M2 measures
  what arrives (§ 9, step 4) before the sentence is split into "Someone removed you" and "The meeting
  was ended for everyone".
- **Bundle evidence:**

```text
function NM(e,t=null){if(e===qt.Success)switch(t){case Bi.RemovedFromConversation:case Bi.RemovedFromCall:return 44;case Bi.DeniedInLobby:return 55;case Bi.TimedOutInLobby:return 56;default:return 1}…e===qt.Forbidden?t===Bi.UserBlocked?64:t===Bi.PolicyRestriction?62:t===Bi.AnonymousJoinDisabledByPolicy?66:t===Bi.NoLobbyForBroadcastJoin?67:t===Bi.NotAllowedDueToInformationBarrier?68:…
Bi={…PolicyRestriction:5448,UserBlocked:10005,MaxLobbyParticipantLimitReached:5807,…RemovedFromConversation:5e3,DeniedInLobby:5854,RemovedFromCall:5300,TimedOutInLobby:5855,AnonymousJoinDisabledByPolicy:5723,B2bJoinDisabledByPolicy:5724,…NotAllowedDueToInformationBarrier:5810,…}
```

- **Status:** the mapping is read-from-bundle. That callEnd frames carry `subCode` is measured
  (5003/5013 on the unreachable 1:1, `src/calling.rs:629-643`). Which subcodes this tenant sends for a
  lobby, a removal or an end-for-all is not measured.

### 4.4 Explain a join the service refuses

- **Method:** the existing join POST (`call_join` → `calling::join_meeting` → `post_signal`). Nothing new
  is sent.
- **Change:** on a non-2xx answer, `post_signal` returns
  `anyhow::Error::new(SignalRefused{status, sub_code, phrase, text})`. It reads `code`/`subCode`/`phrase`
  from the JSON body at any depth (also `transactionEnd.*`) and from `x-microsoft-skype-*` headers. Its
  `Display` is **byte-for-byte today's text**, so every other caller and every existing message stays
  the same. `call_join` downcasts it, maps it through § 4.3's table (a meeting by construction), and
  ends the call locally with that name, or with `CallEndReasonJoinFailed` when nothing matched.
- **Page half (review fix):** `onCallState` records `lastEndReported = {callId, reason}` when it reports
  a NAMED ending. The `joinMeeting` catch skips `reportCall(callFailureMessage(error))` when
  `lastEndReported.callId === callId` and the reason is named. Ordering is safe: the backend emits the
  ended `call_state` before it returns `Err`, and both travel on one socket.
- **Bundle evidence:** the same NM table. The client feeds it from the HTTP error in the join's catch
  (`Xt(m,this.signalingAgentConfig)`).
- **Status:** inferred (that a policy refusal comes back on the join POST rather than as a callEnd);
  the mapping is read-from-bundle.

### 4.5 Roster fidelity

- **Method:** none outward. Parse `/conversation/rosterUpdate/` (body = roster), and `roster` inside
  conversation objects.
- **Measured shape** (NATIVE-CALLING.md:1048-1068):

```text
{ type:"Delta", sequenceNumber, participantCounts:{…},
  participants:{ "8:orgid:<oid>":{ details:{displayName,…}, state:"active"|"inactive",
      role, meetingRole, meetingRoles:[…], version, enforceConsentToJoin,
      endpoints:{ "<endpointId>":{ participantId, endpointState:{endpointStateSequenceNumber, state:{isMuted}},
          call:{ serverMuteVersion, mediaStreams:[{type,label,sourceId,direction,serverMuted,…}] } } } } } }
```

- **Read from the bundle, not yet measured:** `details.participantType`, `details.tenantId`,
  `publishedStates:[{stateType, content, stateId, typeRank}]`, endpoint keys `lobby`,
  `mappedParticipant`, `stream`, `streamLobby`, and `participantCounts`:
  `c$={totalParticipants:0,preheatedParticipants:0,lobbyParticipants:0,totalPresenters:0,…,overflowAttendees:0,totalOrganizers:0}`.
- **Rules (the client's Delta semantics; review fix):**
  1. **Staleness is per PARTICIPANT, not per frame.** Keep `participant_versions: HashMap<mri, i64>` and
     `tombstones: HashMap<mri, i64>`. A participant's update is applied only when its `version` is
     newer than both. An `active` update records the version and clears the tombstone. An `inactive`
     update removes the person and records a tombstone. A frame is dropped whole only when its
     `sequenceNumber` is below the last FULL (non-Delta) roster's. Non-Delta (`MultiPartyEndpoint`)
     frames keep a global sequence check, exactly as the client does. All of this state is reset per call.
  2. **`participantCounts` are adopted only from a frame whose `sequenceNumber` is newer** than the last
     one adopted.
  3. **Presence is any of five modalities**: `call`, `lobby`, `mappedParticipant`, `stream`,
     `streamLobby`. An `active` participant with none of them is SKIPPED: not added, not removed. Only
     `state:"inactive"` removes anybody.
  4. **Kinds:** Person (`8:`), Guest (`8:teamsvisitor` or `participantType` saying so), Phone (`4:`),
     App (`28:`). Apps are KEPT, but only in the new `roster` field. They are excluded from `others`,
     `other_mris`, `waiting_in_lobby`, the recorder's names and `publishing`, so an older page never
     draws a bot as a person.
  5. **A person is in the lobby** when some endpoint is `lobby` and none is `call`.
  6. **A person's audio is server-muted** when a non-mapped endpoint (`!mappedTo`) has an audio stream
     with `serverMuted:true`. This is the client's own test.
  7. **External** is shown only when our OWN roster entry's `details.tenantId` and theirs are both
     known and differ. `Session` has no tenant id (`src/teams.rs:20-30`).
  8. **Hands:** sort by `publishedStates[stateType=raiseHands].typeRank` and display the ordinal
     POSITION ("2nd"), never the raw value. That `raiseHandsOrderedRank` equals `typeRank` is not shown
     in any chunk.
  9. **Names:** resolved through `Store::display_name_for_mri` (`src/store.rs:4475`), OUTSIDE the
     calling lock (collect new mris, release, resolve, re-lock, apply). Cached per call. An entry older
     than 60 s is re-resolved at the next roster frame. That covers a rename made through the other
     backend sharing the store, which never emits `person_override_changed` here.
     `person_override_changed` on this backend clears that mri at once.
  10. **Counts:** `participant_count` = in-call rows held, self included, lobby and apps excluded.
      `participantCounts` is journalled beside it until M0 shows what `totalParticipants` counts. JSON
      lists (`roster`, `others`, `other_mris`) are bounded at `MAX_ROSTER_JSON = 200`, with
      `roster_omitted`. The held roster itself is not bounded: `publishing` and video subscriptions read
      it. (The round-1 claim of "a 1 000-member bound" referred to no code and is dropped.)
- **Bundle evidence:**

```text
handleDeltaRoster(e,t,i,r){…if(this.isRosterStaleFromFull(e,r)){…return}…for(const n of Object.keys(e.participants)){const s=e.participants[n];if(!this.isParticipantRosterStale(n,s.version)){let a=Za.fromRoster(s);s.state&&s.state==="active"?(…this.handleRosterParticipant(a,i,r),this.tombstoneSequence.has(n)&&this.tombstoneSequence.delete(n),this.latestRosterVersionForActiveParticipants.set(n,s.version)):s.state&&s.state==="inactive"?(…this.handleRemovedDeltaRosterParticipant(a,s.version,i,r)):…
if(i.sequenceNumber>this.lastRosterUpdateSeqNumber){this.participantManager.signalingSessionCallback.onParticipantCountsUpdated(i.participantCounts,s),…
static fromRoster(t,i=!1){…return!a&&!c&&!o&&!i&&!l?null:(a&&(c=!1),new pI({…role:t.role||"",tenantId:t.details.tenantId||"",…isLobby:c,meetingRole:t.meetingRole,…meetingRoles:t.meetingRoles,participantType:t.details.participantType,publishedStates:t.publishedStates||null,…
… t[o].lobby&&(u=!0,n=!0,…),…t[o].mappedParticipant&&(s=!0,…),…return a=a||!!t[o].streamLobby||!!t[o].stream,
static isParticipantServerMuted(t){for(const i of t)if(i.mediaStreams&&!i.mappedTo){for(const r of i.mediaStreams)if(r.type==="audio"&&r.serverMuted===!0)return!0}return!1}
handleRosterParticipant(e,t,i){if(!e){…"rosterParticipant processing skipped because null.");return}
```

- **Status:** the Delta, `version`, `state`, `role`, `meetingRole(s)`, `endpointState.isMuted`,
  `serverMuted` and `participantCounts` are measured. Everything else is read-from-bundle. Note that
  `isFullRoster` is a client-side argument (where the roster came from), and teams-lite has no
  equivalent: callback frames are treated as deltas, as they are today.

### 4.6 Server mute: show it (default), enforce it (behind a switch)

- **Inputs:**
  - `server_muted`: rule 6 of § 4.5, applied to OUR endpoint only. Another endpoint of the same mri does
    not count.
  - `can_unmute_self`: `!server_muted || conversation_links.unmute().is_some()`. The set is the REPLACED
    one from § 4.1, so a withdrawal is visible.
  - `attendee_audio_restricted`: true when a published state of type `attendeeAudioRestricted` is seen in
    a roster participant or a conversation object. Where it lives on the wire is unknown, and M0
    records the published-state types it sees.
- **"Locked" is drawn only on a frame signal:** `server_muted && (conversation_links has no unmute ||
  attendee_audio_restricted)`.
- **Display half (M5, default ON):** call_state gains `server_muted`, `can_unmute_self` and
  `unmute_locked`. The page shows "Muted by the organizer". **The mic glyph stays the LOCAL truth.** No
  track is touched and no RPC changes. A display that said "muted" while the track was live would invite
  the user to speak believing nobody hears.
- **Enforcement half (M6, the `server_mute = "enforce"` value of incoming-and-conversation's stored
  per-backend `call_protocol_set`, default `"display"`, published as `call_state.server_mute_enforced`).**
  It is NOT an environment variable (README § 3, collision 10; incoming's risk table rejects one for an
  unproven flow): the user sets it in Settings › This app, behind the write token, and the setting is
  per backend like the rest of `call_protocol_set`. The rules use the client's muteState names: 0
  unmuted, 1 muted because the server muted us, 2 self-muted, 3 asked the server to unmute.
  1. When `server_muted` turns true on the page that holds media, and the user was live (state 0), the
     page disables its own track. That is state 1.
  2. An Unmute press while server-muted calls the existing `call_mute(false)`. While server-muted, the
     backend posts only to `conversation_links.unmute()` and does **not** flip `call.muted`. With no
     such link it refuses before the network, with a sentence. The page records state 3 and does not
     re-enable the track itself. **That backend branch is incoming-and-conversation M9's, not a second
     one** (its § 4.9: the body `self_unmute_payload`, the client's `NV`, and the door): this design
     adds only the page's track rules 1, 3 and 4. M6 lands after incoming M9, so its POST is that body
     from the start.
  3. When a roster frame clears `server_muted`:
     - from state **3**, the user's own press, the track is re-enabled;
     - from state **1** it stays OFF, and the page shows "The organizer lets you unmute now". This is a
       deliberate divergence from the client, which auto-unmutes state 1. Opening the microphone to a
       meeting is outward, and here the trigger would be somebody else's action.
  4. **Escape hatch.** If an accepted unmute POST is followed by no clearing frame within 10 s, the page
     re-enables the track (the user asked to be heard), keeps the label, and journals it. A wrong
     reading therefore costs at most 10 s of silence and never a meeting.
- **Unmute body:** belongs to `incoming-and-conversation` M9 (`self-mute-not-visible-in-meetings`). The
  client's server unmute is `signalingSession.unmuteAsync` with NV `{from:{id,displayName,endpointId,participantId,languageId}, mediaTypes:["audio"]}`.
  Because M6 is ordered after that milestone, enforcement never posts any other body. If the service
  still refuses it, the page keeps saying muted, which is the true state.
- **Bundle evidence:**

```text
async handleServerMutedState(t){…if(!this.isServerMuted){const r=this._muteState===3;if(this._muteState===1||r)try{i.info("is unmuting, as we are no longer server muted")…}if(this.isServerMuted&&this._muteState===0){i.info("is muting, as we are unmuted");…this.setMuted(1,t)
async muteUnmute(t,i){…const n=!t&&(!this.isServerMuted||this._callInLobby);…if(t)this.setMuted(2,i),…else if(n)…this.setMuted(0,i),this.updateEndpointStateForMute(!1,i);else return r.info("is unmuting on server"),this._muteState===2&&this.setMuted(3,i),…this.signalingSession.unmuteAsync(i)
updateCapabilities(t,i=!1){…this.isServerMuted&&!this._callInLobby?this._commandUrlPresence&Dm.Unmute&&(r=!0):this.isMuted&&(r=!0)
getCommandUrlPresence(){let e=0;return this.links[I.LINKS.MUTE]&&(e|=1),this.links[I.LINKS.UNMUTE]&&(e|=2),e}
function YP(e){switch(e){case"raiseHands":…case"attendeeModalitiesUnrestricted":…case"attendeeAudioRestricted":…
```

- **Status:** the behaviour is read-from-bundle. The `serverMuted` field is measured (as false). Being
  server-muted has never been measured.

### 4.7 Compliance-recording indicator

- **Input:** `complianceRecordingContent` on a conversation object (the join answer or a
  conversationUpdate).
- **Rule:** present and non-empty sets `compliance_len = Some(len)`. It is **sticky for the call**: the
  client only ever sets it, and never clears it. Neither the content nor any part of it is drawn or
  logged; the journal gets the length, which is what the client's own telemetry measures.
- **Copy is hedged:** "This meeting may be recorded for compliance". The pluginless and shell chunks use
  the field for telemetry only (`setComplianceRecordingContentLength`). The indicator's
  `isComplianceRecording` comes from module 733684, which is in no downloaded chunk.
- **Bundle evidence:** `e.complianceRecordingContent&&(this.complianceRecordingContent=e.complianceRecordingContent)`.
  calling-app.js: `…complianceRecordingTargetNames:We,isComplianceRecording:Ne,…}=(0,G.Go)(D)…return we||Ne?(0,i.Y)(O.S,{…}):null`.
- **Status:** inferred (that presence means active). Fixture-only on this tenant; the inference is
  stated in the docs.

### 4.8 Cloud recording and transcription indicator

- **Sources:**
  - (a) Live: the messaging trouter's `/messaging` EventMessage (NewMessage or MessageUpdate) in the
    CALL'S OWN thread. It is ephemeral, the pattern of `typing_from_event`.
  - (b) Late join: one GET of the thread's newest page when the thread becomes known, through a new
    `teams_read::fetch_messages_page_raw`. The ordinary page parse drops `Pending` notices
    (`src/teams_read.rs:1780`), so the raw items are needed.

```text
GET {chatService}/v1/users/ME/conversations/{thread}/messages?pageSize=50&view=msnp24Equivalent   (measured endpoint, src/teams_read.rs:442-470)
messagetype: RichText/Media_CallRecording
content: <URIObject type="Video.2/CallRecording.1">…<RecordingStatus status="Initial|ChunkFinished|Success" code="…"/>…</URIObject>
```

- **Rules (gated on M0; review fixes):**
  1. A recording counts as **ongoing** only when an `Initial` (or `ChunkFinished`) notice has NO later
     status for the same recording. Recordings are paired by the URIObject's own identifier when M0
     finds one, or else by compose order within the thread. The late-join check also requires the
     notice to be newer than the latest meeting call-start marker.
  2. Once ON, the strip stays on until the call ends, unless M0 or M4 measures a real stop signal.
  3. **Attribution only from a measured initiator field.** The notice's sender, and the organizer, are
     never used. The fixture's `sender_mri` is the organizer from a bare contacts URL.
  4. Transcription: `RichText/Media_CallTranscript` is a candidate and unconfirmed. M0's messagetype
     histogram decides.
- **Bundle evidence:** the live source (module 733684) is in no chunk downloaded. calling.js: `title:"Recording is being saved",text:"Once the recording is processed and saved in Microsoft Stream, it will be added to the meeting chat."`
  and `alertType:"TranscriptStartedByOthers"…title:"Transcribing has started"`.
- **Status:** inferred. The fixture has `Initial` 5 s before `Success`, which suggests `Initial` may
  mark processing at the END. **The "recording" half does not ship before M0 shows `Initial` arrives at
  START.**

### 4.9 "Meeting has started" prompt

- **Source:** the live chat-side meeting call marker (`teams_read::parse_call_event` gives
  `{event:"started", meeting:true}`). The parser gains `meetingOrganizerId` extraction.
- **Event:** local `meeting_started` (backend → page) `{conversation_id, organizer_mri, at_ms}`.
- **Emitted only if all of these hold:**
  - `calling_available()` (never on a read-only backend);
  - the marker came off the LIVE feed;
  - `compose_time` is within 10 min;
  - the marker's sender is not self (`is_self`, `src/bin/server.rs:11705`);
  - this machine is not in a call on that thread;
  - no meeting call is in `Connecting` or the lobby with a still-unknown `conversation_id` (the user's
    own link join posts this very marker);
  - the thread was not prompted in the last 30 min;
  - a 5 s grace delay has passed, after which everything above is re-checked.

  The page drops it for a muted chat (`chatIsMuted`).
- **Plane readiness is NOT required.** The Join button already states its own not-ready reason, and the
  card stays 10 min, so a transient reconnect does not eat the prompt.
- **evt 128** `notificationType:"MeetingStart"`: only top-level keys are journalled. Its parser is built
  only once M0 or M7 has captured one (bundle: `TP={…,MEETING_STARTED_NOTIFICATION:128,…}`).
- **Join:** the existing `MeetingJoinButton` → `call_prepare` (MACHINE) + `call_join` (OUTWARD).
- **Status:** the marker shape is measured (`src/teams_read.rs:3925-3940`). Its sender semantics are
  unmeasured (M7 records `sender_is_self`).

### 4.10 Rejoin after a lobby timeout

- **Offered only for** `kind == "meeting"` and `CallEndReasonTimedOutInLobby`. Not offered after a denial,
  a removal, a policy refusal or the unexplained default: joining a meeting that has ended starts a new
  call in it and notifies its attendees.
- **Address:** carried by the BACKEND in `call_state` (`rejoin_address`, set at `call_prepare` from
  `meeting_address`, `src/bin/server.rs:10149`), so every page agrees and nothing is replayed from page
  memory.
- **Method:** the existing `call_prepare` + `call_join`, from one press on the notice.
- **Cost, stated in the doc and the button title:** "puts you back in the lobby; the people who can admit
  you are told you are waiting".
- **Status:** the join is measured (2026-08-05, 2026-09-04).

### 4.11 Measure "In a call" presence

- **Method:** READ. `teams_presence::fetch_presence(http, session, profile_token, &[self_mri])`, which does
  `POST {unifiedPresence}/v1/presence/getpresence/`. It is a POST that reads (`src/teams_presence.rs:142-160`).
- **Body:** `[{"mri":"<self mri>"}]`, answered with `{availability, activity, lastActiveTime}`.
- **Status:** the endpoint is measured; the in-call outcome is unmeasured.

## 5. Changes by file

**`src/calling.rs`**
- `GroupChat{thread_id, message_id, lobby_thread_id}`, `group_chat_in_conversation(&Value)`,
  `subject_in_conversation`, `compliance_len_in_conversation(&Value)->Option<usize>`,
  `conversation_sequence(&Value)->i64` (-1 when absent), and `conversation_links_in(&Value)->Links`
  (TOP-LEVEL `links` only). The readers take `/_decoded/conversationUpdate/…`, `/conversationUpdate/…`
  and the bare body, the roster parser's tolerance.
- `own_lobby_state(frame, &LocalParticipant)->Option<LobbyState>` (§ 4.2). `lobby_state_in_frame` is
  renamed `legacy_lobby_string_in_frame`, used only for the journal, and deleted after M2.
- `RosterMember` gains `role`, `meeting_roles`, `kind: MemberKind`, `tenant_id`, `version`,
  `endpoints: Vec<RosterEndpoint{id, in_call, in_lobby, mapped, stream, is_muted: Option<bool>,
  server_muted_audio}>`, and `hand_rank: Option<i64>`. `roster_in_frame` stops filtering `28:`.
- `RosterState{participant_versions, tombstones, last_full_seq, last_counts_seq, counts}`.
  `apply_roster_update(&mut Vec<RosterMember>, &mut RosterState, RosterUpdate)` implements § 4.5 rules 1-3.
- `CallEnded.sub_code`, the `END_REASON_*` consts, and `named_meeting_end_reason(&CallEnded)`.
- `SignalRefused` (Display identical to today) and `refusal_end_reason`.
- `meeting_start_shape(&Value)->Vec<String>` (keys only).
- Tests:
  - `group_chat_*`; `conversation_links_replace_rather_than_merge` (a frame without `unmute` withdraws it);
  - `a_lower_conversation_sequence_is_ignored_for_these_fields_only`;
  - `own_lobby_*` (acceptance `lobby`; our endpoint `lobby` then `call`; another person's lobby never
    moves ours; selfParticipant; legacy strings ignored);
  - `an_out_of_order_delta_about_another_person_is_applied`, `an_older_version_of_one_person_is_ignored`,
    `an_inactive_person_is_tombstoned`;
  - `mapped_and_stream_endpoints_are_present`, `an_active_person_with_no_modality_is_skipped`,
    `apps_are_kept_with_kind_app`;
  - `meeting_end_names_cover_the_table`, `a_one_to_one_ending_is_never_named_removed`;
  - `refusal_end_reason_reads_body_and_headers`;
  - and the page-spelling pin `the_page_knows_the_name_of_an_unreachable_ending` extended to every new
    name, so `web/src/lib/call.ts` spells each.

**`src/bin/server.rs`**
- `CallSession` (`:2229`) gains `conversation_links`, `last_conversation_seq: i64`, `roster_state`,
  `names: HashMap<String,(String, Instant)>`, `media_answered`, `server_muted`,
  `attendee_audio_restricted`, `recording: MeetingRecording{compliance_len, recording: Option<Seen>,
  transcribing: Option<Seen>, observable}`, `rejoin_address: Option<Value>` and `own_tenant: Option<String>`.
- `others()` excludes `MemberKind::App`.
- `json()` (`:2341`) adds:
  - `roster` (bounded 200: `{mri, name, kind, role, external, in_lobby, muted, server_muted, hand_position, camera, sharing}`), `roster_omitted`, `participant_count`;
  - `server_muted`, `can_unmute_self`, `unmute_locked`, `server_mute_enforced`;
  - `recording:{compliance, recording, transcribing, since_ms, initiator, observable}`;
  - `rejoin_address`.

  `others`/`other_mris` are bounded alike, and `waiting_in_lobby` = held lobby members excluding self.
- `call_join` (`:7727`):
  - learn group chat, subject, compliance and conversation links off `joined.raw` (meeting only);
  - use the own-lobby reader;
  - on `Err`, downcast `SignalRefused` and end the call with the named reason.

  Journal: `[calling] the service names the meeting chat (matches the address: yes|no|learned)` and
  `[calling] join answer shape: <key skeleton>` (shapes only; the answer never reaches the page).
- `handle_call_frame` (`:14601`):
  - a `CONVERSATION_UPDATE` branch for § 4.1/4.7 (meeting only, sequence-gated, after the existing link merge);
  - the roster branch (`:14730`) resolves names outside the lock and updates `server_muted` and
    `attendee_audio_restricted`;
  - the lobby branch (`:14746`) uses § 4.2's phase rule;
  - the end branch (`:14715`) prefers `named_meeting_end_reason` for meetings;
  - the media-answer branch (`:14905`) sets `media_answered`;
  - journal-only lines for `CONVERSATION_CONFIRM_UNMUTE` (keys), evt 128 (keys) and legacy lobby strings.
- `call_mute` (`:8423`): nothing of this design's. The server-muted branch (post only to the
  conversation's `unmute` with `self_unmute_payload`, do not flip `call.muted`, refuse before the
  network with no link) is incoming-and-conversation M9's one branch (README § 3, collision 10). M6
  adds the `server_mute` value to `call_protocol_set` and publishes `server_mute_enforced` from it.
- A recording-notice consumer from the trouter messaging role. It updates `recording` only for
  `notice.conversation_id == call.conversation_id` on a live meeting. The late-join GET is spawned once,
  when the thread becomes known.
- `meeting_started_json` beside `call_event_json` (`:12185`, which keeps returning `None` for meeting markers).
- A `person_override_changed` hook clears `names[mri]` and emits call_state.
- No environment variable is read for server mute: enforcement is the stored `server_mute` value of
  `call_protocol_set` (§ 4.6).
- Tests:
  - `a_link_join_learns_its_chat_thread_for_meetings_only`;
  - `an_incoming_one_to_one_never_gains_a_thread_mid_call`;
  - `admitted_before_the_media_answer_keeps_connecting`;
  - `self_server_mute_is_read_from_our_own_endpoint`;
  - `unmute_while_server_muted_needs_the_conversation_link_and_never_flips_local_mute` (it pins
    incoming-and-conversation M9's branch, and lands with it);
  - `call_state_names_people_through_the_override`;
  - `store_access_is_absent_from_call_state_payload_and_the_roster_branch_under_the_lock` (a source scan
    covering BOTH sites; review fix);
  - `apps_never_reach_others_or_publishing`;
  - `meeting_started_only_when_live_fresh_not_self_and_not_joining`;
  - `recording_follows_only_the_calls_own_thread`;
  - `an_initial_followed_by_success_is_not_ongoing`.

**`src/teams_read.rs`**
- `RecordingNotice{kind, status, recording_key: Option<String>, initiator_mri: Option<String>}` and
  `recording_notice(messagetype, content)`, reusing `xml_attr` (`:1272`). `initiator_mri` is filled only
  from a field M0 measures.
- `pub async fn fetch_messages_page_raw(http, session, conversation_id, page_size)->Result<Vec<Value>>`,
  GET only, a sibling of `:442`.
- The meeting marker gains `organizer_mri`.
- `parse_call_recording` and the Pending drop are unchanged, so history does not change.
- Test: `teams_read_issues_get_requests_only` scans the module for `.post(`/`.put(`/`.delete(`.

**`src/trouter_events.rs`**
- `Realtime` (`:85`) gains `recording_notices` via `recording_notices_from_event` (NewMessage and
  MessageUpdate), mirroring `typing_from_event` (`:283`). Never persisted.

**`examples/meeting_recording_recon.rs`** (new, READ-ONLY)
- Lists the newest 40 `19:meeting_` threads from CSA and reads each one's newest page through
  `fetch_messages_page_raw`. Per thread it prints `thread#n t+<s since first call marker> <messagetype>
  status=<…> code=<…> key=<present|absent> initiator_field=<names of candidate attrs> edited=<bool>`, plus
  a messagetype histogram. No content, names or ids.

**`examples/call_presence_recon.rs`** (new, READ-ONLY)
- Every 10 s for `--minutes N`, prints the user's own `availability`/`activity` through
  `fetch_presence`, with the profile token.
- It first prints a one-line baseline of the user's other signed-in clients, as the presence service
  reports them, if it reports them; otherwise it says it cannot.

**`.claude/hooks/guard-live-automation.test.py`**
- Positive cases: both new examples are allowed.
- Negative case: a copy that calls `teams_send::` or names `me/endpoints` is blocked.
- The guard script itself is unchanged.

**`web/src/lib/call.ts`**
- Optional `roster`, `roster_omitted`, `participant_count`, `server_muted`, `can_unmute_self`,
  `unmute_locked`, `server_mute_enforced`, `recording`, `rejoin_address`. Every one is optional, so a page
  in front of an older backend behaves as today.
- `callEndLabel` (`:341`) gains every new name.
- `callPresenceLabel` (`:291`) reads `participant_count` when present.
- Pure `callRecordingNotice(call)` (never negative), `callMicState(call)`, and `callEndOffersRejoin(call)`.
- Fix the stale header at `:11-13`.

**`web/src/lib/call-stage.ts`**
- `callStageParticipants` (`:222`) reads `roster` when present. A blank name becomes "Unnamed participant"
  instead of being dropped.
- New `callStageLobbyEntries`, `callStageApps`, `callStageRecordingNote`, and `handOrdinal`.
- `callStageLobbyLabel` uses the corrected count.
- `initialCallStagePanel` is unchanged.

**`web/src/components/call-stage.tsx`**
- `CallRecordingStrip`: a full-width row under the header, `role="status"`, `aria-live="polite"`,
  `data-testid="call-recording-strip"` with `data-kind`.
- A mini-window dot.
- The People panel (`:579`): roles, tags, mute glyphs, hands, the lobby section, apps, the standing line.
- A server-mute chip and the Unmute states.

**`web/src/lib/store.ts`**
- `onCallState` (`:2623`): records `lastEndReported`; under enforcement it runs the muteState rules on the
  media-holding page; it attaches a Rejoin action when `callEndOffersRejoin`.
- The `joinMeeting` catch (`:3086`): skips its own report for a named ending.
- `setCallMuted` (`:3340`): while enforced and server-muted, does not re-enable the track.
- A `meeting_started` list.

**`web/src/lib/notice.ts`**
- `Notice.action?: {label, onPress, testId}`, passed through to sonner's `action`.

**`web/src/components/meeting-started-card.tsx`** (new)
- Mounted beside `IncomingCallBanner` (`web/src/components/app.tsx:550`).

**`web/src/components/meeting-event-line.tsx`**
- `MeetingJoinButton meeting={{kind:"link", joinUrl}} shape="pill"` plus a small "Open in Teams" link.
- Fix the docstring (`:32-34`) and `MeetingSystemEvent.join_url`'s doc in protocol.ts.

**`web/mock/server.ts`**
- Accept `/meet/` (`:10235`).
- A link join learns a mapped thread (`MOCK_LINK_MEETING_THREADS`), with one link that maps to no stored
  thread.
- The roster becomes typed entries: organizer, muted colleague, raised hand, external guest, lobby
  waiter, app, and a `participant_count` large enough to show the bound.
- Hooks, all reset by `call_invite {reset:true}`:
  - `call_recording`;
  - `call_server_mute {muted, can_unmute, enforce}`, where `call_mute(false)` is refused exactly as the
    backend refuses it;
  - `call_lobby {held}`;
  - `meeting_started`;
  - `call_join_refusal {reason}`, which mirrors the ended frame BEFORE the RPC rejection, as the backend
    orders it.

**`web/scripts/join-live.ts`**
- Rails unchanged.
- `SIGNAL_PROBE` also keeps `call_state` frames (bounded).
- The digest gains:
  - `conversationShape` (key skeleton of conversationUpdate bodies, with `groupChat.threadId` reported only
    as `matches-pinned-thread: yes/no` and `complianceRecordingContent` as `present len=N`);
  - `participantIdPrefixes`, `endpointKeys`, `publishedStateTypes`;
  - `participantCounts` beside the rows drawn;
  - `callStateConversation`, the `recordingState` timeline, and `otherFrameShapes` (evt and keys).
- No `--from-link` mode by default (§ 9 step 2).

**`web/scripts/preview.ts`**
- `--call` captures gain:
  - the strip in four states, in both themes and at 390 px (proving Leave stays reachable);
  - the People panel in both themes;
  - the server-mute chip in display mode and in enforced mode;
  - the meeting-started card;
  - each end notice, with and without Rejoin.
- Use `--dpr 3`.

**Docs**
- NATIVE-CALLING.md: a new § "Meeting awareness on the wire" covering § 4.1-4.9, corrections to § 8's
  lobby reading, and the M0/M2/M4/M8 measurements.
- CLAUDE.md § Joining a meeting: delta semantics, the scoped lobby, named endings, Rejoin and its cost,
  the prompt.
- CLAUDE.md § A call is a PAGE (with M1): a link join gains the Chat tab and its panel stays closed;
  the People panel rules. **This AMENDS that section's rule**, which says "There is no second
  condition on that: every call with a thread behind it opens with it": once M1 learns a link join's
  thread, that call has a thread behind it, and this design keeps its panel closed (§ 6). The
  amendment is the user's to approve (README § 5, D13; § 12 question 1), and M1 rewrites the rule's
  sentence rather than leaving it contradicted.
- CLAUDE.md § Recording a call: a link-joined meeting's local recording now lands in its chat.
- CLAUDE.md § Audio calls: server mute display and the `server_mute` enforcement setting.
- CLAUDE.md § The user's own status: M8.
- CLAUDE.md § Automation safety: the digest fields and the two recon examples.
- Stale comments listed above, plus `src/bin/server.rs:7441-7444` (which becomes true).

## 6. Gating and consent

- **No new RPC.** Nothing new in OUTWARD_METHODS, MACHINE_METHODS or the guard's method list, so
  `every_gated_method_is_named_in_the_automation_guard` needs nothing.
- **Reads with no gate:** thread learning, lobby, roster, recording and compliance state, endings,
  refusal mapping. These become `call_state` fields emitted to every page as today. Identity stays keyed
  on mri, and names go through the one ladder.
- **Two new GETs:**
  - The late-join history GET runs only inside a live meeting, which a read-only backend never has: it
    registers no calling endpoint, since `calling_available()` is `!read_only()`, `src/bin/server.rs:14415`.
  - The presence read is a developer-run example, not an RPC.
- **`meeting_started` is a local emit.** It is never raised on a read-only backend, and its Join is the
  existing gated press. The app never joins on its own.
- **Rejoin** is one press replaying the two gated RPCs with the backend-held address of the call that
  just ended. It is offered only after a lobby timeout of a meeting, with its cost stated.
- **Server mute:**
  - Display is a parse.
  - Enforcement is OFF by default (`server_mute = "display"`). When ON, it only ever turns the user's
    own track OFF on a reading; it turns it ON only after the user's own press (state 3, or the 10 s
    escape hatch that follows that press). Turning it on is `call_protocol_set`, a `MACHINE_METHODS`
    press already named in the guard, never an environment variable.
  - `call_mute` stays OUTWARD. Its server-muted branch is incoming-and-conversation M9's (restrictive:
    it refuses before the network, and it no longer flips local mute while server-muted).
- **The chat panel does not auto-open** for a link join. So no `mark_read` (a read receipt) is published
  without the user opening it. The tab appears and the user's press opens it, marking the thread read
  as it does for any conversation. Ghost mode governs. **This amends CLAUDE.md § A call is a PAGE**,
  whose rule is "every call with a thread behind it opens with it" with "no second condition": once M1
  learns the thread, a link join is such a call. It is therefore a decision for the user (README § 5,
  D13; § 12 question 1), and M1's doc update rewrites that sentence.
- **The recon examples** are read-only because they call only GET-only crate functions
  (`fetch_messages_page_raw`, pinned GET-only by a test, and `fetch_presence`). They name no endpoint
  of their own. They print shapes, offsets and counts, never content, names or ids. Guard tests pin
  that both are allowed and that a copy which sends is blocked.
- **Deferred outward items** (consentToRecording, approveUnmute/rejectUnmute, admit, raise hand) each need
  their own entry, guard name and press, in `meeting-participation`.
- **The verified audio path:** no wire change. Near it are the lobby phase rule (§ 4.2), which is more
  conservative than today's dormant branch, and the switch-guarded enforcement.

## 7. UI and copy (including mobile)

1. **Recording strip.** A full-width row under the stage header with a red dot.
   - recording: "This meeting is being recorded".
   - transcribing: "This meeting is being transcribed".
   - both: "This meeting is being recorded and transcribed".
   - compliance: "This meeting may be recorded for compliance" (call: "This call …"). Recording and
     transcription are appended.
   - Detail line only with a measured initiator: "Started by Ada Lovelace · 10:42".
   - Never a negative. Never drawn for teams-lite's own local recording, which keeps `RECORD_HINT`
     (`web/src/lib/call-recording.ts:449`).
   - Folded window: a 6 px red dot before the title and `aria-label="Recorded"`.
2. **People panel.**
   - Heading "In this call · {participant_count}".
   - Rows: name (rename ladder); role chip Organizer / Co-organizer / Presenter; tag External / Guest /
     Phone; mic-off ("Muted" / "Muted by the organizer"); hand ("Raised hand · 2nd"); camera; sharing.
   - Hands first. "Unnamed participant" instead of a drop.
   - "Waiting in the lobby · N" with names, plus "Only an organizer or presenter can let them in." No
     Admit control: that is `meeting-participation` › `lobby-admit-others`.
   - "Apps in this meeting". "and N more people" past 200.
   - A standing line for every meeting: "teams-lite shows a recording when Teams announces it in the
     meeting chat."
   - When the chat is not observable: "teams-lite cannot read this meeting's chat, so it cannot see
     whether it is being recorded."
3. **Mic control.**
   - Display mode: the glyph shows the local truth, with a warning chip "Muted by the organizer".
   - Enforced mode: the muted glyph in warning tone, label "You were muted by the organizer — Unmute".
   - Locked: disabled, title "The organizer muted you. This meeting does not let you unmute yourself."
   - After a clear from state 1: "The organizer lets you unmute now".
4. **Lobby.** "Waiting to be let in…" is driven by the scoped signal.
5. **End notices:** § 4.3's sentences.
   - Rejoin is a sonner action labelled "Rejoin", with title "Puts you back in the lobby; the people who
     can admit you are told you are waiting."
   - A join refusal shows its named sentence and is never overwritten by the raw text.
6. **Meeting-started card**, top centre like the awareness banner: "<title> has started", "Organized by
   <name>" (resolvable only; review fix: never "Started by"), [Join here] [Open chat] [×].
7. **Meeting activity line:** "Join here" (in-app) plus "Open in Teams".
8. **Link joins:** the Chat tab appears when the service names the thread and the store holds it. The
   panel stays as decided at call start (closed), per `call-stage-context.tsx:54-74`. A link-joined
   meeting's local recording now files into that chat, because `store.ts:3233` reads `conversation_id`
   at the end.
9. **Mobile.**
   - The strip is its own row, so the header gains no control. The 390 px capture verifies Leave stays on
     screen.
   - People rows are 44 px tall, and chips truncate before the name.
   - The card follows the awareness card's width rules.
   - Glyphs come from hugeicons only: `MicOff01Icon`, a hand glyph, `UserGroupIcon`, `RecordIcon` or the
     nearest existing one. `web/src/lib/icon-library.test.ts` stays green.

## 8. Tests, mock and captures

- **Rust:** the tests named in § 5, plus `cargo test` overall.
- **Web unit:**
  - `call.test.ts`: every new label; `callRecordingNotice` never negative; `callMicState` in display and
    enforced modes; `callEndOffersRejoin` (meeting plus timeout only).
  - `call-stage.test.ts`: roster rows, no drops, lobby excluding self, apps, hand ordinals, counts that
    differ from `participantCounts`, the older-backend fallback.
  - `notice.test`: the action pass-through.
- **E2E:**
  - `web/e2e/calling.spec.ts:515-519` is rewritten into two cases: a link whose thread the store holds
    shows the Chat TAB after admission with the panel closed; a link with no stored thread shows none.
  - A new `web/e2e/meeting-awareness.spec.ts`:
    - the strip per hook state, and its absence for a local recording;
    - People rows and the lobby section;
    - server mute in both modes, Unmute refused with no link, no auto re-enable after a state-1 clear;
    - a held lobby → released → admitted, with no Connected before the media answer;
    - each end notice;
    - a join refusal whose FINAL notice text, after the RPC settles, is the named sentence;
    - Rejoin re-joining only after a timeout;
    - the meeting-started card stating `data-meeting-thread`, and suppressed while a link join is still
      unresolved;
    - "Join here" stating `data-join-url`;
    - a `/meet/` link accepted.
  - Every spec resets with `call_invite {reset:true}`.
  - Run `E2E_MOCK_PORT`/`E2E_WEB_PORT` on free ports.
- **Captures:** `cd web && bun run preview -- --out /tmp/call --call --dpr 3`.
- **Suites:** `cargo test`, `cd web && bun run test && bun run typecheck && bun run test:e2e`, and
  `python3 .claude/hooks/guard-live-automation.test.py`.

## 9. Live verification plan

| # | Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- | --- |
| 1 | M0 recording timing from history: `cargo run --example meeting_recording_recon` | new read-only example | agent (no backend started) | For past recorded meetings: whether an `Initial` exists BEFORE the call-end marker (a start signal) or only after it (processing); whether a recording key and an initiator field exist; the transcript messagetype. Recorded in NATIVE-CALLING.md. |
| 2 | M0/M1 conversation skeleton: `cd web && bun run join-live -- --from-chat --hold 60` (after the user re-stages) | join-live (extended digest) | agent; the user started the service | Journal `[calling] the service names the meeting chat (matches the address: yes)` and the answer's key skeleton. Digest `conversationShape … matches-pinned-thread yes`, `complianceRecordingContent: absent`, `participantIdPrefixes`, `endpointKeys`, `participantCounts` beside rows. Audio verdict unchanged. **The no-thread path itself** is verified by the user's own paste of the meeting's `/meet/` link into "Join with a link" (their click): `conversation_id` goes from null to the thread, and the Chat tab appears. If a driver mode is ever wanted, `--from-link` reads the passcode from an untracked 0600 file outside the repo, checks the committed CODE with `namesPinnedMeeting` before the click, and refuses without the file. The passcode is never committed. |
| 3 | M2 own-leave and ordinary end subcodes: the join-live hold above hangs up; separately, one `cd web && bun run call-live -- --hold 30` **by appointment with the authorized colleague**, who hangs up | join-live, call-live | agent; the colleague by appointment | The journal records `code/subCode/phrase` of the service's confirmation of our leave, and of the far-side hangup of a 1:1. Neither is 0/5000 (or, if one is, the 1:1 guard is proven necessary). |
| 4 | M2 end-for-all: while `join-live -- --from-chat --hold 120` is in the pinned meeting (the user is its organizer), the user presses "End meeting" in real Teams | join-live + the user's click | the user | The journal names the subCode an attendee receives when a meeting is ended for everyone. If it differs from 5000/5300, `RemovedOrEnded` is split. |
| 5 | M4 record and transcribe: `join-live -- --from-chat --hold 240`; the user presses Record, then Start transcription, waits 60 s, stops both. Everybody present agrees. | join-live + the user's clicks | the user | The digest's `recordingState` goes ON within seconds. The journal names the frame that announced it (chat notice status, a conversationUpdate key, or both) and any stop signal. A screenshot shows the strip. |
| 6 | M3/M5/M6 other people's fields: the user joins a colleague-organized meeting with "Join here"; the colleague (consenting) mutes them, raises a hand, admits a guest, and with lobby on denies then admits a second attempt | the user's own click | the user and a consenting colleague | Roles, mute glyphs, hands and the lobby section are correct. "Muted by the organizer" appears with the mic glyph truthful (display mode). The journal shows `serverMuted` and conversation links. Then (M6, after incoming-and-conversation M9), with Server mute = "enforce" set by the user in `call_protocol_set`, the track goes off, Unmute posts M9's body to the conversation link, and the clear behaves per § 4.6. Denial gives "You were not let into the meeting." |
| 7 | M2 B2B: the user joins another organization's meeting from its invite | the user's click | the user | Either a join (likely lobbied) with External tags, or a refusal sentence naming the policy and NOT overwritten by raw text. The journal shows status and subCode. |
| 8 | M7 prompt: passive. The backend journals every live meeting marker with `sender_is_self` and the emit/suppress reason during the user's ordinary day. The card is seen when a colleague starts a meeting the user is in. | observation only | nobody acts | One card per started meeting not started by the user; none for the user's own starts or their other installs; the evt-128 keys journalled if one arrives. |
| 9 | M8 presence: `cargo run --example call_presence_recon -- --minutes 4` beside `join-live -- --from-chat --hold 90` (needs nobody), twice (Always available off, then on, flipped by the user). The 1:1 half once, by appointment, with call-live. | new read-only example + join-live | agent; the user flips the setting | A table of availability/activity before, during and after, with the baseline of other clients. Outcome A: derived "InAMeeting", build nothing. B: masked by Always available, a user decision. C: never, a documented limit. |

The pinned meeting has had a colleague in it (join-live.ts:90-93). Steps 2, 4, 5 and 9 join it, so they
run only when the user confirms who is in it at that time.

## 10. Milestones

0. **Measure first** (S, risk none).
   - Digest fields, shape-only journal lines, `fetch_messages_page_raw` with its GET-only scan, two
     recon examples with guard tests.
   - Run § 9 steps 1-3. Deps: coordinate the join-live digest with `video-receive` ›
     `video-measurement-blind-spots`.
1. **Link and calendar joins learn the meeting chat** (S, low).
   - § 4.1 for meetings, the sequence gate, `conversation_links` introduced (read by nothing yet), the
     mock `/meet/`, the spec rewrite, docs. Deps: none.
2. **Scoped lobby and named meeting endings** (S-M, medium).
   - § 4.2 with the phase rule, § 4.3 (meeting only, `RemovedOrEnded` until step 4 splits it), § 4.4
     with the page fix, § 4.10 Rejoin.
   - Ships after steps 2-3 show the phase timeline and subcodes. Deps: none outside this design.
     `plane-reliability` rejects changing the hangup phrases for now (its § 3), so the named endings
     read the service's phrases as they are and no phrase change is expected; `media-robustness-devices`
     › `call-recovery-after-drop-or-reload` keeps rejoin after a reconnect (an exclusion, not a
     dependency).
3. **Roster fidelity** (M, medium).
   - § 4.5 in full, the People panel, mock, e2e, preview.
   - Deps: `video-receive` (the `publishing` consumer: per-participant semantics must not drop a
     publisher; coordinate before merging). **This milestone OWNS the roster parse, raised hands and
     the other published states included**; `meeting-participation` M2 (`raise-hand`) consumes it
     and lands after it, which breaks the cycle the two designs had.
   - **It keeps two existing semantics, pinned by a test**: `others` (everybody but the user's own
     account) and video-send M1's `own_streams` (our own endpoint's streams). video-send M4's
     roster safety rail reads both and lands before this milestone (P2 against P3), so the Delta
     semantics introduced here must not change what either field means. A rail that misread `others`
     would let a test camera go into a meeting with a colleague in it.
4. **Recording, transcription, compliance indicator** (M, medium).
   - Compliance (hedged, sticky) ships after M1. Cloud recording and transcription ship only after M0
     step 1 and § 9 step 5.
   - Deps: M1. **This milestone owns the recording-notice parser.** It used to name
     `meeting-participation` › `teams-cloud-recording-control` as a co-owner, but that workstream
     REJECTS cloud-recording control (its § 3), so nothing else will build the parser.
5. **Server mute: display** (S, low-medium).
   - § 4.6 display half, `conversation_links` read. Deps: M3.
6. **Server mute: enforcement, the `server_mute = "enforce"` value of `call_protocol_set`** (S, medium).
   - Default `display` (off). On by default only after § 9 step 6 and the self-mute body is verified.
   - It is a mode of incoming-and-conversation's stored per-backend setting, not an environment switch,
     and it adds only the page's track rules: the backend branch is incoming M9's (README § 3,
     collision 10).
   - Deps: `incoming-and-conversation` M9 (the one server-muted `call_mute` branch, so its body and
     door; this milestone lands after it), M3 (`self-mute-not-visible-in-meetings`, the setting) and M1
     (`mute-enforced-on-media-page`).
7. **"Meeting has started" prompt and in-app joins from meeting lines** (S, low).
   - Deps: `ringing-alerts-history` M9 (`call-line-actions`), which OWNS the one Join vocabulary on
     call and meeting lines and lands first; this milestone consumes it, so the two designs no longer
     wait on each other. Push stays their decision.
8. **Presence in a call: measure and decide** (S, none). Deps: none.

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A false "not recorded" impression (the live source is unknown; `Initial` may mark the END). | M0 gates the recording half. No negative copy anywhere. A standing People-panel line plus a not-observable footnote. Compliance ships separately. Initial is paired with later statuses. |
| Lobby scoping regresses the verified join (stuck "Joining…" or false "Connected"). | Admitted only clears `in_lobby`. Connected from a lobby exit requires `media_answered`. The organizer path takes today's media-answer branch. Every transition is journalled with its source. The legacy walk stays journal-only until a real lobby is measured. The phase timeline from § 9 step 2 is checked first. |
| A wrong sentence on an ending ("removed" for an end-for-all or a 1:1 hangup). | Names apply to meetings only. 5000/5300 says "removed, or ended for everyone" until § 9 step 4 splits it. The 1:1 phrase path is untouched. |
| A join refusal is overwritten by raw text. | `lastEndReported` plus an e2e asserting the FINAL notice. |
| Delta semantics drop people or publishers. | The client's per-participant version and tombstones, the five presence modalities, removal only on `inactive`, out-of-order unit tests, coordination with `video-receive`. |
| Server-mute enforcement silences the user wrongly, or reopens the mic on someone else's action. | Display by default with a truthful glyph. Enforcement behind an off-by-default switch. No auto re-enable after a state-1 clear. A 10 s escape hatch after the user's own Unmute. |
| A stale `unmute` link makes "locked" unreachable, or posts to a withdrawn link. | `conversation_links` is replaced per update, sequence-gated. "Locked" is drawn only on a frame signal. |
| SQLite under the calling mutex. | Resolve outside the lock. A source scan of both `call_state_payload` and the roster branch. |
| Large meetings inflate `call_state`. | JSON lists bounded at 200, with `roster_omitted`. |
| The chat panel publishes a read receipt nobody asked for. | No auto-open. Auto-open is an open question. |
| Prompt noise (backfill, own starts, two installs, link joins). | Live only, 10 min freshness, not self, not while joining an unresolved meeting, 5 s grace, 30 min dedup, muted chats dropped, no push. |
| A meeting passcode is leaked through a driver constant. | Never committed. The user's own paste, or an untracked file. |
| The recon examples are shaped to slip past the guard. | They are read-only by construction (GET-only functions, pinned by a test), with explicit guard tests for both directions. |

## 12. Open questions and decisions for the user

1. **Auto-open the chat panel when a link join learns its thread?** It would publish a read receipt
   (Ghost mode governs). The default here is no: the tab appears and the user opens it. Keeping it
   closed **amends CLAUDE.md § A call is a PAGE** ("every call with a thread behind it opens with it",
   with "no second condition"), so this is README § 5, D13, and M1 rewrites that sentence.
2. **May the pinned meeting be used for § 9 steps 2, 4, 5 and 9 at the moment you choose?** It has had a
   colleague in it. Step 5 records and transcribes for 60 s, and everybody present must agree. Step 4
   ends the meeting for everyone in it.
3. **Do you want a `--from-link` join-live mode?** If yes, the passcode lives in an untracked 0600 file
   outside the repo, which you create. Otherwise your own paste verifies the no-thread path.
4. **Should Rejoin ever be offered after a denial or a removal?** The design says no: respect the
   organizer.
5. **Should apps and bots (`28:`) be listed?** A recording bot may be the only visible sign on some
   tenants. M0's id-prefix digest will show whether any appear.
6. **When should server-mute enforcement become the default?** The design says after § 9 step 6 and the
   self-mute body landing (incoming-and-conversation M9). Until then it is the `server_mute` value of
   `call_protocol_set`, which you set yourself.
7. **If M8 shows the Always-available endpoint masks "In a call"**, should teams-lite stop refreshing it
   while a call is live? It is inside the existing consent, but it changes what colleagues see.
8. **Is "meeting started" worth a Web Push?** That is a product decision under § Push notifications.
9. **For § 9 step 3:** one call to the authorized colleague, by appointment, to measure a far-side hangup.

## 13. Review notes

**Accepted and changed** (both reviews, re-checked against the code and the bundle):
- `Links::merge` never removes a link (`src/calling.rs:446-452`), and the client replaces its conversation
  links (`this.links[I.LINKS.UNMUTE]=e.links.unmute||null`). A separate replaced `conversation_links` now
  drives `can_unmute_self`, and "locked" needs a frame signal. `attendeeAudioRestricted` is in `YP`; its
  wire location is unknown, so it is recorded by M0.
- The global roster sequence drop applied only to `MultiPartyEndpoint` frames. Delta handling is now
  per-participant `version` with tombstones, and counts are adopted only on a newer sequence (verified in
  `handleDeltaRoster` and `FV.handleRosterUpdate`).
- "Neither call nor lobby means absent" was wrong. All five modalities count, an empty active update is
  skipped, and removal happens only on `inactive` (verified in `fromRoster` / `fromRosterEndpoints` /
  `handleRosterParticipant`).
- The chat panel cannot open by default, because `call-stage-context.tsx:54-74` decides it once. The
  design now says the tab appears, the panel stays closed, and no automatic mark_read happens.
  Auto-open is open question 1.
- A join refusal sentence would be overwritten by the `joinMeeting` catch (`store.ts:3086-3093`). Fixed
  with `lastEndReported` and an e2e.
- A scoped Admitted would have set Connected before the media answer (`server.rs:14746-14768`). Fixed by
  the phase rule and `media_answered`.
- Auto re-enable after an organizer clear (client muteState 1) would open the mic on somebody else's
  action. The design now re-enables only after the user's press; this diverges from the client on
  purpose.
- **The blocker:** enforcement shipped by default on an unmeasured reading. It is now split, with
  display by default and enforcement behind an off-by-default switch plus an escape hatch. The display
  keeps the mic glyph truthful.
- Named endings applied to every call kind. They are now meeting-only. 5000/5300 is worded to cover
  end-for-all until measured, and two measurement steps were added (§ 9 steps 3-4).
- Committing a passcode would publish a key. The default is now the user's own paste; an optional mode
  reads an untracked file.
- The join answer never reaches the page (SIGNAL_PROBE, `server.rs:7803`). It is now measured through
  the backend journal.
- `--pair` cannot exercise other people's fields, and teams-lite never updates `endpointState`. M3 and
  M5 now need a second person (§ 9 step 6).
- Compliance is now sticky, hedged and fixture-only, with the inference stated.
- Hand rank: sorted by `typeRank`, ordinal displayed.
- Apps: excluded from `others`, `other_mris`, `publishing`, the waiting count and the recorder.
- The own tenant comes from our own roster entry; External needs both tenant ids.
- The "held roster bounded at 1 000" claim referred to no code. It is dropped, and the JSON bounds are
  real and tested.
- The meeting_started race during link joins, and self markers, are suppressed, with a 5 s grace.
- Rename across backends: re-resolve names older than 60 s per roster frame.
- An Initial must be paired with any later status before it counts as ongoing. The source scan covers
  the roster branch too. The citation is now NATIVE-CALLING.md:224-228 (it was 212-214).
- "Started by" became "Organized by". A recording initiator comes only from a measured field, never
  from the notice's sender.
- Thread learning is scoped to `CallKind::Meeting` (an incoming 1:1 can have no thread).
- The prompt is gated on `calling_available()`.
- The recon framing changed to read-only through GET-only functions, with a module GET-only scan and
  guard tests in both directions.
- M3/M6/M7 live plans changed (the prompt and presence are numbered M7 and M8 in § 10; this note
  predates the renumbering, so its "M6" is the prompt and its "M7" is presence): the --pair mute step
  was dropped; presence now runs on join-live, with the 1:1 half by appointment and a baseline; the
  prompt became passive, and the colleague-notification consent moved into the open questions.
- `selfParticipant` is read for the lobby, and the legacy walk is kept journal-only until a real lobby
  is measured.
- Counts: `participant_count` = rows held including self, excluding the lobby; lobby waiters exclude
  self; `participantCounts` is only journalled until its semantics are measured.
- A standing People-panel line covers the "no strip means not recorded" reading.
- The capability-bit rule's citation is now `src/calling.rs:35-42`, not CLAUDE.md.

**Rejected or modified:**
- **"Treat a missing conversation sequenceNumber as apply"** (reviewer 1). Modified: the design mirrors
  the client instead. Absent counts as -1, so a frame without a number is ignored for these fields once
  a numbered frame has been applied; the join answer is always applied. The client code is the
  evidence (`a=e.sequenceNumber>=0?e.sequenceNumber:-1;if(s&&a<this.updateSequenceNumber)…return`), and
  "apply" would let an unnumbered stale frame replace `conversation_links`.
- **"Emit meeting_started only when the plane is ready"** (reviewer 2). Partly rejected: it is gated on
  `calling_available()` (no read-only prompts), but not on readiness. `MeetingJoinButton` already states
  a not-ready reason, and a transient reconnect must not eat a 10-minute prompt.
- **"Offer Rejoin for Reconnected after the plane is ready"** (reviewer 2's alternative). Moved out of
  scope: rejoin after a moved registration is `media-robustness-devices` recovery, and here Rejoin is
  only after a lobby timeout.
- **The escape hatch** (reviewer 2). Accepted, but narrowed so that it fires only after the user's own
  Unmute press. It is never an autonomous re-enable.
- **"Derive the link from a calendar event or chat-line URL"** for `--from-link` (reviewer 1). Not
  usable: the pinned meeting is a "Meet now" in no calendar (`join-live.ts:93-99`), and nothing shows
  its chat holds the link. The user's own paste replaces it.
