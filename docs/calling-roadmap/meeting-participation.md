# Meeting participation: hands, reactions, the lobby, moderation, and joining a call already going on

## 1. Summary

This workstream lets the user take part in a Teams meeting the way a stock client does, and join a
group-chat or channel call that is already going on. The user can raise and lower a hand and see the
queue. They can send and receive live reactions. As organizer, they can admit people from the lobby.
As organizer or presenter, they can moderate: mute one person or everyone else, remove someone,
change a role, lock the meeting, and end it for everyone. Every new act is one press. Each is its own
`OUTWARD_METHODS` entry, named in the automation guard, refused by a read-only backend, and mirrored
in the mock. Every control is drawn only where the service handed over the link AND the user's own
role allows the act.

Nothing here is on by default until it is proven. A new MACHINE-gated setting, **Meeting controls in
preview**, is off by default. An act is offered only when that setting is on or when the act is
listed in a `PROVEN_PARTICIPATION` constant, which a later commit fills once the live proof is in.
With the setting off, the verified audio paths (1:1 both ways, meeting join) send byte-for-byte what
they send today, and golden tests pin that.

Proofs run on the user's own two installs in the pinned `join-live` meeting where a pair can show
the act. Everything else waits for the user's own click. Meet now, breakout-room management, live
captions, Teams cloud-recording control, starting a channel meeting, spotlight, attendee hard-mute
and approving an attendee's unmute request are deferred or rejected, with the reasons stated in § 3.

## 2. Where we are today

| Area | Today | Where |
| --- | --- | --- |
| Participation links | No code reads `publishState`, `removeState`, `sendMessage`, `admit`, `admitAll`, `removeParticipant`, `updateParticipantRole` or `updateMeetingSettings`; a grep of `src/` and `web/src` finds none of these names. They arrive in the join answer (measured: `admit`, `admitAll`, `sendMessage`, `mute`, NATIVE-CALLING.md:224-226) and are folded into one merged table that never forgets a link. | `src/calling.rs:303` (`Links`), `:449` (`merge`), `src/bin/server.rs:14682-14692` |
| Conversation URL | `JoinedConversation.controller` is parsed and never used. `CallSession` does not keep it. | `src/calling.rs:1848-1851`, `:1880` |
| Proxied messages | `/conversation/receiveMessage/` is published in every conversation request, but `handle_call_frame` has no branch for it, so a reaction frame is dropped. | `src/calling.rs:129`, `:1233`; `src/bin/server.rs:14601` |
| Roster | `RosterMember{mri, display_name, in_lobby, present, streams}`. `in_lobby` comes from `state` strings, which this tenant never sends (it sends `active`/`inactive`). Roles, `publishedStates` and endpoint mute are not parsed. | `src/calling.rs:1989-2003`, `:2058-2067` |
| People panel | Lists names (blank names dropped) with a lobby count line from `callStageLobbyLabel`. Rows have no actions. | `web/src/lib/call-stage.ts:222-262`, `:276`; `web/src/components/call-stage.tsx:579` |
| Own mute | `call_mute` posts `{muteUnmute:{sender, muted, scope:"Myself"}}` to the `mute`/`unmute` link. The client uses `updateEndpointState` for this and the `mute` link only for muting others (incoming-and-conversation › `self-mute-not-visible-in-meetings`). | `src/bin/server.rs:8423-8458`; `src/calling.rs:1559-1567` |
| Join addresses | A link (`from_join_url`) or a `19:meeting_` thread (`from_thread_id`). Anything else is refused. A channel long link builds `groupChat{threadId: channel, messageId: post}` and has never been joined live. | `src/calling.rs:1609`, `:1689-1691`, `:1712`; `src/bin/server.rs:10149-10163` |
| Join body | `groupChat.messageId` is `"0"` when empty; `allowConversationWithoutHost:true`; no `suppressDialout`. | `src/calling.rs:1764-1846` (`:1778`, `:1816`) |
| Group chat | The menu row "Call everybody" builds an invitation that RINGS every member (capped at 20). The awareness banner offers only "Open chat" for a group chat or channel. | `src/bin/server.rs:7505-7583`, `:10173`; `web/src/components/conversation-menu.tsx:310-335`; `web/src/components/incoming-call-banner.tsx:84-89` |
| Channel call events | A call event is stored with an EMPTY `thread_root_id`: the call-event branch returns before `parse_thread(m)` runs. The page groups channel posts by `m.thread_root_id \|\| m.id`, so a call event becomes a pseudo-thread keyed by its own id. | `src/teams_read.rs:1658-1680` (vs `:1690`); `web/src/lib/threads.ts:34` |
| Gating | `OUTWARD_METHODS` is `[&str; 23]` and holds nine calling entries (`call_place`, `call_join`, `call_accept`, `call_answer_media`, `call_offer_media`, `call_start_sharing`, `call_stop_sharing`, `call_hangup`, `call_mute`). The calling-membership test covers only four of them. | `src/bin/server.rs:367-391`, `:17137-17160` |
| Guard | The write-RPC alternations name every calling method. No rule covers a cargo example that links `teams_lite::calling`; today two examples do (`meeting_join_probe`, pinned to the calendar subject `"Onboard"`; `meeting_join_recon`, read-only). | `.claude/hooks/guard-live-automation.sh:981`, `:792`; `examples/meeting_join_probe.rs:39` |
| Docs | "What is deliberately NOT built: … admitting somebody from a lobby …" | NATIVE-CALLING.md:488-490 |

## 3. Gaps covered, deferred and rejected

**Covered** (from `w1/ws/meeting-participation.json`): `raise-hand`, `live-reactions`,
`lobby-admit-others`, `organizer-controls` (mute one, mute everyone else, remove, change role, lock,
end for everyone), `join-ongoing-group-call` (both entries), and `channel-meetings` (joining only).

| Gap | Decision | Why |
| --- | --- | --- |
| `meet-now` | DEFERRED | No creation endpoint exists in any downloaded chunk. `services-meeting-scheduling-service.js` holds `getMeetingObject` (GET) and nothing else, and `calling-app.js` holds only the MeetNow/Adhoc type enum. It is still a write to Teams' meeting service, and each press creates a new tenant object, so no constant target could ever prove it. A group call in a chat and the pinned or pasted Meet-now link already cover ad-hoc meetings. Revisit only after a recon finds the endpoint, as its own OUTWARD entry. |
| `breakout-rooms` (manage) | REJECTED | Runs on a separate middle tier (`services-breakout-rooms.js`, `beta/breakoutRoom/${tenant}/${organizer}/${thread}/${message}`, auth source `CMDStable`) whose token this app does not hold, and it changes the meeting for everyone. |
| `breakout-rooms` (follow a move) | DEFERRED | `breakoutDetails` arrives on `/conversation/localParticipantUpdate/` (`processLocalParticipantUpdateResponse`), but no field of it is read in the downloaded chunks, and the implied `joinMeetingGroup` has never been seen. M0 records the frame shape while the user opens rooms in real Teams. The smallest honest follow-up is a notice ("You were assigned to a breakout room — this app cannot follow you there yet"), never an automatic move. |
| `live-captions` | DEFERRED | Turning captions on is the client GraphQL mutation `startClosedCaptions({teamsCallId})` (`calling-app.js`). Its resolver and the caption-entry transport live in `services-io-calling-c6bd7cb820231eb7.js` and `services-io-calling-commands-46e6504366eefe69.js` (ids in `runtime.js`), which were not downloaded. `async-apps-closed-captions.js` is only the pop-out window shell. Starting captions brings a transcription bot into the call, so it would be at least an OUTWARD press. |
| `teams-cloud-recording-control` | REJECTED | The transport is not located. It notifies everyone and stores everyone's audio and video in the tenant: the most outward act in this area, adding little over the local recorder. For the call-recording owner (no code here): stock clients flag external recorders (`external-recording-banner.js` reads `participant.externalRecording.isRecording`), teams-lite publishes nothing of the kind, and `RECORD_HINT` (`web/src/lib/call-recording.ts:449`) already says so. |
| Starting a channel meeting ("Meet" in a channel) | DEFERRED | It posts a new meeting into the channel for every member, and there is no sandbox channel. Only joining is built. A join to a channel meeting that has ENDED may restart it under the same post (§ 4.16); that is stated on the button. |
| Spotlight | DEFERRED | `publishState` type `spotlight` with `participantIds` is meaningful only once video receive works (video-receive). |
| Attendee hard-mute and attendee restrictions | DEFERRED | Their effect has to be visible first (meeting-awareness § 4.6 server mute). Their content shapes are unmeasured. |
| Approve or reject an attendee's request to unmute (`confirmUnmute`) | DEFERRED | meeting-awareness handed it here. It belongs with attendee hard-mute, and the current client's own callback is empty (`onUnmuteRequested:n=>{}`). Approving opens somebody else's microphone, so it would be its own OUTWARD entry. |
| Remove-participant pair test | NOT PROPOSED | The pair is two endpoints of ONE participant. Removing your own other endpoint exercises the endpoint-scoped body (`DV`), not the participant-scoped one (`OV`) used on a colleague. |
| Mute-everyone-else pair test | NOT PROPOSED (dropped in review) | Same reason: "everyone else" at participant level is empty when the only other endpoint shares the user's mri, so the run proves nothing about the body used on colleagues. |

## 4. Protocol

### 4.0 Shapes common to every act

- **No `payload` envelope on the wire.** Every bundle builder returns `{payload:{…}}`; the transport
  sends the contents (NATIVE-CALLING.md § 2.3). Every builder below returns the CONTENTS.
  `no_body_carries_the_sdk_request_envelope` (`src/calling.rs:3362`) is extended to them.
- **`from`** is `LocalParticipant::json()` (`id, displayName, endpointId, participantId, languageId`,
  `src/calling.rs:204-212`). The role builder (`LV`) omits `displayName`, and the role body follows it.
- **The state sequence counter.** `publishState`, `removeState` and `updateMeetingSettings` share
  one per-session counter, pre-incremented
  (`calling-pluginless.js` @1328619 `getPublishStateSequenceNumber(){return++this.publishStateSequenceNumber}`).
  `CallSession.publish_sequence: u64` starts at 0, so the first value sent is 1.
- **Callback links minted per request.** The bundle mints a fresh cause segment for each callback
  link (@931704 `function lt(e,t,i=Ht()){return`${e.baseMessagingChannelUrl}${I.URL_BASE.CALLAGENT}/${e.sessionId}/${i}${t}`}`).
  teams-lite reuses one `cause_id` per call (`src/calling.rs:284-290`). For the NEW callback links
  only (admit, admitAll, remove, sendMessageStatus), the backend mints a fresh cause
  (`CallbackBase::with_cause(short_cause_id())`). It records `PendingOp{cause, act, mri, name, deadline}`
  so that a success or failure frame names WHICH request and WHICH person it answers. Existing links
  are unchanged.
- **Names on the wire are the ROSTER's own.** Where a body names another person
  (`to:[{id, displayName}]`), `displayName` is that person's `details.displayName` as the roster carries
  it (`RosterMember.display_name`), never the store's nickname ladder: a rename is a local override
  that must never be written back to Teams. The bundle builds these from the roster participant
  (`AV`/`OV` `to:[{id:t.id,displayName:t.displayName}]`). A unit test pins it.

### 4.1 The conversation snapshot: links, controller, sequence

- **Method:** none outward. Read the join answer, and every frame on `/conversation/conversationUpdate/`
  (URL-matched, `calling::paths::CONVERSATION_UPDATE`).
- **Rule (the client's, verbatim in effect):** let `n = sequenceNumber`, or `-1` when it is absent. A
  snapshot is applied only when `n >= conversation_sequence` (which starts at `-1`). This applies to
  the join answer as well: the answer races the frames the trouter loop handles while `call_join`
  awaits its POST, and an answer older than an applied frame must not win. When applied, these are
  REPLACED wholesale (not merged): `conversation_controller` from the top-level
  `conversationController`, and `conversation_links` from the TOP-LEVEL `links` object only. A body
  with no `links` object is not a snapshot (the client would throw on `e.links.X`), and is skipped.
- **Body:**

```text
{ conversationController, sequenceNumber?, subject?,
  links: { leave, mute, unmute, admit, admitAll, publishState, removeState, sendMessage,
           removeParticipant, updateParticipantRole, updateMeetingSettings, … },
  state: { conversationType, isMultiParty, isHostless, … },
  activeModalities: { call?, groupChat?: { threadId, messageId, … } } }
```

- **Bundle evidence** (`calling-pluginless.js` @1344600 `processConversationServiceUpdate`):

```text
a=e.sequenceNumber>=0?e.sequenceNumber:-1;if(s&&a<this.updateSequenceNumber){r.info(`seqNum=${a} is less than latest seqNum=…`);return}
… this.updateSequenceNumber=a,this.links[I.LINKS.CONVERSATION_CONTROLLER]=e.conversationController,…
this.links[I.LINKS.ADMIT]=e.links.admit||null,this.links[I.LINKS.ADMIT_ALL]=e.links.admitAll||null,…
this.links[I.LINKS.UPDATE_PARTICIPANT_ROLE]=e.links.updateParticipantRole||null,this.links[I.LINKS.PUBLISH_STATE]=e.links.publishState||null,…
this.links[I.LINKS.SEND_MESSAGE]=e.links.sendMessage||null,…
```

  `handleConversationUpdate(e){…this.processConversationServiceResponse(t,i,2)}` (@1395050). For an
  incoming call, the client takes the snapshot from the attach answer's
  `additionalActionResponses[0].output` (@1295318).
- **Ownership:** meeting-awareness § 4.1 introduces `conversation_links` and the sequence gate. This
  workstream needs two things that design does not yet state: (a) the gate also applies to the join
  answer (meeting-awareness says "the join answer is always applied"); (b) the link replacement also
  runs for a group-chat or channel JOIN (`CallKind::Group` with an empty ring list), not only for
  `Meeting`. Chat-thread learning stays meeting-only. The incoming path's snapshot from the attach
  answer belongs to incoming-and-conversation › `incoming-attach-at-ring-and-join`. Until it lands,
  an answered incoming group call has an empty `conversation_links`, so every `can_*` below is false.
- **Status:** the sequence check and the replacement are read-from-bundle; the ~37 links in a join
  answer are measured (NATIVE-CALLING.md:224-228). Whether conversationUpdate frames carry
  `sequenceNumber` on this tenant is measured by M0.

### 4.2 Raise one's own hand

- **Method:** POST, synchronous (the answer carries the result).
- **Link:** `conversation_links.publishState`. Whether an organizer's join answer on this tenant names it
  is unmeasured; M0 answers it from `joinLinks` and `frameLinks`.
- **Body:**

```json
{
  "from": { "id": "8:orgid:…", "displayName": "…", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
  "publishedState": {
    "content": "<JSON OBJECT — UNMEASURED, from M0>",
    "level": "<\"user\" | \"endpoint\" — UNMEASURED, from M0>",
    "stateType": "raiseHands",
    "sequenceNumber": 1
  }
}
```

  There is no `scope` and no `to`: one's own hand has no participant list.
- **Answer:** `publishStateResponse.stateId` (or `results[0].stateId`), kept as `my_hand_state_id`. A
  hand raised from another page or before a restart is found again by our own roster entry.
- **Callbacks:** none. Everyone, us included, then sees the state in the roster.
- **Bundle evidence** (`calling-pluginless.js`):

```text
@1187102 function rG(e,t,i,r,n,s){const a={payload:{from:{id:…,displayName:…,endpointId:…,participantId:…,languageId:…},publishedState:{content:r,level:i,stateType:t,sequenceNumber:e.getPublishStateSequenceNumber()},scope:n}};if(s&&s.length>0){const o=s.map(l=>({id:l}));a.payload.to=o}return a}
@1804499 publishState(t,i){const r=gT(t.type),n=this.getPluginlessPublishLevel(t.level),s=t.participantIds&&t.participantIds.length>0?"specified":void 0;…publishStateAsync(r,n,i,t.content,s,t.participantIds)}
@1327218 publishStateAsync(e,t,i,r,n,s){…const a=this.links[I.LINKS.PUBLISH_STATE];…let o={};try{o=JSON.parse(r)}…sendPostRequest({url:a,…payload:rG(this,e,t,o,n,s)…}).then(l=>{if(l?.response?.publishStateResponse)return this.parsePublishStateResult(l,i);…
@941802 function gT(e){switch(e){case"raiseHands":return"raiseHands";…
```

  `scope` is `undefined` when there are no participant ids, and JSON drops it. The level values
  `"user"`/`"endpoint"` come from enum `sT` (@907594). The raiseHands content is built in
  `services-io-calling`, which was not downloaded; `calling-app.js` only dispatches
  `executeCommand("ToggleRaiseHandCurrentCall")`.
- **Status:** envelope, link name, answer: read-from-bundle. `content` and `level`: UNMEASURED.
  M2 does not ship until M0 has measured them.

### 4.3 Lower one's own hand

- **Method / link:** POST to `conversation_links.removeState`.
- **Body:**

```json
{ "from": { … }, "sequenceNumber": 2, "scope": "specified", "stateIds": ["<our stateId>"] }
```

- **Answer:** `removeStateResponse.results[{stateId, result:{code, phrase, subCode}}]`.
- **Bundle evidence:** @1187711 `function nG(e,t,i,r){const n={from:{…},sequenceNumber:e.getPublishStateSequenceNumber(),scope:t};return t==="specified"?n.stateIds=i:n.stateType=r,{payload:n}}`;
  @1805056 `removeState(t,i){return this.signalingSession.removeStateAsync(i,"specified",void 0,t.stateIds)…}`;
  @1329962 `removeStateAsync` posts `nG` to `this.links[I.LINKS.REMOVE_STATE]`.
- **Status:** read-from-bundle.

### 4.4 A moderator lowers someone's hand, or every hand

- **Method / link:** POST to `conversation_links.removeState`.
- **Bodies:** one person: `{from, sequenceNumber, scope:"specified", stateIds:[<their raiseHands stateId>]}`;
  everybody: `{from, sequenceNumber, scope:"all", stateType:"raiseHands"}`. The page sends MRIs; the
  backend resolves each stateId from the roster at press time, so the page never handles state ids.
- **Bundle evidence:** @1805179 `removeStatesForEveryone(t,i){const r=gT(t.type);return this.signalingSession.removeStateAsync(i,"all",r,void 0)…}`;
  `calling-roster.js` declares `mutation lowerAllRaisedHands($teamsCallId:ID!)` and
  `mutation lowerOtherParticipantsHand($teamsCallId:ID!,$participantId:ID!)`.
- **Status:** bodies read-from-bundle. Which builder each mutation uses is INFERRED (its resolver
  was not downloaded).

### 4.5 Read the hand queue

- **Method:** read `/conversation/rosterUpdate/` (`roster_in_frame`, `src/calling.rs:2037`).
- **Shape:** `participants[mri].publishedStates[{stateType, content, stateId, typeRank}]` at
  participant level, and `endpoints[ep].publishedStates[…]` at endpoint level.
- **Rule:** meeting-awareness § 4.5 rule 8 owns the reading. It sorts by `typeRank` and shows an
  ordinal position ("2nd"), never the raw value. This workstream adds `hand_state_id` to the held
  member (backend only) and `my_hand_position` to `call_state`.
- **Bundle evidence:** @944400 `processPublishedStates(t,i){…o={id:i,publishedState:{content:s.content,stateId:s.stateId,typeRank:s.typeRank}}…}`;
  `fromRosterEndpoints` keeps `publishedStates:t[o].publishedStates||null` per endpoint;
  `calling-roster.js` @45618 `const fe="raiseHandsOrderedRank"in N?"raiseHandsOrderedRank":"raiseHandsRank",_e=N[fe],Ee=k[fe];return _e===Ee?0:…_e-Ee`.
- **Status:** wire fields read-from-bundle. `raiseHandsOrderedRank`/`raiseHandsRank` are GraphQL
  fields, so ordering by `typeRank` is INFERRED. The key is absent from the measured roster skeleton
  (NATIVE-CALLING.md:1053-1063), presumably because nobody had a state then; M0 measures it.

### 4.6 Send a live reaction

- **Method / link:** POST to `conversation_links.sendMessage` (measured present in a joined
  meeting's answer, NATIVE-CALLING.md:226).
- **Body:**

```json
{
  "from": { "displayName": "…", "endpointId": "…", "id": "8:orgid:…", "languageId": "en-us", "participantId": "…" },
  "messageContent": [
    { "operationId": "<uuid>", "payload": "<UNMEASURED, from M0>", "scope": "all", "to": [], "type": "Reaction",
      "links": { "sendMessageStatus": "<our /conversation/sendMessageStatus/, fresh cause>" } }
  ]
}
```

  The client omits `links.sendMessageStatus` for a Reaction when `enableSendReactionMessageOptimization`
  is on. Sending it costs nothing, and the status frame may never arrive.
- **Bundle evidence** (`calling-pluginless.js`):

```text
@1158769 function UV(e,t){…const n=r.scope===0?"all":"specified",s=[];…const a={operationId:r.operationId,payload:r.payload,scope:n,to:s,type:r.type};_T.includes(r.type)&&ET(r.type,e.signalingAgentConfig)||(a.links={sendMessageStatus:lt(e,We.SEND_MESSAGE_STATUS)}),i.push(a)}return{payload:{from:{displayName:…,endpointId:…,id:…,languageId:…??"",participantId:…},messageContent:i}}}
@1159863 var _T=["Reaction","LiveInking"]
@1159896 function ET(e,t){switch(e){case"Reaction":return!!t.enableSendReactionMessageOptimization;…
@1314458 sendProxiedMessageAsync(e,t){…this.links[I.LINKS.SEND_MESSAGE]?this.proxiedMessageNotificationManager.sendProxiedMessageAsync(e,t,this.links[I.LINKS.SEND_MESSAGE]):Promise.reject(…"missing url for CONSTANTS.LINKS.SEND_MESSAGE"…)
@929766 SEND_MESSAGE_STATUS:"/conversation/sendMessageStatus/"
```

  `calling-app.js` names five reaction commands: `MeetingLikeReaction`, `MeetingHeartReaction`,
  `MeetingApplauseReaction`, `MeetingLaughReaction`, `MeetingSurprisedReaction`. Its renderer reads
  `reactionType` and `skinTone`.
- **Status:** envelope and link: read-from-bundle; the link is measured present. `payload`
  is UNMEASURED; M3 waits for M0.

### 4.7 Receive live reactions

- **Method:** read `/conversation/receiveMessage/` (already published, `src/calling.rs:1233`).
- **Shape:** `{messageContent:[{operationId, type, payload, from:{id, participantId, endpointId}}]}`,
  or a single such object as the body.
- **Rule:** only `type == "Reaction"` is acted on. One sent from THIS endpoint
  (`from.endpointId == local.endpoint_id`) is dropped, because our own chip is drawn from our RPC's
  success. A reaction from the user's other device (same mri, other endpoint) is shown. It is
  emitted as an ephemeral `call_reaction`, never stored.
- **Bundle evidence:** @1326462 `handleReceiveMessage(e){…Array.isArray(e.body?.messageContent)?i=e.body.messageContent:i=[e.body];…n.push({operationId:a.operationId,messageType:a.type,payload:a.payload,from:{[a.from.id]:{participantLegIdMap:{[a.from.participantId]:{...a.from.endpointId&&{endpointId:a.from.endpointId}}},level:"endpoint"}}})…`
- **Status:** read-from-bundle; payload UNMEASURED.

### 4.8 Admit one person from the lobby

- **Method / link:** POST to `conversation_links.admit` (measured present for the organizer,
  NATIVE-CALLING.md:226).
- **Preconditions (the client's own):** the call is CONNECTED and holds a group modality. The person
  is in the lobby now: some endpoint has `lobby` and none has `call`, which is meeting-awareness
  § 4.5 rule 5.
- **Body:**

```json
{
  "participants": { "from": { … }, "to": [ { "id": "<their mri>", "displayName": "<roster details.displayName>" } ] },
  "links": { "admitFailure": "<…/conversation/admitParticipantFailure/, fresh cause>",
             "admitSuccess": "<…/conversation/admitParticipantSuccess/, fresh cause>" },
  "debugContent": { "causeId": "<that cause>" }
}
```

- **Callbacks:** `/conversation/admitParticipantSuccess/` and `/conversation/admitParticipantFailure/`.
  Each is matched to its `PendingOp` by the cause segment in the frame URL. The op expires after 210 s
  (the client's `admitParticipantTimeoutSec:210`). An expired op is settled from the roster: if the
  person is now in the call, that is a success; otherwise "Teams did not say whether {name} was let
  in".
- **Bundle evidence:**

```text
@1094207 function AV(e,t,i){…{payload:{participants:{from:{…},to:[{id:t.id,displayName:t.displayName}]},links:{admitFailure:lt(e,We.CONV_ADMIT_PARTICIPANT_FAILURE),admitSuccess:lt(e,We.CONV_ADMIT_PARTICIPANT_SUCCESS)},debugContent:{causeId:i}}}}
@1248445 this.admitParticipantAsync=(a,o=Ht())=>(…Ni(this.fsmState===I.SIGNALING_FSM_STATE.CONNECTED,"remoteParticipants can only be admitted to an ongoing call"),this.hasGroupModality()||Ni(!1,"threadId/groupId must be set before admitting more participants to connected call"),…this.participantManager.admitParticipantAsync(a,this.links[I.LINKS.ADMIT],o))
@928368 CONV_ADMIT_PARTICIPANT_SUCCESS:"/conversation/admitParticipantSuccess/",CONV_ADMIT_PARTICIPANT_FAILURE:"/conversation/admitParticipantFailure/",CONV_ADMIT_ALL_STATUS:"/conversation/admitAllStatus/"
```

- **Status:** body read-from-bundle; link measured present.

### 4.9 Admit everybody in the lobby

- **Method / link:** POST to `conversation_links.admitAll` (measured present).
- **Body:**

```json
{ "participants": { "from": { … } },
  "links": { "admitAllStatus": "<…/conversation/admitAllStatus/, fresh cause>" },
  "operationId": "<cause>", "debugContent": { "causeId": "<cause>" } }
```

  It carries NO list: the service admits whoever is in the lobby when the POST lands. The count shown
  before the press can therefore understate it, and the confirmation says so (§ 7).
- **Callbacks:** `/conversation/admitAllStatus/`; the op expires after 60 s (`admitTimeoutSec:60`).
- **Bundle evidence:** @1167405 `function GV(e,t){…{payload:{participants:{from:{…}},links:{admitAllStatus:lt(e,We.CONV_ADMIT_ALL_STATUS)},operationId:t,debugContent:{causeId:t}}}}`;
  @1304866 `sendNetworkRequestForAdmit(e){…sendPostRequest({url:this.links[I.LINKS.ADMIT_ALL],requestName:dt.ADMIT.name,payload:GV(this,e)…`.
- **Status:** read-from-bundle; link measured present.

### 4.10 Mute one participant, or everyone else

- **Method / link:** POST, synchronous, to `conversation_links.mute` (measured present). Today's
  self-mute body uses the same link (`src/calling.rs:1559`). incoming-and-conversation ›
  `self-mute-not-visible-in-meetings` moves self-mute to `updateEndpointState`, after which this link
  carries only these bodies. M6 lands after that fix.
- **Body:**

```json
{ "from": { … }, "scope": "specified", "muteParticipants": [ { "id": "<mri>" } ], "mediaTypes": ["audio"] }
```

  Everyone else: `scope:"all"` with every OTHER present participant's mri in `muteParticipants`
  (INFERRED: the app-level caller `muteAllParticipants($scope:MuteAllScope)` was not read). The
  answer is `muteUnmuteResponse`, inline.
- **Gate:** the client's `canMuteOthers()`: no `attendee` role, on the participant or on our own
  endpoint, AND the `mute` link is present.
- **Bundle evidence:**

```text
@1095870 function IV(e,t,i,r=!1,n=null){…const s={from:{…},scope:t,muteParticipants:i,mediaTypes:["audio"]};return r&&(s.links={clientCallback:lt(e,We.MUTE_UNMUTE_ASYNC,n)},s.operationId=n),{payload:s}}
@1131087 this.muteAsync=(s,a,o,l)=>{…let c="specified";…case I.MUTE_SCOPE.EVERYONE_ELSE:E=dt.MUTE_ALL_SYNC,c="all",o.forEach(_=>{u.push({id:_})});break;case I.MUTE_SCOPE.SPECIFIED_PARTICIPANTS:…
@1799196 async muteParticipants(t,i,r=ws()){…i.forEach(o=>{a.push(o.id)}),this.signalingSession.muteAsync(s,a,r)…
@1935634 canMuteOthers(){const t=…this.meetingRoles?.some(r=>r==="attendee")||this.isSelfAttendee(),i=…endpointMeetingRoles?.some(r=>r==="attendee");return!t&&!i}
```

- **Status:** body and gate read-from-bundle; link measured present; the "all" participant list inferred.

### 4.11 Remove a participant

- **Method / link:** POST to `conversation_links.removeParticipant`. Whether it is present on this
  tenant is unmeasured; M0's `joinLinks`/`frameLinks` answer it.
- **Body (whole participant):**

```json
{ "participants": { "from": { … }, "to": [ { "id": "<mri>", "displayName": "<roster details.displayName>" } ] },
  "links": { "removeParticipantSuccess": "<fresh cause>", "removeParticipantFailure": "<fresh cause>" } }
```

  The endpoint-scoped body (`DV`, `toEndpoints:[{id, endpointId}]`) is not offered.
- **Callbacks:** `/conversation/removeParticipantSuccess/` and `/conversation/removeParticipantFailure/`;
  the op expires after 60 s (`removeParticipantTimeoutSec:60`) and is then settled from the roster.
- **Bundle evidence:** @1098792 `function OV(e,t){…participants:{from:{…},to:[{id:t.id,displayName:t.displayName}]},links:{removeParticipantSuccess:lt(e,We.CONV_REMOVE_PARTICIPANT_SUCCESS),removeParticipantFailure:…}`;
  @1122598 `this.removeParticipantAsync=(s,a,o)=>{…$i(a,"correct removeParticipantUrl is not provided")…`;
  @928566 `CONV_REMOVE_PARTICIPANT_SUCCESS:"/conversation/removeParticipantSuccess/",CONV_REMOVE_PARTICIPANT_FAILURE:"/conversation/removeParticipantFailure/"`.
- **Status:** read-from-bundle; link presence unmeasured.

### 4.12 Change a participant's role

- **Method / link:** POST to `conversation_links.updateParticipantRole` (presence unmeasured).
- **Body:**

```json
{ "from": { "id": "…", "endpointId": "…", "participantId": "…", "languageId": "en-us" },
  "scope": "specified", "to": [ { "id": "<mri>", "meetingRole": "presenter" } ] }
```

  `meetingRole` is `"presenter"` or `"attendee"`. `"organizer"` is valid in the client but not offered.
- **Callbacks:** none. The effect shows in the roster; the client waits 15 s
  (`updateMeetingRoleTimeoutSec:15`).
- **Bundle evidence:** @1099895 `function LV(e,t,i,r=""){st(i&&i==="specified","Only non-null non-empty scope of 'specified' is supported.");…t.forEach(s=>{const a={id:s,meetingRole:r};n.push(a)}),{payload:{from:{id:…,endpointId:…,participantId:…,languageId:…},scope:i,to:n}}}`;
  @1117690 `updateMeetingRolesAsync=(s,a,o,l=Ht())=>{…a==="attendee"?u=dt.UPDATE_MEETING_ROLE_ATTENDEE.name:a==="presenter"?u=dt.UPDATE_MEETING_ROLE_PRESENTER.name:a==="organizer"?u=dt.UPDATE_MEETING_ROLE_ORGANIZER.name:c.reject(…)`.
- **Status:** body and role strings read-from-bundle.

### 4.13 Lock or unlock the meeting

- **Method / link:** PUT to `conversation_links.updateMeetingSettings` (presence unmeasured).
- **Body:** `{ "from": { … }, "meetingSettings": { "lockMeeting": true }, "sequenceNumber": 3 }`. This uses
  the same counter as § 4.2. The builder also carries `allowRaiseHands`, `attendeeRestrictions`,
  `breakoutRoomsEnabled` and others, none of which is offered.
- **Callbacks:** none. The client ignores the answer; the effect shows on the next snapshot
  (`meetingDetails.meetingCapability`, measured by M0).
- **Bundle evidence:** @1192542 `function pG(e,t){const i={};…t.lockMeeting!==void 0&&(i.lockMeeting=t.lockMeeting),…{payload:{from:{…},meetingSettings:i,sequenceNumber:e.getPublishStateSequenceNumber()}}}`;
  @1331523 `updateMeetingSettingsAsync(e,t){…const i=this.links[I.LINKS.UPDATE_MEETING_SETTINGS];…return this.http.sendPutRequest({url:i,…payload:pG(this,e)…`.
- **Status:** read-from-bundle. Whether a lock outlives the conversation ending is unmeasured (§ 9 L4).

### 4.14 End the meeting for everyone

- **Method / link:** DELETE with a JSON body, to `conversation_controller` (the snapshot's
  `conversationController`).
- **Gate:** our own role is `organizer` or `coorganizer`, and the controller is known.
- **Body:**

```json
{ "participants": { "from": { … } },
  "conversationTransactionEnd": { "reason": "noError", "code": 0, "phrase": "ConversationEndForAllInitiated" },
  "callTransactionEnd": { "code": 0, "subCode": 0, "phrase": "ConversationEndForAllInitiated", "resultCategories": ["Success"] } }
```

- **Order (review fix):** first copy the held `leave`/hangup link and `local`; then DELETE. On success,
  `end_call_locally` with the ended-for-everyone reason meeting-awareness names
  (`named_meeting_end_reason`). On failure, post the ordinary hangup on the copied link first, then
  `end_call_locally(END_REASON_END_FOR_ALL_FAILED)`, which `callEndLabel` maps to "Teams did not end
  the meeting for everyone, so it goes on without you." `end_call_locally` drops the session
  (`src/bin/server.rs:15106`), so the hangup cannot come after it.
- **Bundle evidence:** @1271184 `this.endAsync=(a,o={})=>{…if((this.fsmState===I.SIGNALING_FSM_STATE.CONNECTED||c)&&o.forEveryone)return this.deleteConversation(a,l);`;
  @1357490 `deleteConversationController(e,t){return this.convJoined?(…this.http.sendDeleteRequest({url:this.links[I.LINKS.CONVERSATION_CONTROLLER],requestName:dt.DELETE_CONVERSATION.name,payload:YV(this,e),withoutTrouter:!0…`;
  @1179020 `function YV(e,t){…{payload:{participants:{from:{…}},conversationTransactionEnd:{reason:"noError",code:I.CALL_END_CODE.SUCCESS,phrase:I.CALL_END_PHRASE.CONV_END_FOR_ALL_INITIATED},callTransactionEnd:t}}}`.
- **Status:** read-from-bundle.

### 4.15 Join a group-chat call that is already going on

- **Method / link:** POST to `calling_conversationServiceUrl`, as a meeting join is.
- **Body:** the existing `join_payload` (`src/calling.rs:1764`) with
  `groupChat:{threadId:<19:…@thread.v2 group chat>, messageId:null}` and no `meetingInfo` or
  `meetingData`. It carries NO `participants.to`, plus `conversationRequest.suppressDialout:true`.
- **What actually stops a ring.** In the client, ringing a thread's members is driven by the THREAD
  and switched off by `suppressDialout`. The absence of `to` is not a rail. `suppressDialout` is the
  only one, and it is unmeasured on a group-chat thread.
- **Bundle evidence:**

```text
@1179919 function gM(e,t,i,r,n,s=!1){…c=e.threadId?{threadId:e.threadId,messageId:e.teamsMessageId||null}:null,u={payload:{conversationRequest:{conversationType:r.conversationType,subject:e.convSubject,suppressDialout:r.suppressDialout,…},…groupChat:c,…
@1256206 this.joinGivenConversation=(a,o,l,c,u,E={},m=Ht(),v=!1)=>{…const y={newCall:!1,suppressDialout:!0,conversationServiceUrl:a,…
@1742926 this.getSignalingSessionCallOptions=P=>{const q={suppressDialout:!P.callStartOptions.ringOthers,…
@1833086 async join(t,i={},r=Ie()){…this.setCallerMri(Fm(t.groupCallInitiator))…this.joinOrStartCall({…},this.joinGivenConversation,r,t.conversationUrl)}
@1374022 const o=this.getConversationUrl()?this.getConversationUrl():e.conversationServiceUrl;…
```

- **Status:** body read-from-bundle. INFERRED: that a POST to `calling_conversationServiceUrl` with the
  thread joins the live call. The client's `join(t)` posts to a caller-supplied `t.conversationUrl`,
  and the chunk that supplies it was not downloaded. M0 looks for where that URL comes from
  (trouter `evt` 115 `ACTIVE_CALL_STARTED_NOTIFICATION` / 116 `NO_ACTIVE_CALLS_NOTIFICATION`, @822860,
  or the started event's properties). If M0 finds it, the join posts there instead. UNMEASURED live.
- **Join that finds no call.** The body carries `allowConversationWithoutHost:true` (the verified join's
  value). A join to a call that has just ended may therefore start a new hostless call and post
  "Call started" into the chat for every member. Nobody is rung. The button says so (§ 7).
  Sending `false` instead is an unmeasured change and is an open question (§ 12), not this design.

### 4.16 The 409 redirect, for a group-chat JOIN only

- **Frame:** `/call/end/` with `callEnd:{code:409, conversationUrl:{Location}}` while a group JOIN is
  establishing.
- **Rule:** re-POST the SAME join body (the held offer) to `Location`, at most once. It runs only
  while the phase is `Connecting` and no acceptance or media answer has arrived. `Location` must be
  https with a host ending in `.teams.microsoft.com` (every calling endpoint on this tenant does,
  NATIVE-CALLING.md:369-376). The redirected join gets a FRESH `CallbackBase.cause_id`. The old cause
  goes into `abandoned_causes`, and `handle_call_frame` drops any frame whose URL carries
  `callAgent/{session}/{abandoned cause}/` before anything else reads it. That stops a late
  `conversationEnd` from the first attempt ending the redirected one. The branch sits BEFORE the
  generic `Links::collect`/merge (`src/bin/server.rs:14682`) and returns, so the 409 frame's links
  never enter `call.links`.
- **A placed group call is NOT redirected.** If "Call everybody" meets a 409, the call ends as today.
  The conversation is marked as having a call going on (§ 5, `ongoingGroupCalls`), and a notice says
  "A call is already going on in {chat}". Its action (meeting-awareness `Notice.action`) is "Join it",
  which is a new press of the join.
- **Bundle evidence:** @1389873 `if(i.code===I.CALL_END_CODE.CONFLICT&&this.multiParty&&i.conversationUrl?.Location)this.handleConversationResolutionConflict(i,r);`;
  @1391850 `handleConversationResolutionConflict(e,t){…this.subscribeToCall(e.conversationUrl.Location,…` /
  `r.scenario="409RedirectJoin"…`. teams-lite's capability mask 73463 has bit 32 (`autoJoinOnConflict`)
  set (`src/calling.rs:41`), so the service may join directly instead.
- **Status:** read-from-bundle; UNMEASURED. It is the one act in this design that follows a frame
  rather than a press, stated in § 6 as an exception.

### 4.17 Join a channel meeting from its post

- **Preferred address:** a long join link in the post (`/l/meetup-join/{channel}/{message}?context=…`),
  joined through the existing `from_join_url` (`src/calling.rs:1609`; `is_channel_meeting` `:1712`).
  It carries `meetingInfo`, which is what a stock client's link carries.
- **Fallback address (preview only):** `groupChat:{threadId:<19:…@thread.tacv2>, messageId:<root id>}`
  with no `meetingInfo`. The root id is a digit string of at most `MAX_THREAD_ROOT_CHARS`
  (`src/teams_send.rs:262`). It comes from the call event's `thread_root_id`, which requires the read
  fix in § 5 (`src/teams_read.rs`). An event with an empty root is never offered a Join.
- **Join that finds no meeting.** The meeting may restart under that post (`allowConversationWithoutHost:true`),
  as rejoining any ended Teams meeting does. The button says so.
- **Bundle evidence:** `gM`'s `groupChat:{threadId:e.threadId,messageId:e.teamsMessageId||null}` (§ 4.15);
  `updateGroupChatIds` tracks `teamsMessageId`.
- **Status:** read-from-bundle; UNMEASURED live (there is no sandbox channel).

## 5. Changes by file

**`src/teams_read.rs`**
- The call-event branch (`:1658-1680`) calls `parse_thread(m)`, as the meeting-activity branch does at
  `:1690`, and stores `thread_root_id`/`thread_subject`. Rows that already exist heal on the next re-sync:
  the upsert backfills an empty `thread_root_id` (`src/store.rs:3491`, `:3514`). Legacy rows whose
  content was cleared cannot heal without a re-sync, and they are never offered a Join.
- Test: a channel `Event/Call` frame carrying `conversationLink …;messageid=<root>` keeps its root, and
  a chat `Event/Call` keeps an empty one.

**`src/calling.rs`**
1. `paths`: `CONVERSATION_ADMIT_PARTICIPANT_SUCCESS/FAILURE`, `CONVERSATION_ADMIT_ALL_STATUS`,
   `CONVERSATION_REMOVE_PARTICIPANT_SUCCESS/FAILURE`, `CONVERSATION_SEND_MESSAGE_STATUS`, spelled as
   the bundle's table (@928368, @929766). They are published only inside the bodies that use them, so
   `conversation_callback_links` (`:1221`) stays byte-identical.
2. `CallbackBase::with_cause(&self, cause: String) -> CallbackBase`.
3. `Links` accessors (exact names): `publish_state`, `remove_state`, `send_message`, `admit`,
   `admit_all`, `remove_participant`, `update_participant_role`, `update_meeting_settings`. They read
   `conversation_links` only (meeting-awareness's `conversation_links_in`, `conversation_sequence`).
4. Transport: **this is plane-reliability M4's one transport API, consumed here, not a second one**
   (README § 3, collision 7; M4 lands before this M1). The extraction described here is what that
   API is built on: the header set and the reply handling come out of `post_signal` (`:2188`) into
   private `signal_headers(builder, region, ic3, correlation_id)` and `read_signal_reply(response,
   url)`; `post_signal` becomes `signal_headers(http.post(url)…).json(payload)` plus
   `read_signal_reply`; and the siblings `put_signal` and `delete_signal` sit beside it, all three
   taking plane-reliability's `SignalClass`. If plane-reliability M4 has landed it, this item adds
   nothing; if not, this design's extraction is the shape it lands in. The source-scan header test
   (`a_calling_request_carries_one_credential_and_no_api_version`, `:3311`) is kept, retargeted at
   `signal_headers`. A new behavioural test builds a POST, a PUT and a DELETE through `signal_headers`
   with reqwest's `RequestBuilder::build()` and asserts all three carry the SAME header-name set as
   today's POST: no `X-Skypetoken`, no `api-version`. A source scan asserts `delete_signal` has
   exactly one caller (`call_end_for_everyone`).
5. Pure builders returning envelope-free contents: `publish_state_payload(local, "raiseHands", level,
   content, seq)`, `remove_state_payload(local, seq, Removal::Ids(Vec<String>) | Removal::AllOfType(&str))`,
   `send_reaction_payload(local, operation_id, payload, status_link)`, `admit_payload(local, mri,
   roster_name, callbacks)`, `admit_all_payload(local, callbacks)`, `mute_participants_payload(local,
   MuteScope::Specified(Vec)|All(Vec))`, `remove_participant_payload(local, mri, roster_name,
   callbacks)`, `update_role_payload(local, mri, Role::Presenter|Attendee)`,
   `meeting_settings_payload(local, LockMeeting(bool), seq)`, `end_for_everyone_payload(local)`.
6. `receive_messages_from_frame(&Value) -> Vec<ProxiedMessage{operation_id, kind, payload, from_mri,
   from_endpoint}>` (both shapes).
7. `conflict_redirect_from_frame(&Value) -> Option<String>` (code 409 plus `Location`) and
   `is_calling_host(url) -> bool` (https, host ends with `.teams.microsoft.com`).
8. `MeetingJoin` gains `kind: JoinKind{Meeting, GroupChat, ChannelPost}` and `suppress_dialout: bool`.
   Every existing constructor sets `Meeting` and `false`. New `for_group_chat(thread)` refuses a
   `19:meeting_` id and a channel id. New `for_channel_post(channel, root)` requires
   `teams_read::is_channel_thread_id` and digits. `join_payload` writes `groupChat.messageId: null` for
   `GroupChat` only, and `conversationRequest.suppressDialout: true` only when `suppress_dialout`.
9. Tests:
   - one per builder, pinning field names and absences against § 4: no `scope`/`to` on one's own
     hand; `stateIds` vs `stateType`; reaction `scope:"all"`, `to:[]`, `type:"Reaction"`; admit
     `to:[{id,displayName}]`; role `from` without `displayName`; lock plus `sequenceNumber`; the phrase
     written twice in end-for-everyone;
   - **golden joins:** the meeting-thread join, the link join and the invitation are byte-identical to
     a JSON fixture captured from today's builders. That fixture set is video-send M1's golden pins
     (README § 3, collision 4), landed once there; this design EXTENDS it with the cases below and
     pins no second copy of the same bodies; the same joins with `suppress_dialout` differ by
     exactly that one field; a group join has `messageId:null`, `suppressDialout:true` and no
     `participants.to`; a channel-post join has `messageId:"<digits>"`;
   - `conflict_redirect_from_frame` needs both 409 and `Location`; `is_calling_host` refuses http,
     another domain and a userinfo trick (`https://x.teams.microsoft.com@evil.example/`);
   - `receive_messages_from_frame` handles both shapes.

**`src/calling.rs` (roster — consumed from meeting-awareness § 4.5, not re-implemented)**
- From that design: `RosterMember.role`, `meeting_roles`, per-endpoint `in_lobby`/`in_call`, `hand_rank`,
  the lobby rule (some endpoint `lobby` and none `call`), and names resolved outside the calling lock.
- This workstream adds `hand_state_id: Option<String>` (the raiseHands `stateId`, participant or
  endpoint level) and `own_endpoint_roles` (our endpoint's `endpointMeetingRoles`) for `canMuteOthers`.
  Bodies take `RosterMember.display_name` (the roster's own); UI copy takes the resolved label.

**`src/bin/server.rs`**
1. `OUTWARD_METHODS` (`:367`) grows to `[&str; 33]` with `call_raise_hand`, `call_lower_hands`,
   `call_react`, `call_admit`, `call_admit_all`, `call_mute_participants`, `call_remove_participant`,
   `call_set_participant_role`, `call_set_meeting_lock`, `call_end_for_everyone`, with a doc paragraph
   per entry. `MACHINE_METHODS` gains `set_meeting_preview`.
2. `ParticipationAct` enum (the ten acts plus `GroupJoin`, `ChannelJoin`, `JoinSuppressDialout`).
   `const PROVEN_PARTICIPATION: &[ParticipationAct] = &[];`. `fn act_offered(store, act) -> bool`
   returns PROVEN contains it OR the `meeting_preview` setting is on. A store this backend cannot read
   counts as off.
3. `CallSession` (`:2229`) gains `publish_sequence: u64`, `my_hand_state_id: Option<String>`,
   `pending_ops: Vec<PendingOp>` (bounded 16), `join_offer: Option<MediaContent>`, `redirected: bool`,
   `abandoned_causes: Vec<String>` and `recent_reactions: VecDeque<i64>`. `conversation_links`,
   `conversation_controller` and `conversation_sequence` come from meeting-awareness, extended to
   group joins and to the gated join answer (§ 4.1).
4. `CallSession::json` (`:2341`) adds `my_hand_position`, `self_roles`, `meeting_preview` and the
   capability flags `can_raise_hand`, `can_react`, `can_admit`, `can_moderate`, `can_remove`,
   `can_set_roles`, `can_lock`, `can_end_for_everyone`. Each flag is decided here from:
   `act_offered` AND the `conversation_links` entry AND our role AND phase `Connected`. `can_moderate`
   is the client's `canMuteOthers`. `can_end_for_everyone` requires organizer or coorganizer and a
   known controller. The preview flag is READ when the call state is built, so `json()` does not open
   the store under the calling lock: `meeting_preview` is cached on `Ctx` and refreshed by
   `set_meeting_preview`.
5. `fn participation_refusal(&CallSession, act, target: Option<&str>) -> Option<String>`, pure. It
   refuses before the network, with a sentence, when:
   - there is no such call, or it is not Connected;
   - the act is not offered;
   - the link is absent;
   - our role is attendee (for moderation);
   - the target is not in the roster now, or is ourselves (remove, mute-other, role);
   - an admit target is not in the lobby;
   - admit has no group modality;
   - end-for-everyone lacks the organizer role;
   - the reaction rate floor is hit (5 per 10 s).
6. Handlers for the ten RPCs: re-read the session under the lock, run `participation_refusal`, release
   the lock, post through `post_signal`/`put_signal`/`delete_signal`, and answer
   `{call_id, told_service, …}`. The state shown is the roster's, never optimistic.
7. `set_meeting_preview {enabled: bool}`: refused read-only, writes the setting, updates `Ctx`'s cache,
   emits `call_state` and `settings_changed`.
8. `handle_call_frame` (`:14601`). New URL-matched branches BEFORE the generic merge (`:14682`), each of
   which returns:
   - the abandoned-cause drop;
   - `receiveMessage` → `call_reaction {call_id, from_mri, from_name, reaction, at_ms}`;
   - `admitParticipantSuccess/Failure`, `admitAllStatus`, `removeParticipantSuccess/Failure` →
     settle the matching `PendingOp`, emit `call_operation {call_id, op, ok, name, reason}`;
   - `sendMessageStatus` → journal only;
   - the 409 redirect (§ 4.16) for a group-JOIN reservation.

   `conversationUpdate` snapshot handling is meeting-awareness's branch, extended to group joins.
   A source-scan test pins that each new branch precedes the merge and returns, so
   `lobby_state_in_frame`'s unscoped walk (`:14746`) never reads these frames.
9. `meeting_address` (`:10149`):
   - accepts `group_call_thread`, and `meeting_channel_thread` + `meeting_message_id`;
   - REFUSES a params object naming more than one of `join_url`, `meeting_thread`,
     `group_call_thread`, `meeting_channel_thread`;
   - refuses `GroupJoin`/`ChannelJoin` unless `act_offered`;
   - sets `suppress_dialout` on a meeting/link join only when `JoinSuppressDialout` is offered
     (preview on).
   An old backend given an unknown address param falls through to `Ok(None)` and the placing branch,
   which then needs `conversation` and refuses; it never rings.
10. `call_prepare` (`:7401`), group-call branch:
    - `CallKind::Group`, ring empty, `peer_name` from the store's chat title;
    - while `GroupJoin` is unproven, fetch the thread roster exactly as the placing branch does
      (`teams_members::fetch_thread_members`) and refuse above `MAX_GROUP_CALL_PEOPLE`, because the
      failure mode being guarded is an accidental dial-out;
    - refuse when a call is Ringing for that conversation, and point at Answer.
11. `call_join` (`:7740`) admits `CallKind::Group` when `ring.is_empty()` and keeps `join_offer`.
12. Tests:
    - `every_calling_method_that_reaches_a_person_is_outward_facing` (`:17137`) lists all 19 calling
      OUTWARD methods;
    - `participation_refusal` per refusal;
    - the branch-order scan;
    - `can_*` false when the link is absent, the role is attendee, or the act is neither proven nor in
      preview;
    - `meeting_address` refuses two addresses;
    - a group join never reaches `invitation_payload`;
    - a demotion (a snapshot without `admit`) removes `can_admit`;
    - the existing `every_gated_method_is_named_in_the_automation_guard` (`:17954`) and
      `every_gated_method_is_called_with_the_write_token` (`:18015`) force the guard and ws-client
      changes.

**`.claude/hooks/guard-live-automation.sh`** (+ `guard-live-automation.test.py`)
- Name the ten methods and `set_meeting_preview` in BOTH alternations of the write-RPC scan (`:981`),
  with a comment paragraph saying what each reaches.
- New example rule `drives_the_calling_plane`. An example is refused, whatever it pins, when it names:
  - `place_call`, `invitation_payload`, any builder in § 5 item 5, `put_signal`, `delete_signal`, or
    `signal_headers` — after `calling::`, or bare after a `use teams_lite::calling` import;
  - any moderation body key literally (`muteParticipants`, `publishedState`, `stateIds`,
    `messageContent`, `admitFailure`, `admitAllStatus`, `removeParticipantSuccess`, `toEndpoints`,
    `meetingSettings`, `ConversationEndForAllInitiated`, `"meetingRole"`).
  `meeting_join_probe` (`join_meeting` plus a `post_signal` leave, pinned by `MEETING_SUBJECT`) and
  `meeting_join_recon` stay open. It is a tripwire, and the guard's own header says what it cannot see.
- `join-live … --end-for-all` is refused for tooling, like `teams-lite-service.sh update --now`
  (`:2182`). The user types it.
- Python tests cover both halves: blocked spellings (including `place_call` beside the pinned meeting
  constant, and a hand-built `muteParticipants` body over `post_signal`), plus the two existing
  examples and a `grep` naming a builder staying open.

**`web/src/lib/call.ts`**
- `ActiveCall` gains `my_hand_position`, `self_roles`, `meeting_preview` and the `can_*` flags, all
  optional, so an older backend reads as "cannot".
- `MeetingAddress` gains `{kind:"group_call", thread}` and `{kind:"channel_meeting", thread, messageId}`.
- `MEETING_REACTIONS` table (`like` 👍, `heart` ❤️, `applause` 👏, `laugh` 😂, `surprised` 😮). The wire
  key per reaction is filled in from M0, and the table is the one place to correct it.
- Pure helpers `handQueue`, `lobbyWaiters` and `moderationOffered(call, person)`.

**`web/src/lib/call-stage.ts`**
- Reads meeting-awareness's `roster` field for rows.
- New pure helpers `callStageHands`, `callStageLobby`, `callStageModeration`, and copy helpers for the
  armed states and the refusal sentences.

**`web/src/lib/ws-client.ts`**
- `writeRequest` wrappers for the ten RPCs and `setMeetingPreview`.
- `meetingParams` (`:219`) gains the two new address kinds and still sends exactly one.

**`web/src/lib/store.ts`**
- Controller actions mirroring `setCallMuted` (`:3340`): call the backend, report a refusal with
  `showNotice` under `CALL_NOTICE` (`:2650`) in the user's words.
- `on("call_reaction")`: a bounded list (12, 4 s each), dropped at call end.
- `on("call_operation")`: a notice naming the person on failure or an unknown outcome.
- `ongoingGroupCalls: Map<conversationId, {startedAtMs}>`:
  - set by a LIVE `call` awareness event `started` for a group chat or channel;
  - cleared by `ended`/`missed`, by our own join ending, and after `ONGOING_CALL_TTL_MS` (2 h);
  - NOT derived from stored history until M0's store recon shows whether "ended" is a separate
    message (the store freezes `system_event`, `src/store.rs:3496`).
  The live path parses each frame (`src/bin/server.rs:15348`), so an edited ended frame still clears it.

**`web/src/lib/channel-meeting.ts`** (new, pure) + `channel-meeting.test.ts`
- `channelMeetingOf(threadMessages)` returns `{link}` when the thread's root carries a meetup-join
  link. Otherwise, only under preview, it returns `{thread, messageId}` when a `started` call event
  has a non-empty `thread_root_id` and no later `ended` within the TTL.

**`web/src/components/call-stage.tsx`**
- Header (`FullStage`, `:273`): Raise hand (`HandIcon`, `call-raise-hand`, pressed while raised) and
  React (`SmilePlusIcon`, `call-react`, a `DropdownMenu` so it inherits the 44 px rows and the
  window-width clamp), each drawn only when its flag is true.
- `StagePeople` (`:579`) gets three sections:
  - Waiting in the lobby: Admit per row, armed Admit all.
  - Raised hands: position badge; Lower, and an armed Lower all, for moderators.
  - In this call: a row menu (`MoreHorizontalIcon`, `call-participant-menu`) with Mute, Make a
    presenter / Make an attendee, and Remove from the meeting…; an armed "Mute everyone else" in the
    section header; a "Lock the meeting" switch at the foot.
- `LeaveButton` (`:885`) gains, for an organizer, a chevron menu with "End the meeting for everyone…"
  opening a confirmation dialog.

**`web/src/components/call-reactions.tsx`** (new)
- The picker items and `CallReactionBurst`: chips over the stage's bottom-left in an
  `aria-live="polite"` region. With reduced motion they appear and disappear with no float.

**`web/src/components/incoming-call-banner.tsx`** and **`conversation-menu.tsx`**
- A group chat or channel in `ongoingGroupCalls` gets a Join (`incoming-call-join-group` /
  `conversation-join-group-call`) carrying its address as `data-group-call-thread` (or
  `data-meeting-channel-thread` + `data-meeting-message-id`), drawn only when the act is offered.
- While the Join is drawn, the menu's "Call everybody" row is replaced; when the TTL lapses it comes
  back.
- While `call_state.call` is Ringing for that conversation, Answer is shown instead of Join.
- The banner comment at `:84-88`, which says such calls are not addressable, is rewritten.

**`web/src/components/message-pane.tsx`** (`ThreadGroup`)
- The thread card's foot row gains "Meeting in progress — Join" (`channel-meeting-join`) from
  `channelMeetingOf`.

**`web/src/components/meeting-preview-settings.tsx`** (new) + `settings-pane.tsx`
- The Settings row "Meeting controls in preview" (§ 7), beside Call recordings.

**`web/mock/server.ts`**
- `MockCall` gains the new fields. The `{kind:"meeting"}` hook takes: `role` (`organizer` |
  `presenter` | `attendee`), `lobby: [names | {guest:true}]`, `hands: [names]`,
  `reaction: {from, reaction}`, `ongoing_group_call: <conversation id>`,
  `channel_meeting: {thread, root}`, `missing_links: [names]` and `refuse: <rpc>`.
- `call_invite {reset:true}` resets all of it. The preview setting resets through the existing
  settings reset.
- Handlers for the ten RPCs and `set_meeting_preview` mirror every backend refusal in the backend's
  words and apply the effect to the mock roster on a delay. Admit moves the person after one tick;
  remove drops them; end-for-everyone ends with the ended-for-everyone reason.
- `call_prepare`/`call_join` accept the new addresses, and a hook-readable RPC counter lets a spec
  count `call_place` calls.
- `ongoing_group_call` emits the awareness `started` signal for an EXISTING low-traffic group fixture
  at test time (`Coffee Chat`, after confirming no spec reaches it by name or index), rather than a new
  seeded conversation, which would move the sidebar order that other specs index.
- The channel fixture's root id is 13 digits, so the mock can mirror the digits rule exactly.

**`web/scripts/preview.ts`** — `--meeting-participation` (§ 8).

**`web/scripts/join-live.ts`**
- `--watch` (acts on nothing):
  - adds `stateType`, `typeRank`, `meetingRole`, `participantType` and `reactionType` to
    `SKELETON_VALUES` (`:363`);
  - teaches `skeleton()` (`:384`) to print an ARRAY OF STRINGS as values for whitelisted keys
    (`meetingRoles`, `endpointMeetingRoles`);
  - adds a `participation` digest: `publishedStates` shapes per person; the CONTENT values in full
    for the user's OWN state, identified by the user's mri or this run's endpoint (theirs, not a
    colleague's), with string content parsed first; `receiveMessage` type plus payload skeleton,
    with our own payload printed in full; `localParticipantUpdate` skeleton; snapshot
    `sequenceNumber` presence; `meetingDetails.meetingCapability` keys; and any `evt` 115/116/128
    frame's keys.
- New outward modes `--pair --hand`, `--pair --react` and `--pair --lock`, plus a user-typed
  `--pair --end-for-all`. Immediately before each press they prove:
  - the pinned meeting, as today;
  - that `call_state.call.roster` holds no mri but the user's own;
  - that `participantCounts` (captured verbatim at `:979`) reports no more participants than this
    pair. The field is named after M0; unknown counts REFUSE.
  They undo on every path out (lower, unlock, then hang up both sessions) and print a loud line when
  an undo did not go out.

**Docs** — see § 12's decision D2 and the doc list in § 6.

## 6. Gating and consent

**New RPCs:** ten `OUTWARD_METHODS` entries (write token, refused read-only at the dispatch choke point,
never reached by anything automatic) and one `MACHINE_METHODS` entry.

| Method | Reaches | Reversible |
| --- | --- | --- |
| `call_raise_hand {call_id, raised}` | A state about the user, visible to everyone under their name | yes |
| `call_lower_hands {call_id, mris \| all}` | Others' visible state (moderator only) | no (queue order is lost) |
| `call_react {call_id, reaction}` | Everyone, transient; backend rate floor 5 per 10 s | n/a |
| `call_admit {call_id, mri}` | A person who then hears everything from that moment | half |
| `call_admit_all {call_id}` | The same, for everyone in the lobby when it lands | half |
| `call_mute_participants {call_id, mris \| all}` | Colleagues (each can unmute unless the meeting restricts attendees) | yes, by them |
| `call_remove_participant {call_id, mri}` | Disconnects someone | no |
| `call_set_participant_role {call_id, mri, role}` | Someone's permissions | yes |
| `call_set_meeting_lock {call_id, locked}` | Anyone trying to join | yes |
| `call_end_for_everyone {call_id}` | Everybody in the meeting | no |
| `set_meeting_preview {enabled}` (MACHINE) | Which unproven outward controls this machine offers | yes |

**Group-chat and channel joins** ride the existing `call_join` (OUTWARD). A group join carries
`suppressDialout:true` and no `participants.to`. While unproven it is also capped at
`MAX_GROUP_CALL_PEOPLE`, and offered only under preview.

**The preview switch.** `meeting_preview` is a store setting, so it is shared by every backend on the
machine, which is what the pair proofs need. It is off in a fresh store. It gates drawing AND the
backend: an act neither proven nor in preview is refused before the network with "This meeting control
is still in preview — turn on Settings › Meeting controls in preview to try it". A setting was chosen
over an environment variable because the user drives from a phone, and a unit edit is not a press
(decision D1).

**Rollout rule.** An act moves into `PROVEN_PARTICIPATION` by its own commit, after its live proof in
§ 9 is recorded in NATIVE-CALLING.md § 11 with the date. That commit is the "default" step the
project rule requires. `GroupJoin` and `ChannelJoin` additionally need the colleagues of the first
run to confirm nobody was rung.

**Read-only behaviour.** Every new RPC is refused before the network. No frame handler acts: a
reaction, an admit callback or a roster frame only emits. The mock mirrors every refusal.

**The one exception to "nothing acts on a frame"** is the 409 redirect (§ 4.16). It is a continuation
of the join the user just pressed, to the address the service named for that very join. It is bounded
to one, allowed only while establishing, host-checked, offered only with `GroupJoin` in preview or
proven, and never applied to a placed call. It is recorded in CLAUDE.md § Joining a meeting.

**Offer rule.** A control is drawn only where (a) the act is offered, (b) the service's own link is
in `conversation_links`, and (c) the user's role allows it. A control therefore never merely reports
a refusal. The backend re-checks all three at press time.

**Asking twice.** The irreversible acts on another person ask through a dialog that names them (the
store's ladder for the words, the mri for the target): remove and end-for-everyone. The bulk acts arm
first and then confirm with the count: admit all, mute everyone else, lower all hands.

**Guard.** The ten names and `set_meeting_preview` go in both alternations, pinned by
`every_gated_method_is_named_in_the_automation_guard`. `ws-client` uses `writeRequest`, pinned by
`every_gated_method_is_called_with_the_write_token`. The `drives_the_calling_plane` rule closes the
example hole, and `--end-for-all` is the user's to type.

**Docs to update** (in the change that lands each part):
- CLAUDE.md:
  - a new section "Taking part in a meeting (hands, reactions, the lobby, moderation)" under § A call
    is a PAGE;
  - § Audio calls: the list of methods that reach a person (19 calling OUTWARD entries), and why
    remove and end-for-everyone ask twice;
  - § Joining a meeting: the group and channel addresses, `suppressDialout`, the bounded 409 redirect
    as the stated exception, "Call everybody" replaced while a call is known, and the "A join rings
    nobody" bullet made conditional on `suppressDialout` for group threads;
  - § A call is a PAGE: the People panel sections;
  - § Automation safety: the new guard names, the example rule and `--end-for-all`;
  - § Sending messages: the MANDATORY `join-live` paragraph, widened to the new pair modes once the
    user authorizes them (D2).
- NATIVE-CALLING.md:
  - a new § 11, with every body tagged measured / read-from-bundle / inferred and the M0 values;
  - § 6 "deliberately NOT built" (:488-490);
  - the § 6 RPC table and the § 7 consent rows;
  - § 8 "Still open": the group and channel joins, switch-gated and unverified.
- Code comments: the `incoming-call-banner.tsx:84-88` comment; the `teams_read.rs` call-event comment;
  the `join-live.ts` usage header.

## 7. UI and copy (including mobile)

All of this is meeting-only unless the matching `can_*` flag says otherwise.

1. **Stage header.**
   - Raise hand: "Raise your hand" / "Lower your hand". Before the press its title is "Raise your hand
     — everybody in the meeting sees it". While raised: "Your hand is up — 2nd in line. Lower it".
   - React opens a menu titled "React — everybody sees it, with your name". It has five 44 px items:
     Like 👍, Heart ❤️, Applause 👏, Laugh 😂, Surprised 😮, with aria labels "React with Applause".
   - Incoming chips read "{name} 👏" for 4 s (aria "Ava Thompson reacted with Applause"); our own
     reads "You 👏".
2. **People panel.**
   - "Waiting in the lobby · {n}". Each row has "Admit", titled "Admit {name} — they hear the meeting
     from now on".
   - A guest gets a "Guest" badge. Its title, also a visible line on tap, reads "Not signed in to your
     organization — the name is whatever they typed".
   - "Admit all" arms to "Admit everyone waiting (now {n})", with the line "Everyone in the lobby when
     Teams receives this is let in. They hear everything said from now on, and nothing takes that
     back."
   - A non-moderator sees "{n} people are waiting in the lobby. An organizer or a presenter can let
     them in."
   - "Raised hands · {n}": position badges, "Lower" per row (aria "Lower {name}'s hand"), and
     "Lower all", which arms to "Lower {n} hands — the order they were raised in is lost".
   - "In this call · {n}": a muted glyph on every muted row and a "Presenter" / "Organizer" tag. The
     row menu "More for {name}" offers "Mute {name}", "Make a presenter" / "Make an attendee" and
     "Remove from the meeting…".
   - "Mute everyone else" arms to "Mute {n} people — each of them can unmute themselves, unless the
     meeting restricts attendees".
   - "Lock the meeting" switch: "Nobody new can join while it is locked. People already here stay."
3. **Remove dialog.** Title "Remove {name} from the meeting?"; body "They are disconnected now. Nothing
   in this app takes it back."; buttons [Cancel] [Remove].
4. **Leave.** An organizer gets a chevron with "End the meeting for everyone…". The dialog reads "End
   the meeting for everyone?" / "Everybody still in it — {n} people — is disconnected now. Nothing
   takes this back." / [Cancel] [End for everyone]. On failure: "Teams did not end the meeting for
   everyone, so it goes on without you: {reason}."
5. **Refusals and outcomes** are notices under `CALL_NOTICE`, never swallowed: "Teams did not take your
   raised hand: {reason}"; "The reaction did not reach the meeting: {reason}"; "Could not admit {name}:
   {reason}"; "Teams did not say whether {name} was let in"; "Could not remove {name}: {reason}"; and
   the preview refusal sentence (§ 6).
6. **Group chat with a call going on.**
   - The banner and the menu offer "Join the call", subtitle "Going on now · joining rings nobody — if
     it has just ended, this starts a new call in the chat". "Call everybody" is replaced while the
     call is known, and comes back after the TTL.
   - A 409 on a placed call gives "A call is already going on in {chat}", with the action "Join it".
7. **Channel.** The thread card's foot row reads "Meeting in progress — Join", subtitle "If it has
   ended, this starts it again in this post".
8. **Settings › Meeting controls in preview.** A switch, off by default, with the line: "Lets this app
   try meeting controls it has not yet proven against Teams: raising your hand, reactions, admitting
   people from the lobby, moderating, locking or ending a meeting, and joining a call already going on
   in a group chat or a channel. Each one still acts only when you press it. While this is on, joining
   a meeting also tells Teams not to ring anybody."
9. **Mobile.** Below `md` the People panel already replaces the stage (`SidePanel beside=false`,
   `call-stage.tsx:423`). Every row, menu item (the shared `DropdownMenuItem`, 44 px) and picker item
   meets the touch floor. Dialogs use the app's `Dialog` with its 44 px close. The burst sits above the
   bottom safe area and never covers the header. No hover is required; every consequence that lives
   in a title is also a visible line. With reduced motion there are no floating chips and no pulsing
   hand.

Icons, all verified in the installed `@hugeicons/core-free-icons`: `HandIcon`, `SmilePlusIcon`,
`MoreHorizontalIcon`, `LockIcon`, `SquareUnlockIcon`, `UserRemove01Icon`, `MicOff01Icon`.

## 8. Tests, mock and captures

**Rust (`src/calling.rs`).** Every builder test and golden join in § 5. Also:
- `conflict_redirect_from_frame` and `is_calling_host`;
- `receive_messages_from_frame`;
- the `signal_headers` behavioural test plus the retargeted scan, and the single-caller scan for
  `delete_signal`;
- names on the wire are the roster's own: admit and remove are fed `display_name`, never a label.

**Rust (`src/teams_read.rs`).** A channel call event keeps its root.

**Rust (`src/bin/server.rs`).**
- The outward-membership test over all 19 calling methods.
- `participation_refusal` per refusal.
- The branch-order scan.
- `can_*` derivation: link absent, attendee, not offered, demotion.
- `meeting_address` refusing two addresses, and refusing group or channel joins with preview off.
- The 409 redirect: once only; never on a placed call; never after acceptance; foreign host refused;
  an abandoned-cause `conversationEnd` ignored.
- End-for-everyone ordering: the hangup is posted before the session is dropped.
- The snapshot gate: a frame with seq 5 applied before a join answer with seq 3 keeps seq 5's links.

**Python.** `guard-live-automation.test.py` for the eleven names, `drives_the_calling_plane` (block and
allow halves) and the `--end-for-all` refusal.

**Web unit.**
- `call.test.ts` and `call-stage.test.ts`: hand order, the lobby section, the guest line, the moderation
  offer matrix, armed copy, and the older-backend fallback (everything absent means nothing drawn).
- `channel-meeting.test.ts`: link first, root only under preview, an empty root never offered.
- Store reaction lifetime and bound; `ongoingGroupCalls` TTL expiry restores "Call everybody".

**E2E (`web/e2e/meeting-participation.spec.ts`).** Every test resets with `call_invite {reset:true}` and
turns preview off.
- Preview off draws no new control, and `meeting_address` refuses a group join.
- Preview on:
  - raise then lower, with the position shown from the roster;
  - one `call_react` per press; an incoming hook reaction draws a chip that leaves;
  - an attendee sees no moderation and does see the lobby sentence;
  - an organizer admits one (the row leaves the lobby); Admit all arms, then sends exactly one
    `call_admit_all`;
  - mute one;
  - Remove opens a dialog and only Remove sends (RPCs counted);
  - End for everyone asks twice and ends with the right line;
  - a `refuse` hook makes each control say why;
  - a missing-link hook hides the control;
  - the group-chat Join sends `call_join` with `group_call_thread` and NEVER `call_place` (counted);
  - a placed call meeting the mock 409 shows the notice and its action;
  - the channel Join sends the link first, or the thread and root under preview;
  - at a phone's width (390 px), targets measured at 44 px.

**Regression and baseline.** `calling.spec.ts` must keep passing unchanged. Because one mock process
serves the whole run, run the full suite on master first as a BASELINE, then on the branch, and compare
the two (CLAUDE.md § Pictures in a message, on sibling flakes).

**Captures.** `cd web && bun run preview -- --out /tmp/mp --meeting-participation --dpr 3` captures:
- the header with hand and react, in both themes;
- the reaction menu, in both themes;
- a burst;
- the organizer People panel with lobby, hands and members, in both themes;
- the row menu, the Remove dialog and the End-for-everyone dialog;
- the attendee view;
- the 390 px panel;
- the group Join banner and menu row;
- the channel Join on a thread card;
- the Settings row, in both themes.

## 9. Live verification plan

All of these run after merge, behind the preview switch, which is off by default. The two installs
only run new code once it is on master (19440 re-stages, 19442 takes the in-app update), so merging
behind the switch is what makes the pair proofs possible without offering an unproven control by
default. The user turns preview on in Settings, which is their own press.

| Step | What it proves | Driver | Who acts | Success signal |
| --- | --- | --- | --- | --- |
| L0a | The shapes the bundle cannot say | `cd web && bun run join-live -- --from-chat --hold 180 --watch` (acts on nothing) | The user, in real Teams on their phone in the same pinned meeting: raise and lower a hand, send Like and Applause; optionally open breakout rooms | `joinLinks`/`frameLinks` show (or not) `publishState`, `removeState`, `sendMessage`, `removeParticipant`, `updateParticipantRole`, `updateMeetingSettings`; a `participation` block shows `stateType="raiseHands"`, `typeRank`, the level and the content values; `framePaths` include `/conversation/receiveMessage/` with type `Reaction` and the payload; organizer role values print; snapshot `sequenceNumber` presence and `participantCounts` fields print. Recorded in NATIVE-CALLING.md § 11 as MEASURED, with the date. |
| L0b | How call events are stored, and whether a channel post carries a join link | `cargo run --example call_event_recon` (new, READ-ONLY over the local store, counts only, no content or names) | Driver only | Per group chat: started vs ended rows and whether an ended row ever shares a started row's id; per channel: call events with and without a root, and root posts carrying a `meetup-join` link. |
| L0c | Whether an "ongoing call" signal reaches this endpoint | `TEAMS_LITE_CALL_CAPTURE` on the user's own backend, which they set; the `--watch` digest | The user's ordinary day (a colleague starts a group call in a chat they are in) | A frame with `evt` 115/116 or a started-event property naming a conversation URL, recorded as shape only. |
| L1 | `suppressDialout` is ACCEPTED on a join (acceptance only; it says nothing about dial-out on a group thread) | Preview on, then `cd web && bun run join-live -- --from-chat` | Driver | Join answered 200, the timeline reaches connected, RTP sent. A 400 means `JoinSuppressDialout` and `GroupJoin` stay unproven. |
| L2 | Raise and lower | `cd web && bun run join-live -- --pair --hand` | Driver, with the only-self proof | 19442's `call_raise_hand` answers `told_service:true`; 19440's digest shows a raiseHands state under the user's mri, gone after the lower; 19440's panel shows the hand. |
| L3 | Reactions both ways | `cd web && bun run join-live -- --pair --react` | Driver, with the only-self proof | 19442 answers `told_service:true`; 19440 logs a `receiveMessage` Reaction from the other endpoint and draws `call-reaction-burst`. |
| L4 | Lock and unlock | `cd web && bun run join-live -- --pair --lock` | Driver, with the only-self proof; unlocks on every path out | The PUT is accepted; 19440's snapshot digest shows the lock, then the unlock. If the unlock fails: unlock from real Teams. |
| L5 | End for everyone | `cd web && bun run join-live -- --pair --end-for-all` (refused by the guard for tooling; the user types it) | The user, at a moment they choose | The DELETE is accepted, both installs end with "The meeting was ended for everyone", and a follow-up plain run shows whether the pinned meeting is still joinable. |
| L6 | Admit | None can produce a lobby waiter: both installs are the organizer | The user's own click, with capture on, when someone knocks on a meeting they organize | The person moves to "In this call", or the failure notice names them. |
| L7 | Mute one person | None (the pair has no second participant) | The user's own click, with a colleague who agreed | Inline `muteUnmuteResponse`; the colleague's row shows muted. |
| L8 | Change a role | None | The user's own click, with a colleague who agreed | The row's tag changes, or the refusal shows Teams' words. |
| L9 | Remove | None | The user's own click, with a colleague who agreed | The person leaves the roster, or the failure notice names them. |
| L10 | Join an ongoing group call | None (starting a group call rings real people) | The user, in a group chat whose members agreed in chat beforehand; a colleague starts the call from real Teams; the user presses Join with capture on | The colleagues confirm nobody was rung again; the join answers or redirects once and connects; the panel shows who is already in. |
| L11 | Join a channel meeting | None (no sandbox channel) | The user, on a real channel meeting, with capture on | The join lands in the meeting (its roster and title), not in a new call; a refusal is recorded with the headers `post_signal` already captures. |

**The only-self proof** (L2-L5) uses two readings, and both must hold: the roster holds no mri but the
user's own, and the service's own `participantCounts` agrees. The roster alone is not enough: it is a
folded delta that drops ids not starting with `8` or `4` (`src/calling.rs:2058`).

## 10. Milestones

Each milestone ships alone, behind the preview switch, with its tests and captures.

| # | Title | Effort | Risk | Depends on |
| --- | --- | --- | --- | --- |
| 0 | Recon: `--watch` digest, the store recon example, the capture reading (L0a-c) | S | Low: reads only. The digest prints shapes plus the user's own values. | Coordinate the digest with video-receive (`video-measurement-blind-spots`) and meeting-awareness M0. |
| 1 | Plumbing and the switch: paths, accessors, `with_cause`, the transport split, every builder, the parsers, `MeetingJoin` kinds, `ParticipationAct` + `set_meeting_preview` + Settings row, the guard names and example rule | M | Low to the audio path when the golden joins pass. The transport split is pinned by a header test on built requests. | meeting-awareness M1 (`conversation_links`, the sequence gate, extended to group joins and the join answer); plane-reliability M4 (the one transport API this milestone uses, landed first: README § 3, collision 7); video-send M1 (the golden pins this milestone extends) |
| 2 | Raise and lower a hand; the queue (L2) | M | Content and level are unmeasured until M0; a wrong body is a silent `400 {}`. | M0, M1; meeting-awareness M3 (roster: published states, roles). meeting-awareness M3 OWNS the roster parse, raised hands included, and lands first; this milestone consumes it and does not feed it back (the cycle is broken that way) |
| 3 | Live reactions, both directions (L3) | M | The payload is unmeasured until M0. The frame branch returns before the merge and the lobby walker. | M0, M1 |
| 4 | Lock and unlock (L4) | S | Low and reversible. Whether a lock survives the conversation ending is unmeasured. | M1; meeting-awareness M3 (own role) |
| 5 | Admit one and admit all; outcome notices | M | A wrong admit cannot be undone. The mri target, the press-time lobby re-check, the guest label and the connected and group-modality refusals guard it. | M1; meeting-awareness M2 (scoped lobby) and M3 (lobby rule) |
| 6 | Mute one, mute everyone else | S | It shares the `mute` link with today's self-mute body. | M1; incoming-and-conversation (`self-mute-not-visible-in-meetings`); meeting-awareness M3 and M5 (server mute visible) |
| 7 | Change a participant's role | S | Reversible; role strings read from the bundle. | M1; meeting-awareness M3 |
| 8 | Remove a participant (asks twice) | M | Irreversible and socially sharp: the mri target, the roster re-check, self refused. The link is unmeasured until M0. | M1; meeting-awareness M3 |
| 9 | End the meeting for everyone (asks twice) (L5) | M | Irreversible. A failed DELETE still takes the user out. The pinned meeting may not survive. | M1; meeting-awareness M2 (named meeting endings) and M3 (own role) |
| 10 | Join an ongoing group-chat call, the bounded 409 redirect, the placed-call 409 notice (L1, L10) | M | High if wrong: an accidental dial-out. The preview gate, `suppressDialout`, the ceiling, the honest copy and a consenting first run guard it. | M0 (b, c), M1; incoming-and-conversation (`group-call-ring`, `incoming-attach-at-ring-and-join`); ringing-alerts-history (`call-line-actions` owns the Join vocabulary on call lines and lands first; this milestone consumes it and adds the group lines' actions RA9 leaves out); meeting-awareness M1 |
| 11 | Join a channel meeting from its post, with the root read fix (L11) | M | A wrong root addresses the channel, not the meeting; the link path is preferred and an empty root is never offered. | M0 (b), M1; meeting-awareness M1 |

After each live proof, a one-line commit moves the act into `PROVEN_PARTICIPATION` and updates
NATIVE-CALLING.md § 11. That commit is the rollout rule of § 6, not a separate milestone.

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Unmeasured content or payload shapes cause a silent `400 {}`, or an accepted state that other clients do not draw. | M0 measures a real client's shapes first. The UI draws what the roster and `receiveMessage` say, never the press. `post_signal` keeps the service's headers. Refusals are said. |
| Regressing the verified audio join or call. | Golden JSON for the meeting, link and invitation bodies. New fields are written only for new kinds or under preview. The transport split is pinned on built requests. `calling.spec.ts` and a plain `join-live` run are the gate. |
| Stale role-gated links after a demotion, or an older snapshot overwriting a newer one. | `conversation_links` is REPLACED under the client's sequence rule, the join answer included. `can_*` reads it, and the backend re-checks at press time. |
| New callback frames read by generic walkers (`lobby_state_in_frame`, `Links::collect`). | URL-matched branches before the merge, each returning; a source-scan test. |
| The wrong target for admit, mute, remove or role (a shared name, a nickname, a moving roster). | RPCs take the mri. The backend checks presence (or the lobby) at that moment and refuses self. Dialogs name the person; bodies carry the roster's own name. |
| A nickname written to Teams. | Bodies take `RosterMember.display_name`; a test pins it. |
| A group-chat join rings everybody. | `suppressDialout`; preview only; the 20-person ceiling while unproven; a first run with consenting colleagues; never `invitation_payload`. |
| A join finds no call and starts one ("Call started" for everyone). | Join is offered only on a fresh live signal with a TTL. The copy says what happens if the call has ended. M0c looks for the service's own signal. |
| The 409 redirect joins the wrong conversation or a late frame ends it. | Group joins only, once, while establishing, host-checked, fresh cause, abandoned-cause frames dropped. |
| End-for-everyone half fails. | Links are copied first. On failure the hangup is posted before the session is dropped, and the line says the meeting goes on without the user. |
| A pair test acts in front of a colleague. | Two-reading only-self proof (roster plus `participantCounts`) immediately before each press; undo on every path; `--end-for-all` is user-typed. |
| An example bypasses the write token. | `drives_the_calling_plane` refuses every new builder, `place_call` and the moderation body keys in any example. |
| A reaction flood from a stuck key or a double tap. | The backend rate floor and one RPC per press; the e2e counts RPCs. |
| The `mute` link carries two bodies. | M6 lands after the self-mute fix; both bodies are pinned; the journal names which was posted. |
| Turning preview on changes the meeting-join body (`suppressDialout`). | L1 is the first preview step and proves acceptance; a failure keeps it unproven and the user turns preview off. Alternatively, it can be scoped to group joins only (D4). |

## 12. Open questions and decisions for the user

**Decisions:**
- **D1.** Is the preview switch a Settings row (this design) or an environment variable on the user's
  own units?
- **D2.** May `join-live` gain `--pair --hand`, `--pair --react` and `--pair --lock` as tooling-run
  modes, with CLAUDE.md's MANDATORY paragraph widened accordingly? Is `--pair --end-for-all` run at
  all, given it may end the pinned meeting for good? The guard already makes it user-typed.
- **D3.** May the public, unauthenticated CDN chunks `services-io-calling-c6bd7cb820231eb7.js` and
  `services-io-calling-commands-46e6504366eefe69.js` be fetched, into a fresh untrusted directory, to
  read the raiseHands content, the reaction payload and the captions resolver? This run did not fetch
  them.
- **D4.** Should `suppressDialout` ride meeting joins under preview, as the client does (L1 proves it
  cheaply), or only group and channel joins?
- **D5.** Who agrees to the first group-chat join, and to the first mute, role change and removal?
- **D6.** Once the group join is proven, should the 20-person ceiling stay for joins?
- **D7.** Is the reaction rate floor (5 per 10 s) right?

**Open questions:**
- The raiseHands `content` and `level`, and participant- vs endpoint-level placement (M0, or D3).
- The reaction `payload`, and whether our own reaction is echoed to us on `receiveMessage`.
- Which of `publishState`, `removeState`, `removeParticipant`, `updateParticipantRole` and
  `updateMeetingSettings` an organizer's join answer names, and which arrive only on a later
  conversationUpdate.
- Whether conversationUpdate frames carry `sequenceNumber` on this tenant.
- The organizer's role strings (`role`, `meetingRole`, `meetingRoles`), and whether a presenter gets
  `admit` and `mute`.
- Where a stock client gets the `conversationUrl` of an ongoing group call (`evt` 115/116, a property
  of the started event, or something in the undownloaded chunk).
- Whether a POST to the conversation service with the group thread joins the live call, or answers 409.
- Whether "ended" is a separate message or an edit of "started" (L0b), which decides whether
  `ongoingGroupCalls` can be rebuilt from history.
- Whether `allowConversationWithoutHost:false` on a group join refuses a join to an ended call instead
  of starting one. This would be an unmeasured change, so it is a question here and not the design.
- Whether a channel meeting join needs `meetingInfo`, or the thread and root suffice.
- Whether a lock outlives the conversation ending, and whether the pinned meeting survives
  end-for-everyone.
- The coordination points with meeting-awareness: the join answer under the sequence gate, and
  `conversation_links` for group joins.

## 13. Review notes

Every reviewer claim was re-checked against the code and the bundle. "Accepted" means the claim held
and the design changed.

| Reviewer | Claim | Verdict | What changed |
| --- | --- | --- | --- |
| Code (blocker) | A channel call event stores an empty `thread_root_id`. | Accepted (verified at `src/teams_read.rs:1658-1680`, `web/src/lib/threads.ts:34`) | The read fix, healing by the existing backfill, an empty root never offered, and the link path preferred (§ 4.17, § 5). |
| Code | The client applies a snapshot only when `seq >= stored`. | Accepted (verified @1344600) | § 4.1 rule, including the join answer; a seq-5-before-seq-3 test. The `leave` heuristic is replaced by URL matching plus a links-object guard. |
| Code | The 409 path merges links, has no call correlation, no window, and weak evidence. | Accepted | The branch before the merge, a fresh cause, abandoned-cause drop, the establishing window, a host check, and INFERRED status with an M0 item (§ 4.16). |
| Code | The `suppressDialout` proof had no mechanism. | Accepted | Decided in the backend from the preview setting; no localStorage or RPC param; L1 described as acceptance only. |
| Code + Rules | New outward controls would ship enabled before their first proof. | Accepted | The `meeting_preview` switch plus `PROVEN_PARTICIPATION` (§ 6); proofs post-merge behind it. |
| Code + Rules | Nickname in outward bodies; a store read under the lock. | Accepted | Bodies take the roster's own name; names resolved outside the lock (meeting-awareness § 4.5 rule 9); the preview flag cached on `Ctx`. |
| Code + Rules | `ongoingGroupCalls` from frozen history is unreliable, and Join may start a call. | Accepted | The live signal plus a TTL; history only after L0b; honest copy; Answer while ringing; "Call everybody" restored after the TTL. |
| Code | `meetingRoles` arrays do not print. | Accepted | `skeleton()` prints string arrays for whitelisted keys. |
| Code + Rules | `--pair --mute-all` proves nothing. | Accepted | Dropped; mute-others is a user click (L7). |
| Code + Rules | The lobby rule must also clear on a `call` endpoint. | Accepted (verified @946936 `a&&(c=!1)`) | Taken from meeting-awareness rule 5. |
| Code | No `call_notice` event, and no attribution. | Accepted | A per-request cause, `PendingOp`, the `call_operation` event and a store handler. |
| Code + Rules | The 409 contradicts "nothing acts on a frame". | Accepted | Stated as the one exception, limited to group JOINs (§ 6). Placed calls get a notice with a press instead. |
| Code + Rules | The end-for-everyone fallback must come before the session drops. | Accepted (verified `src/bin/server.rs:15106`) | Order: copy links, DELETE, hangup on failure, then end; a distinct failure reason. |
| Code + Rules | The example rule's allow half was too wide and missed imports. | Accepted | Refuse every builder, `place_call` and the body keys in any example; bare names after `use … calling`. `meeting_join_probe`'s pin is its calendar subject. |
| Code | Admit-all carries no list; missing connected and group-modality checks. | Accepted (verified @1167405, @1248445) | Copy "everyone waiting (now {n})"; refusals added. |
| Code | Citation, offset and status fixes. | Accepted | `sendProxiedMessageAsync` is @1314458 in calling-pluginless.js; role strings are read-from-bundle (@1117690); typeRank ordering is inferred; self-mute belongs to incoming-and-conversation; the count is 19; `rG` @1187102, `nG` @1187711, `publishState` @1804499. |
| Code | The attach answer carries the snapshot for an incoming call. | Accepted as a dependency | It belongs to incoming-and-conversation; `can_*` stays false until then (§ 4.1). |
| Rules (blocker) | A missing `to` is not a rail; the 20-person ceiling does not apply to joins. | Accepted (verified @1742926, `src/bin/server.rs:7552`) | `suppressDialout` named as the only rail; the ceiling applied to group joins while unproven; preview only; a consenting first run. |
| Rules | The channel join may restart a meeting. | Accepted with nuance | Restarting a meeting under an existing post is what rejoining any ended meeting does. It is not the deferred "Meet" act, which creates a new post. The copy says so; preview only; the link path first. |
| Rules | The `join-live` scope widens beyond joining. | Accepted | User authorization (D2); `--end-for-all` refused for tooling; loud undo failures. |
| Rules | Only-self proof by roster alone. | Accepted (verified the `8`/`4` filter, `src/calling.rs:2058`) | `participantCounts` must agree; unknown refuses. |
| Rules | Copy states unmeasured facts; Lower all unarmed; count. | Accepted | Copy revised; Lower all armed; 19 entries. |
| Rules | Missing docs; the mock fixture moves sidebar order. | Accepted (verified no "Design crew" fixture) | Four docs added (§ 6); a test-time hook on an existing low-traffic fixture plus a baseline run. |
| Rules | `messageId` empty maps to `"0"`; two addresses resolved by order. | Accepted (verified `src/calling.rs:1816`, `src/bin/server.rs:10150`) | An explicit `JoinKind`; `meeting_address` refuses more than one address. |
| Rules | "Leave `post_signal`'s body untouched; extract only a header builder." | Partly rejected | Extracting a shared header builder IS a change to `post_signal`'s body; the two asks conflict. The design extracts `signal_headers` and `read_signal_reply` (the refusal journal must be shared by PUT and DELETE too) and pins equality with a test on built requests, which is stronger than leaving the code untouched and scanning it. |
| Code + Rules | Send `allowConversationWithoutHost:false` on a group join. | Rejected as a default | Unmeasured, and it could refuse a legitimate join. The verified join carries `true`. Kept as an open question to measure under preview. |
| Rules | Refuse `--lock` in the guard like `--end-for-all`. | Rejected as stated, left to the user (D2) | It is reversible, undone on every path out and recoverable from real Teams. It still needs the user's authorization before any tooling runs it. |
| Code | A fresh `session_id` or `cause_id` for the redirect. | Accepted with a choice | Only `cause_id` changes. Frames route by the surl, and one session id keeps every other link of the call stable. |
