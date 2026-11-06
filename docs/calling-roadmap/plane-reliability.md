# Plane reliability: a calling plane that looks after itself

**Workstream:** `plane-reliability` · **Status:** final design after two adversarial reviews (2026-11-05)

## 1. Summary

The calling plane should recover, bound and report itself with nobody watching. This design covers
seven things:

- **Recovery.** The calling registration should come up by itself after any sign-in outage,
  including the embedded image's boot, which always loses that race today. It should do this
  without polling the broker.
- **Honest state.** The page should say where the registration stands: waiting for sign-in, a
  directory with no calling service, connecting, or registered.
- **Bounded requests.** Every signaling request should be bounded, and a request whose answer
  matters must never be cancelled. Today an RPC handler dies with its page's socket, and a POST
  that places, joins or accepts can be cancelled mid-flight. That loses the links the orphan
  hangup needs.
- **Answering.** A failed Answer must never show the caller a decline, and a lost answer must not
  be treated as a refusal.
- **Stopping.** A stopped backend should leave the call it is in and hand its registration back.
  Tooling must not be able to cause that by stopping a backend that holds a live call.
- **Restarts.** A restart, an update or a re-stage should ask before it ends a live call. An
  automatic re-stage should wait for the call to end and never end one itself.
- **Container and Kubernetes.** Calling from the image and the chart should be verified, and the
  docs should say plainly that call MEDIA flows from the browser, not from the pod.

The verified audio shapes are not touched: the 1:1 both ways and the meeting join, which means
their bodies, links, callbacks and order. Three things are protocol behaviour new to this app.
Each is measured first and ships behind a switch or a measurement gate:

- extending a registration on a live socket (M2);
- detecting a half-open socket (M8);
- cancelling an outgoing ring nobody answered (M10).

## 2. Where we are today

Every reference was re-checked against this worktree (HEAD `c941850`) and the 2026-11-05 bundle.

| Area | Today | Where |
| --- | --- | --- |
| Calling start | `spawn_calling` makes ONE attempt and logs a failure. `start_calling` calls `self.session()` and `calling::endpoints()` BEFORE it spawns `trouter::run`, so a failure there is permanent for the process. `spawn_calling` runs right after the embedded boot, whose own comment says the first sign-in "fails fast against a broker that is not up yet". | `src/bin/server.rs:15660-15669`, `:14454-14460`, call at `:4938`, comment `:4897-4904` |
| Reconnects after start | `trouter::run` asks the provider for credentials before every attempt, so once started, calling survives outages. The provider's `session()` REBUILDS through `teams::connect`, which reaches the broker, whenever the cached session is stale or absent. | `src/trouter.rs:273-312`; impl `src/bin/server.rs:4543-4549`; `session()` `:4287-4299` |
| Backoff | It doubles to 30 s and never resets after a good connection. A socket that dropped after days reconnects up to 30 s late. | `src/trouter.rs:310` |
| Registration | `register()` runs only in the socket.io `1::` branch, with `CALLING_TTL = 3600`, and IGNORES the HTTP status (`.send().await?; Ok(())`). The allocate answer's `ttl` is never read, and the handshake's heartbeat and close timeouts are discarded (only `split(':').next()` is kept). | `src/trouter.rs:401`, `:60`, `:466-482`, `:324-334`, `:345-346` |
| Unbounded requests | `ctx.http` has no timeout. `trouter::run`'s own client and `connect_async` have none either, so a hung allocate stops the reconnect loop (calling AND messaging). | `src/bin/server.rs:4818`; `src/trouter.rs:279`, `:362` |
| Liveness | The read loop handles only `1` and `3` frames. It pings `5:{n}+::{"name":"ping"}` every 30 s and never reads an answer. | `src/trouter.rs:366-437` |
| Ready state | `ready = channel.is_some() && connected`. A credentials failure after start arrives as `Status::Disconnected`, and nothing records why. | `src/bin/server.rs:14422-14432`, `:14518-14531`; `src/trouter.rs:296-308` |
| Stale copy | Both `call_prepare` refusals say "turn it on in Settings", split across a Rust line continuation. The placing path checks `channel` only, while the meeting path also checks `connected`. The same copy lives on in the web app. | `src/bin/server.rs:7408-7412`, `:7510-7520`; `web/src/lib/call-failure.ts:163`; `call-failure.test.ts:99-101` |
| RPC lifetime | Every RPC future lives in `serve_conn`'s local `FuturesUnordered`, so a dropped page socket CANCELS the handler, mid-POST included. `call_place`, `call_join` and `call_accept` read the creating POST's answer to merge links or call `hang_up_orphan`. A cancelled handler reads nothing. | `src/bin/server.rs:5176-5214`; `:7707`, `:7787`, `:7878`; `hang_up_orphan` `:15120-15136` |
| Accept chain | Attach, then acceptance. `Connected` is set after the attach answers. An acceptance failure is only logged. `CallEndReasonAcceptFailed` happens only when the FIRST POST fails. `post_call_signal` awaits `session()` and `tokens.get()` before the POST. | `src/bin/server.rs:7853-7948`; `:15138-15151` |
| A failed Answer declines | `answerCall`'s catch calls `hangUpCall()`. `call_hangup` treats incoming+Ringing as a decline and posts `rejection_payload`. But the answering `call_prepare` changes no state, so a failure before `callAccept` has reached nobody. | `web/src/lib/store.ts:3098-3118`; `src/bin/server.rs:8369-8416`, `:7486-7503` |
| Reconnect reconcile | `refreshCallStatus` sets `UNKNOWN_CALL_STATUS` (`call: null`) on any error and tears nothing down. A page whose backend restarted mid-call keeps its microphone. | `web/src/lib/store.ts:3025-3031`, `:1698`; `web/src/lib/call.ts:160` |
| Unanswered rings | `expire_unanswered_call` ends BOTH Ringing and Dialing locally after `CALL_RING_TIMEOUT` (60 s) and posts nothing. `CallEndReasonUnanswered` is drawn as "The call ended." (the default label). | `src/bin/server.rs:15168-15186`, `:773`; `web/src/lib/call.ts:367-369` |
| Stopping | No `tokio::signal` anywhere (Cargo.toml has no `signal` feature). The idle shutdown runs `stop_calling`, which drops the call locally and DELETEs the registration. `unregister_calling` reaches `session()` (the broker). | `Cargo.toml:14`; `src/bin/server.rs:4987-5001`, `:14570-14598`, `:15074-15107` |
| Launcher | `stop()` is `proc.kill(9)`. `killChildOnExit` SIGKILLs on SIGINT, SIGTERM and SIGHUP. The child is spawned in the launcher's own process group. `relaunch()` stops the web server BEFORE the backend. | `launcher/src/backend.ts:274-279`, `:450-458`, `:420-425`; `launcher/src/update.ts:72-83`; `launcher/src/launch.ts:320-324` |
| Supervisors | `teams-lite-app.service`: `KillMode=control-group` (systemd TERMs the launcher AND the backend), `KillSignal=SIGTERM`, `TimeoutStopSec=10`. Backend unit: `TimeoutStopSec=10`. `compose.yaml` `stop_grace_period: 60s`; `compose.host-broker.yaml` sets none (Docker's 10 s). The chart uses 90 s embedded and 30 s otherwise. The entrypoint forwards ONE TERM to the launcher. | `packaging/systemd/teams-lite-app.service:118-122`; `teams-lite-backend.service:87-88`; `packaging/docker/compose.yaml:152`; `packaging/helm/teams-lite/templates/statefulset.yaml:132-135`; `packaging/docker/entrypoint.sh:609-624`, `:692-707` |
| Restart/update | `restart_backend` guards agent runs only, and in the launcher shape it runs `stop_calling` BEFORE it emits `backend_restart`. `apply_update` has no guard, and the dispatcher passes it no params. | `src/bin/server.rs:4233-4271`, `:4128-4204`, `:5291` |
| Re-stage | `cmd_update` builds, waits for the agent only, stages, then try-restarts all three units, `teams-lite-app.service` included, unconditionally. | `bin/teams-lite-service.sh:858-889`, `:745-762` |
| Guard | Every stop is tooling's by design. Only `update --now` is refused. | `.claude/hooks/guard-live-automation.sh:2128-2136`, `:2172-2186` |
| Docs that pin the SIGKILL | The chart test pins `kill(9)`. values.yaml and the chart README say the registration is never handed back. README:409 says the backend reaches "Teams' media relays directly". | `packaging/helm/chart.test.ts:1957-1974`; `packaging/helm/teams-lite/values.yaml:521-531`; `README.md:240-247`, `:409` |
| Drivers | `call-live` fronts: `--local` or the tailnet. `join-live`: `--local`, `--released` or the tailnet. | `web/scripts/call-live.ts:366`; `web/scripts/join-live.ts:1436-1445`; `web/scripts/sandbox-live.ts:57-68` |

## 3. Gaps covered, deferred and rejected

**Covered**

- `calling-registration-retry` and `calling-registration-recovery`, merged (M1).
- `registration-ttl-extension`. Round 1 said the logic was not in the bundle; it IS in `cmd-trouter.js`, and the extension fires only when the registrar TTL is shorter than the connection's. Gated, M2.
- Registrar status ignored (M1 measures it; M9 handles a refusal, gated).
- `signaling-post-timeouts`, extended to the trouter's own client and to the cancellation hazard in `serve_conn` (M4).
- NEW: the backoff never resets (M1). The half-open socket (M1 counts, M8 acts, gated).
- `graceful-shutdown-unregister` and `backend-restart-mid-call` (M5). The restart half of `refreshCallStatus` bypassing the one teardown path (M3).
- `restart-guard-live-call` (M6).
- `answer-failure-declines` (M3).
- Stale "turn it on in Settings" copy, and guard parity on the placing path (M1).
- `container-calling-unverified` (M7).
- `outgoing-ring-timeout-cancel` (M10). Gated, and corrected: rings are ALREADY abandoned locally at 60 s.

**Deferred or rejected**

| Item | Decision | Why |
| --- | --- | --- |
| Hangup phrases (`ConversationEndNoModalityConnected`, `{code,subCode,phrase,resultCategories}`) | Rejected for now | `hangup_payload` (`src/calling.rs:1522-1540`) is the body measured to end an outgoing 1:1 for both people (NATIVE-CALLING § 8b) and to leave a meeting (§ 8). Changing a measured shape for no failure is a risk on the verified path. Revisit only if the service refuses ours. |
| Per-link cause ids (`lt(e,t,i=Ht())`) | Rejected for now | `CallbackBase::link` (`src/calling.rs:282-292`) reuses one cause id per call on every measured call; nothing has refused it. |
| `TsCallingVersion` 2026.24.01.6 → 2026.40.01.8 | Deferred | `CLIENT_IDENTITY` (`src/calling.rs:80-81`). The bundle's `On()`/`UH()` (calling-pluginless.js) and `Kn()`/`x7()` (calling-shell.js) return `"2026.40.01.8"`. The service reads this to decide what a client understands, so a bump can change what it SENDS. Plan: a `TEAMS_LITE_CALLING_TS_VERSION` override, off by default, exercised with join-live and call-live, flipped together with **video-send**. |
| TURN reachability "from a pod" | Not this workstream's | The pod never carries media; `call_ice_servers` hands the BROWSER its STUN/TURN. Our TURN credentials belong to **media-robustness-devices**. Here we fix the doc and verify signaling egress. |
| `trouter-reconnect-keeps-call`, `media-owner-lost` | **media-robustness-devices** | This workstream only reconciles a page reconnecting to a backend with no call (M3). Re-pointing links after an surl change, and the beacon on pagehide, stay there. |
| The incoming `callLeg` hangup door | **incoming-and-conversation** | The shutdown hangup and the orphan hangup reuse `Links::hangup()` (`src/calling.rs:390-392`) and inherit whatever door that workstream lands. |
| "Always available" withdrawal on TERM | Out of scope (not calling) | The new signal arm is where it would go. Recorded as a follow-up in § 12. |
| A reconnect after three registration refusals | Dropped from M9 | It is not the client's rule: `forceReconnectDueToNoRegistration` fires only when `connectionDependsOnRegistration` is true (default `()=>!1`) and only after a registration had succeeded. A reconnect can move the surl and end a live call. |

## 4. Protocol

### 4.1 Register the calling endpoint (unchanged body; status now read; bounded)

- **Method / link:** `POST {calling_registrarUrl}`, read by `calling::endpoints` (`src/calling.rs:166-191`), falling back to `trouter::REGISTRAR`. It is sent once the socket.io `1::` frame arrives (`src/trouter.rs:383-405`).
- **Body:** `registration_body` (`src/trouter.rs:519-534`), unchanged:

  ```json
  {"clientDescription":{"appId":"SkypeSpacesWeb","aesKey":"","languageId":"en-US","platform":"edge",
    "templateKey":"SkypeSpacesWeb_2.6","platformUIVersion":"1415/26061118216"},
   "registrationId":"<calling epid, endpoint_id_path(db, port, \"calling\")>","nodeId":"",
   "transports":{"TROUTER":[{"context":"","path":"<bare surl>","ttl":3600}]}}
  ```
  Headers are unchanged (`content-type`, `X-Skypetoken`, `Authorization: Bearer <ic3>`). New: `.timeout(10 s)`, the client's `fetchTimeoutMs`. New: `register()` returns the `StatusCode` and logs `[<role>] registered: <status> (ttl 3600 s)` for both roles. That is log-only, so messaging cannot regress. `CallingChannel` gains `registrar_status: u16`. In M1 a non-2xx still proceeds exactly as today (`on_registered`, `Status::Connected`).
- **Callbacks:** none.
- **Bundle evidence** (cmd-trouter.js):

  ```
  const C={clientDescription:f,registrationId:this.options.registrationId,nodeId:"",transports:k},T=new Request(this.proxyUrlRewrite(this.options.registrarUrl),{method:"POST",…
  w=await V(b,this.options.requestTimeoutMs),w.status===401){…this.setTokenHeader(f,await this.getToken(D))…continue}if(w.status>=500&&w.status<600)throw new Error(…)
  ```
  The registrar client is built with `requestTimeoutMs:t.timeoutOptions.fetchTimeoutMs`, and `fetchTimeoutMs:1e4`.
- **Status:** the body is measured (calls ring here and are placed, NATIVE-CALLING § 8a and § 8b). The registrar's HTTP status has never been read; M1 measures it.

### 4.2 Calling socket: allocate, handshake, websocket — bounded and diagnosed (M1)

- **Method / link:** `POST {calling trouter}/v4/a?epid=…`, then `GET {socketio}socket.io/1/?…`, then `WS …/socket.io/1/websocket/{sid}?…` (`trouter::allocate_url_for`, `src/trouter.rs:107-128`; `connect_once` `:315-362`). These are the same steps for messaging.
- **Body:** unchanged. New bounds use the client's own numbers: `connect_timeout(10 s)`, `.timeout(10 s)` on the allocate and the handshake, and `tokio::time::timeout(30 s)` around `connect_async`. New readings, as numbers only:
  - the allocate's `ttl` (string or number, as `populateConnectionStateFields` reads it);
  - the handshake's heartbeat and close timeouts (`sid:heartbeat:close:transports`);
  - a per-connection tally of frame types.

  On every connection end, one line:
  `[<role>] connection ended after <s>s (allocate ttl <t>s; heartbeat <h>s close <c>s; frames 1:<a> 2:<b> 3:<c> 6:<d> other:<e>)`.
  The backoff resets to 1 s after a connection that lived 60 s or more (`next_backoff(prev, lived)`).
- **Callbacks:** none.
- **Bundle evidence** (cmd-trouter.js):

  ```
  timeoutOptions:{connectionTimeoutMs:i.trouterSettings.trouterConnectTimeoutInMs||3e4,fetchTimeoutMs:1e4,pingTimeoutMs:4e4,pongTimeoutMs:5e3,maxBackoffMs:…?3e5:3e4,requestTimeoutMs:5e3,…},
  incallTimeoutOptions:{connectionTimeoutMs:1e4,fetchTimeoutMs:5e3,pingTimeoutMs:5e3,pongTimeoutMs:2e3,maxBackoffMs:…?3e5:1e4,…}
  populateConnectionStateFields(e){const t=typeof e.ttl=="string"?parseInt(e.ttl,10):e.ttl;if(this.connectionExpireTimestampInSecs=b(t)…
  ```
  The client switches to the tighter in-call set while a call is up (`enterIncallMode(){…this.timeoutOptions=this.options.incallTimeoutOptions…}`). That is recorded as an input to M8 and to **media-robustness-devices**, and is not adopted here.
- **Status:** read from the bundle. No wire change.

### 4.3 Extend the registration on a live socket (M2, gated)

- **Gate:** built only if M1's lifetime lines show calling connections outliving 3570 s. It sits behind `TEAMS_LITE_CALLING_REREGISTER=1` (read once) until a 75-minute observation with a call during the extension has passed (§ 9).
- **Method / link:** the same `POST` as § 4.1, on the SAME connection and surl, with fresh credentials.
- **Timer, following the client exactly:** `registration_extension(registrar_ttl, connection_ttl: Option<u64>) -> Extension`:
  - connection ttl known and `registrar_ttl < connection_ttl` and `registrar_ttl > 30` → `Extend(registrar_ttl - 30)`;
  - connection ttl known otherwise → `ExpireOnly(min(registrar_ttl, connection_ttl))`;
  - connection ttl unknown → `ExpireOnly(registrar_ttl)`.

  `ExpireOnly` logs `[calling] the registration's TTL ran out on a live connection` and does nothing else. That is what the client's expiry timer amounts to with `connectionDependsOnRegistration` at its default. If M1 shows the allocate states NO ttl and sockets outlive 3600 s, the 75-minute colleague call (§ 9) decides first. If calls stop ringing, extending at 3570 s with an unknown ttl ships as a STATED deviation from the client.
- **Never in the read loop.** When the timer fires, the read loop `tokio::spawn`s the extension. The task holds clones of the client, endpoint, surl, epid and provider, and reports its status over a oneshot, which the loop's third `select!` arm reads. Fetching credentials and POSTing inside the loop would stop reading and acking frames ("ack EVERY request (the server drops us otherwise)", `src/trouter.rs:393-416`). A dropped socket moves the surl, and that ends a live call (`src/bin/server.rs:14481-14503`). `run` therefore takes `P: CredentialProvider + Clone + 'static`.
- **Callbacks:** none.
- **Bundle evidence** (cmd-trouter.js):

  ```
  getRegistrationTtl(){const e=w(this.connectionExpireTimestampInSecs);…if(this.options.registration?.registrarTtlSec&&e>0){const t=this.options.registration.registrarTtlSec<e;return[Math.min(this.options.registration.registrarTtlSec,e),t]}return this.options.registration?.registrarTtlSec?[this.options.registration.registrarTtlSec,!1]:e>0?[e,!1]:[3600,!1]}
  startRegistrationTimer(){…if(e<=30||!t)return …void(this.registrationTimerId=setTimeout(()=>{…"Registration expired but the connection is still alive. Should never happen"),this.dispatchUnregistered()},1e3*e));const n=e-30;…setTimeout(()=>{this.logger.info("Registration extension timer fired"),…this.fsm.onRegistrationNearExpiry()},1e3*n)}
  ```
- **Status:** read from the bundle. Whether this tenant's registrar lapses a registration on a socket that stays up is unmeasured.

### 4.4 A refused registration (M9, gated)

- **Gate:** ships only if M1's logs show the tenant answering 2xx today, so the refused state cannot be reached on the working path.
- **Behaviour (calling role only):**
  - a 401 → fresh credentials and one immediate retry;
  - a 5xx → retry with backoff;
  - any other non-2xx → `registration = refused(status)` and a retry after 123 s.

  There is no reconnect (§ 3). `ready` is NOT changed by a refusal, so outgoing calls and joins keep working exactly as today.
- **Bundle evidence:**

  ```
  onRegistrationFailed(){this.state===g.Connected&&this.registrationState===f.Registering?(this.worker.dispatchUnregistered(),this.registrationState=f.Retrying,this.worker.startRegistrationRetryTimer()):…
  startRegistrationRetryTimer(){this.stopRegistrationTimer(),this.registrationTimerId=setTimeout(()=>{this.registrationTimerId=void 0,this.fsm.onRetryRegistration()},123e3)}
  ```
- **Status:** read from the bundle.

### 4.5 A half-open calling socket (M8, gated)

- **Frames:** socket.io v1 on the existing websocket. Today teams-lite sends `5:{n}+::{"name":"ping"}` every 30 s and reads nothing back.
- **Plan:** M1's handshake reading and frame tally decide it:
  - a non-empty heartbeat timeout, or `2::` frames arriving → echo `2::` (socket.io-client does that on its own);
  - `6:::{n}+…` acks arriving for the pings → a watchdog reconnects after three unanswered pings (about 95 s).

  It sits behind `TEAMS_LITE_TROUTER_WATCHDOG=1`, off until a week of journal shows no false positive. A false positive reconnects, and that can end a live call until **media-robustness-devices** lands `trouter-reconnect-keeps-call`.
- **Bundle evidence:**

  ```
  sendPingRequest(){if(this.socket&&this.pingResponseTimerId===void 0)try{…this.socket.emit("ping",()=>{e!==!0&&this.onPingResponse()}),this.pingResponseTimerId=setTimeout(()=>{this.logger.error("Ping response timeout is fired"),…this.fsm.onPingResponseTimeout()},this.timeoutOptions.pongTimeoutMs)}…
  ```
- **Status:** the socket.io 0.9 ack spelling is inferred. It is measured by M1 before anything acts on it.

### 4.6 Bound every signaling request, and never cancel one whose answer matters (M4)

- **Method / link:** every `calling::post_signal` (`src/calling.rs:2188-2236`), the relay-token GET (`fetch_relay_credentials`, `:2406`), and the registrar DELETE. This milestone also owns the ONE transport API the other workstreams' new requests go through (README § 3, collision 7; § 5 below): incoming's attach, DELETE decline and keep-alives, and meeting-participation's PUT and DELETE, so none of them adds a transport or a timeout of its own.
- **Classes**, each with a transport bound through `RequestBuilder::timeout`. The bound aborts the HTTP request only; no broker call sits inside it.

  | Class | Requests | Bound | Why |
  | --- | --- | --- | --- |
  | `Create` | call invitation, join, attach, acceptance, `addModality` (sharing grant) | 65 s | Their answer carries the links a cancelled start is hung up on. 65 s matches the client's longest single-request operation (`addModalityTimeoutSec:65`). |
  | `Answer` | media answers, our own renegotiation offer, subscriptions | 35 s | `mediaAnswerTimeoutSec:35`. A lost answer costs one renegotiation, not an orphan. |
  | `End` | hangup, reject, mute, keepAlive, stop-sharing, glare rejection, acknowledgements, relay GET, registrar DELETE | 10 s | Idempotent or ending. |
  | `Shutdown` | the hangup and the DELETE at exit | 4 s | Inside the stop budget (§ 4.8). |

- **The credential phase is never wrapped in a dropping timeout.** `session()` and `tokens.get()` can hold the broker's interactive turn, and `auth::rescue` takes the broker's window back before it returns (`src/auth.rs:587-640`). A future dropped there would leave an abandoned window on the broker's display.
- **Detached execution.** The operations whose answer matters (`call_place`, `call_join`, `call_accept`, `call_start_sharing`) and the hangup run in a task of their own (`tokio::spawn`). A dropped page socket or a page timeout no longer cancels them. The handler BOUNDS ITS WAIT on that task:
  - `CALL_START_WAIT` = 150 s, which covers two `Create` POSTs plus headroom;
  - `HANGUP_WAIT` = 12 s.

  When the wait ends first, the handler acts on local state: the start ends locally with its existing `…Failed` reason, and the hangup drops locally with `told_service: false`. The task's late answer then goes through the EXISTING `held == false` → `hang_up_orphan` path. This is the rule `finish_claude_signin` already follows ("the handler waits for the task; the task does not wait for the handler"). Nothing on the wire changes.
- **Page ceilings, pinned across the boundary:**
  - `callPlace`, `callJoin`, `callAccept` and `callStartSharing` carry `CALL_START_REQUEST_TIMEOUT_MS = 180_000` in `web/src/lib/ws-client.ts`, longer than `CALL_START_WAIT`;
  - `callHangup` keeps the 30 s ceiling, longer than `HANGUP_WAIT`.

  `the_pages_call_start_ceiling_outlasts_the_backends_wait` pins both pairs, the way `the_pages_agent_ceiling_outlasts_the_backends_idle_bound` pins the agent's.
- **Bundle evidence** (calling-pluginless.js). The client bounds OPERATIONS and aborts a request on its own timer:

  ```
  incomingCallEstablishmentTimeoutSec:75,outgoingCallEstablishmentTimeoutSec:90,…,mediaAnswerTimeoutSec:35,mediaAnswerAcknowledgementTimeoutSec:15,addModalityTimeoutSec:65,unmuteTimeoutSec:35
  const c=()=>{r.info("Aborting pending request"),s.abort()};
  ```
- **Status:** the numbers are inferred (ours, at or above the client's). Every measured signaling answer arrived in about 2 s (NATIVE-CALLING § 8).
- **As implemented (M4, PR4):** the four classes and their numbers exactly as the table above, on `calling::SignalClass::timeout()`. The transport is `post_signal`, `put_signal` and `delete_signal`, each `(http, class, url, session, ic3, correlation_id, payload)`, over a PRIVATE `signal_within(…, method, bound)` (the design's `post_signal_within`, kept private so the example guard's sender list needs no new name; the tests drive it from `calling::tests`), `signal_headers(builder, region, ic3, correlation_id)` and `read_signal_reply`. One reply rule is new: a 4xx is `SignalRefused { status }` (Display is the refusal sentence it always was), a 5xx is not. A SUCCESS whose body was cut off still reads as `Value::Null`, as it did, with one journal line — reported as a failure it would end a placed call while the callee rang and strand the frames that can still name its links (§ 13, the review of M4). A timeout is said as `calling: no answer to a signaling POST from <redacted url> within 65 s` and never as a refusal, and reqwest's own copy of the address (query included) is stripped first (`without_url`); the page maps it to "Teams' calling service did not answer in time. Try again." The backend's own give-up names no cause (`<method>: the call did not start in time`, mapped to "The call took too long to start. Try again in a moment."), because every request inside the wait is bounded well under it. `fetch_relay_credentials` and `trouter::unregister` (which now takes its `bound`) get the `End` class's 10 s. The detached work is `Ctx::place_reserved_call`, `join_reserved_meeting`, `accept_ringing_call` and `start_sharing_session`, run by `detached(wait, op)` — which returns `Finished`, `StillRunning` or `Stopped` (a panic) — and waited on by `await_call_start` (`CALL_START_WAIT`, 150 s; on giving up: `end_call_if_still(call_id, failed)` and the error `<method>: the calling service did not answer in time`). `call_start_sharing` gives up by answering the page only: its task gives its own reservation back when no grant comes. `call_hangup` decides its door and body in the handler and hands the POST and the local drop to `tell_then_end_call`, waited on for `HANGUP_WAIT` (12 s), after which the handler drops the call itself. An accept's ACCEPTANCE has an orphan path of its own (`hang_up_orphan("call_accept (acceptance)", …)` on the attach's and the acceptance's links, on its late success and on its failure alike), and is never posted for a call that went while the attach was answered. Every ending a task makes is id-scoped (`end_call_if_still`), and `end_call_locally` now clears the slot only if it is still the call it ended — an ended call frees the slot, so a new call admitted while the ending frame went out was clearable before. A task's requests carry their own call's correlation id (`post_signal_for`), `hang_up_orphan` included. The page's ceiling is `CALL_START_REQUEST_TIMEOUT_MS = 180_000` on the four starts; the hangup keeps the ordinary 30 s. The credentials are read in `post_signal_for`, outside every bound. Not shipped with M4, because nothing consumes them before M5: `TokenCache::cached` and `END_REASON_BACKEND_STOPPING` (both land with the stop that uses them). The golden pins M4 depends on (video-send M1's fixture set) landed first, as their own commit.

### 4.7 Unregister the calling endpoint on stop

- **Method / link:** `DELETE {calling_registrarUrl}/{urlencoded epid}` (`trouter::unregister`, `src/trouter.rs:489-512`). No body. It already treats a non-2xx as an error.
- **New:** it also runs on SIGTERM, SIGINT and SIGHUP, and on the supervisor restart (§ 4.8), bounded at 4 s inside the stop and 10 s elsewhere. At exit it uses CACHED credentials only: a new `Ctx::session_if_fresh()` that never mints, and a new `TokenCache::cached(scope)` that exposes today's private `cached_fresh` (`src/auth.rs:283-292`). With nothing cached, it skips the DELETE with one line, and the registration expires within the hour as it does today.
- **Bundle evidence:**

  ```
  async unregister(){this.logger.info("[RegistrarClient] sending unregister request");const f=new Request(this.proxyUrlRewrite(`${this.options.registrarUrl}/${this.options.registrationId}`),{method:"DELETE",…});await this.callRegistrar(f)}
  ```
- **Status:** implemented and read from the bundle. Its answer on this tenant is not recorded; the journal line `[calling] unregistered …` exists to show it (§ 9).

### 4.8 Leave the live call when the backend stops (M5)

- **When:** SIGTERM, SIGINT or SIGHUP; the idle shutdown; a Settings restart in the supervisor shape; a launcher restart or update (the launcher's TERM); and a confirmed restart or update (§ 6).
- **Method / link:** `POST` to `calls.links.hangup()`, the first of `hangup`, `end`, `leave`, `conversationEnd`, `callLeg` (`src/calling.rs:390-392`), with the call's id as correlation.
- **Body:** `hangup_payload`, unchanged (`src/calling.rs:1522-1540`):

  ```json
  {"participants":{"from":<local participant>},
   "conversationTransactionEnd":{"reason":"noError","code":0,"phrase":"ConversationEndedNoModality"},
   "callTransactionEnd":{"code":0,"phrase":"LocalUserInitiated"}}
  ```
- **Phases:** only a call that HOLDS THE USER, through a new `fn call_holds_the_user(phase)`: Dialing, Connecting or Connected. An incoming call still RINGING gets no request. It is dropped locally as `CallEndReasonCallingTurnedOff` ("This machine stopped taking calls.") and keeps ringing on the user's other devices. It is never declined.
- **Sequence (`Ctx::leave_calls_for_exit`, replacing `stop_calling`):**
  1. Set `stopping`, so `call_prepare` refuses and the start loop stands down.
  2. Snapshot the call, its hangup link, its local participant and the epid, and take the trouter handle.
  3. `end_call_locally(END_REASON_BACKEND_STOPPING)` (or `CallingTurnedOff` for a ring). This emits to every page.
  4. Abort the trouter task.
  5. `tokio::time::timeout(SHUTDOWN_BUDGET = 5 s, join!(hangup, delete))`, each request bounded at 4 s and both on cached credentials.
  6. Emit the final status.
  7. The caller then waits `RESTART_ANSWER_GRACE` (250 ms) so the frames flush, and calls `std::process::exit(0)` rather than returning. Dropping a runtime waits on every `spawn_blocking` task (X11 capture, the agent CLI), and a return could hang until the supervisor's KILL.

  A read-only backend has no calling, so it only exits. Steps 3 and 5 also check `!read_only()` explicitly, as a second gate behind `calling_available()`.
- **Repeated signals are ONE stop.** `KillMode=control-group` TERMs the launcher and the backend together, and the launcher then forwards its own TERM. A terminal's Ctrl+C reaches both processes of one group, and the launcher forwards again. So every signal after the first is logged once (`… while stopping — ignored; the launcher or the supervisor bounds this stop`) and ignored. The hard bound is the launcher's KILL (7 s) or systemd's `TimeoutStopSec` (10 s). Registering tokio's handler replaces the default terminate action, which is what makes ignoring possible.
- **Callbacks:** the service's `/call/end/` frames are not awaited.
- **Bundle evidence.** The client leaves on page close, and DECLINES a ringing call on page close. teams-lite does the first and deliberately not the second:

  ```
  function fM(e,t,i,r){…const n={payload:{participants:{from:{id:…,displayName:…,endpointId:…,participantId:…,languageId:…},scope:…},conversationTransactionEnd:{reason:"noError",code:I.CALL_END_CODE.SUCCESS,phrase:I.CALL_END_PHRASE.CONV_END_NO_MODALITY},callTransactionEnd:t}};…
  …&&this.currentCallStatus===I.CALL_STATUS.RINGING)return this.rejectCallWithFetchAndKeepAlive(e),t(),!0;const i={code:I.CALL_END_CODE.SUCCESS,subCode:I.CALL_END_SUB_CODE.BEACON,phrase:I.CALL_END_PHRASE.LOCAL_USER_INITIATED},r=JSON.stringify(fM(this,i).payload)
  ```
  This sits under `enableCallStateChangesOnPageClose` (calling-pluginless.js `endWithBeacon`).
- **Status:**
  - Measured: End on a CONNECTED outgoing 1:1 ended the call for both people (§ 8b), and a meeting leave was confirmed by the service's own `callEnd` (§ 8).
  - Unmeasured: the same body during DIALING (the client sends `cancelOutgoingCall`'s 487 there), the incoming `callLeg` door (**incoming-and-conversation**), and sending it from a signal handler. All three are in § 9.

### 4.9 Decline only from the Decline press (M3)

- **Method / link:** unchanged. `POST calls.links.reject()` (`src/calling.rs:361`), falling back to `hangup()`, with `rejection_payload` (`{"callRejection":{"sender":<local>,"code":603,"phrase":"Decline"}}`).
- **New optional `call_hangup` param:** `intent: "decline" | "end"`. Any other value is refused with `call_hangup: intent must be decline or end`. The rule is a pure `fn hangup_door(direction, phase, intent) -> HangupDoor { Reject, Hangup, Nothing }`:

  | direction | phase | intent | door |
  | --- | --- | --- | --- |
  | incoming | Ringing | `decline` | Reject |
  | incoming | Ringing | `end` | Nothing (local drop, `CallEndReasonLeftRinging`, `told_service:false`) |
  | incoming | Ringing | absent (an older page) | Reject (today's behaviour) |
  | anything else | any | any | Hangup |

  `end` while an accept is IN FLIGHT (the phase stays Ringing until the attach answers, `src/bin/server.rs:7853-7869`) drops locally. The in-flight accept's own answer is then hung up by `hang_up_orphan`, exactly as for every other start cancelled mid-POST. Its door is the attach answer's `callLeg`. That is unmeasured, and so is today's reject door: no decline has been measured live either. What the change buys is that an End press, a page timeout or a transport failure can never show the caller "declined".
- **Bundle evidence** (calling-pluginless.js). The client routes an incoming end to `this.rejectIncomingCall(a,l)`:

  ```
  sendDeleteRequest({url:this.links[I.LINKS.REJECT],requestName:dt.REJECT_CALL.name,payload:mM(this,t),…})
  ```
  The POST-versus-DELETE divergence belongs to **incoming-and-conversation** and is untouched here.
- **Status:** a local consent decision, not a wire change.

### 4.10 Cancel an outgoing ring nobody answered (M10, gated)

- **Today:** outgoing Dialing is ALREADY abandoned locally after 60 s with nothing posted (`expire_unanswered_call`, `src/bin/server.rs:15168-15186`). Later frames are dropped by the `mine` check.
- **Measurement first** (§ 9). In a `call-live` run the authorized person lets it ring and reports two things: (a) whether a service ending frame arrives before 60 s, and when; (b) whether their device is still ringing after teams-lite's 60 s local drop.
  - (a) arrives → nothing to build; a doc line.
  - (a) absent and (b) yes → split Dialing out of `expire_unanswered_call`: at the existing 60 s timer, POST `hangup_payload` to `links.hangup()`, then end locally with `CallEndReasonUnanswered`, labelled "Nobody answered." for an outgoing call.
  - (a) absent and (b) no → only the label.
- **Bundle evidence:**

  ```
  cancelOutgoingCall(e,t){…const r={code:isNaN(e?.code)?I.CALL_END_CODE.CANCEL:e.code,subCode:e?.subCode||I.CALL_END_SUB_CODE.SUCCESS,phrase:e?.phrase||I.CALL_END_PHRASE.LOCAL_USER_INITIATED,resultCategories:e?.resultCategories||[hr.Success],…
  ```
  It keeps teams-lite's measured End body rather than introducing 487.
- **Status:** inferred and gated. The incoming expiry stays local-only, and its label stays "The call ended." until **ringing-alerts-history** draws a missed-call notice.
- **Which ticker.** "The existing 60 s timer" is checked by today's 20 s `CALL_KEEPALIVE` ticker; once incoming-and-conversation M6 lands it is checked by the 5 s `CALL_TICK` (README § 3, collision 4). The 60 s bound is unchanged.
- **Consent.** This is a hangup posted by a TIMER: the callee is shown the ring ending and nobody pressed anything. It is therefore listed for the user's approval with the other automatic acts (README § 5, D2; § 12 item 11 below), justified as completing an act the user started.

## 5. Changes by file

The tag says which milestone each change ships in.

| File | Change |
| --- | --- |
| `Cargo.toml` (M5) | Add `"signal"` to tokio's features (line 14). |
| `src/trouter.rs` (M1) | `register()` returns its `StatusCode` and logs it. `CallingChannel.registrar_status: u16`. `Status::Disconnected` gains `no_credentials: bool`, set on a credentials failure (the messaging consumer's match gains `..`). Allocate `ttl` and handshake heartbeat/close are parsed (pure `connection_ttl_secs`, `parse_handshake`). The frame tally and the connection-ended line. `connect_timeout(10 s)`, `.timeout(10 s)` on allocate, handshake and register, `timeout(30 s)` around `connect_async`. Pure `next_backoff(prev, lived)`. |
| `src/trouter.rs` (M2, M8, M9 gated) | `run<P: CredentialProvider + Clone + 'static>`. Pure `registration_extension`. The spawned extension and the third `select!` arm. Echo `2::`, and the watchdog behind `TEAMS_LITE_TROUTER_WATCHDOG`. Refusal handling for the calling role (§ 4.4). |
| `src/trouter.rs` tests | `registration_extension` table: 3600/86400 → Extend 3570; 3600/1800 → ExpireOnly 1800; 3600/None → ExpireOnly 3600; 20/_ → ExpireOnly. `next_backoff`; the handshake parse; the tally. `run_asks_provider_for_fresh_credentials_each_attempt` gains a provider that returns credentials pointing at an unroutable loopback port (`127.0.0.1:1`), so the success branch is exercised with no network. |
| `src/calling.rs` (M4) | **The one signaling transport API** (README § 3, collision 7), consumed by incoming-and-conversation M4a/M4b/M6 and meeting-participation M1, which add none of their own: `post_signal`, `put_signal` and `delete_signal`, each taking a `SignalClass` (`Create`/`Answer`/`End`/`Shutdown`, each with `fn timeout()`), over one private header builder and reply reader (`signal_headers` / `read_signal_reply`, meeting-participation's extraction), with `SignalRefused { status }` carried in the error chain for a definitive 4xx (incoming's). The ring-time and answer-time attach take `Create` and run detached like every other creating request, so a local give-up (incoming's 15 s `ATTACH_TIMEOUT`) is a wait on the task and never an abort; keep-alives take `End`. `place_call` and `join_meeting` pass `Create`; `fetch_relay_credentials` gets 10 s. A lower-level `post_signal_within(…, Duration)` for tests. New `pub const END_REASON_BACKEND_STOPPING = "CallEndReasonBackendStopping"`, pinned to `web/src/lib/call.ts` exactly like `END_REASON_UNREACHABLE` (`src/calling.rs:2733`). |
| `src/auth.rs` (M4) | `pub fn TokenCache::cached(&self, scope) -> Option<String>` (today's private `cached_fresh`). |
| `src/bin/server.rs` — start (M1) | `spawn_calling` becomes a loop. (1) It never mints from its own timer: an attempt happens only with `session_if_fresh()` (new, never rebuilds). (2) Real recovery is driven by `ctx.calling_wake: Arc<Notify>`, which `adopt_session` notifies. (3) The fallback timer goes 5 s → 300 s and only retries a non-session failure. (4) It logs only when the error text changes. (5) It returns when `!calling_available()` or `stopping`. `start_calling(session)` takes the session and returns a typed `CallingStartError { NoSession, NoCallingEndpoints(anyhow::Error) }`. New `CallingPlane.registration: Registration { Starting, WaitingForSignIn, Unavailable, Connecting, Registered, Refused(u16) }` with these transitions: start errors (NoSession → WaitingForSignIn, NoCallingEndpoints → Unavailable); `Disconnected{no_credentials:true}` → WaitingForSignIn; `Disconnected{no_credentials:false}` → Connecting; channel arrival → Registered or Refused(status) (informational). `Status::Connecting` changes nothing, so the state does not flicker every 30 s. It emits on change only. `call_state_payload` adds `"registration": {"state","registrar_status"}` (null when `!calling_available()`) and `"backend_started_ms"` (a `OnceLock` set at the top of `serve()`). `ready` is unchanged. |
| `src/bin/server.rs` — copy (M1) | Both `call_prepare` refusals become `call_prepare: calling is not connected yet — this machine registers for calls on its own once it can; try again in a moment`. The placing path filters on `plane.connected` like the meeting path. Test: a crate scan with whitespace and `\`-continuations normalized finds no `turn it on in`. |
| `src/bin/server.rs` — hangup (M3) | `intent` and `hangup_door`, with a full table test. |
| `src/bin/server.rs` — bounds (M4) | `call_place`, `call_join`, `call_accept` (attach and acceptance) and `call_start_sharing` move their POST-and-merge body into a spawned task; the handler awaits it with `CALL_START_WAIT`. `call_hangup` spawns its POST and waits `HANGUP_WAIT`, then drops locally whatever happened. `hang_up_orphan` and keepalives use `End`. `a_call_answered_for_a_cancelled_start_is_hung_up` (`:17357`) is updated to find the orphan path inside the task. New test `the_pages_call_start_ceiling_outlasts_the_backends_wait`. A behavioural test drives `post_signal_within` against a local `TcpListener` that never answers (bounded, returns Err) and one that answers late after the caller's wait gave up (the answer is still read). |
| `src/bin/server.rs` — stop (M5) | `stopping: Arc<AtomicBool>`. `leave_calls_for_exit(reason)` (§ 4.8) replaces `stop_calling`. The main `select!` gains a signal arm (SIGTERM, SIGINT, SIGHUP); a spawned repeat-logger keeps the later signals ignored. `exit_after_flush()` is shared by the signal arm and the idle branch (`:4993-5000`). `restart_backend`: the supervisor shape calls `leave_calls_for_exit` then exits; the launcher shape only emits `backend_restart` (the launcher's TERM does the rest), so a launcher that never acts leaves calling intact. Call markers: `$XDG_RUNTIME_DIR/teams-lite/calls/<port>` holding `pid=`, `port=`, `phase=`, `since_ms=` (no ids, no names), written by `sync_call_marker()` from `emit_call_state` ONLY when `(call id, holds-the-user)` changes (`CallingPlane.marker` remembers the last write), removed by `leave_calls_for_exit` and when the call stops holding the user, and dead-pid files are cleared at boot (`clear_dead_call_markers`, beside `clear_dead_agent_run_markers`). |
| `src/bin/server.rs` tests (M5) | A behavioural `leave_calls_for_exit` against a local HTTP stub: the hangup and the DELETE are both sent, never a reject, within `SHUTDOWN_BUDGET`, and nothing is sent for a Ringing call. Scans bounded at markers the text cannot contain, each proved by mutation in both directions: the signal arm precedes `exit_after_flush`; `leave_calls_for_exit` names no `rejection_payload` and no `session()` / `tokens.get(`. `a_requested_restart_asks_about_the_agent_and_answers_before_it_goes` is updated (it looks for `leave_calls_for_exit` before `std::process::exit(0)` in the supervisor arm). New `the_stop_budgets_close` reads `launcher/src/backend.ts`, both systemd units, `compose.yaml`, `compose.host-broker.yaml`, the chart's statefulset template and `src/intune_embedded.rs`, and checks: SHUTDOWN_BUDGET + grace (5.25 s) < BACKEND_STOP_GRACE_MS (7 s) < each unit's `TimeoutStopSec` (10 s); the app unit is `KillMode=control-group`; 7 s < Docker's 10 s default for the host-broker file (no `stop_grace_period`); 7 s + `STOP_LOCK_WAIT` + 20 s ≤ `compose.yaml`'s 60 s and the chart's 90 s; 7 s < the chart's 30 s host-mode grace. Marker name/body and dead-marker cleanup with a fake pid. |
| `src/bin/server.rs` — asks (M6) | `fn live_call_here()` reads the plane, not markers. `restart_backend` takes `confirm_call` beside `force`. It answers `{restarted:false, blocked:<first unconfirmed>, blockers:[…], runs}` while any blocker is unconfirmed, and a blocker that APPEARED between presses is asked again. `force` keeps meaning agent runs only, so an older page's `force:true` can never end a call it was not told about. `apply_update(params)` checks the call BEFORE `install_binary`; blocked, it re-emits `update_progress(ready)` (an older page that set "Restarting…" optimistically is put back) and answers `{…progress, blocked:"call"}`. Dispatcher: `"update_apply" => ctx.apply_update(params)`. Order scans: call check < agent count < `RESTART_ANSWER_GRACE`; call check < `install_binary`. |
| `launcher/src/backend.ts` (M5) | `stopGracefully(graceMs = BACKEND_STOP_GRACE_MS = 7000)`: bump the generation, `proc.kill("SIGTERM")`, race `proc.exited` against the grace, then `kill(9)`, then await `exited`. `killChildOnExit`: the first signal sets `gone` and runs `void stopGracefully().finally(() => process.exit(0))`. A repeat within `REPEAT_SIGNAL_MS = 1000` is ignored (control-group and process-group duplicates). A repeat later than that is a human asking twice and KILLs at once. The sync `exit` hook keeps a KILL-if-alive fallback. `restart()` uses the graceful stop. `BackendHandle.stop` becomes `stopGracefully`. Header comments updated. |
| `launcher/src/launch.ts`, `launcher/src/update.ts` (M5) | `stopBackend: () => backend.stopGracefully()`. `relaunch()` stops the BACKEND first (awaited), then the web server, then spawns. Both are still down before the spawn, so the EADDRINUSE ordering holds, and pages on the relay receive the `CallEndReasonBackendStopping` frame. `update.test.ts` pins the new order. |
| `launcher/src/backend-stop.test.ts` (new, M5) | Injected deps: TERM first; KILL only after the grace; a child that exits in time is never KILLed; a repeat within 1 s is ignored; a repeat after 1 s KILLs; the generation bumps before the signal (`backend-supervise.test.ts` stays green). |
| `web/src/lib/call.ts` | `CallStatus.registration?` and `backend_started_ms?`. `callUnavailableReason` / `meetingUnavailableReason` give a sentence per state (§ 7), with today's sentence when the field is absent. `callEndLabel` adds `CallEndReasonBackendStopping` and `CallEndReasonLeftRinging` (""). `CallEndReasonUnanswered` is UNCHANGED until M10. Doc comments lose the Settings-switch history. |
| `web/src/lib/call-failure.ts` | `requestNeverSent(e)` (`not connected`) and `requestOutcomeUnknown(e)` (`timeout: …`, `connection closed`). `answerFailureMessage(error, stillRinging)`. The not-connected mapping (§ 7). The comment at `:163` updated; `call-failure.test.ts:99-101` updated. |
| `web/src/lib/store.ts` (M3) | `answerCall` tracks `posted`, set right before `callAccept`. Before it (or `requestNeverSent`): release media, NO hangup, refresh, then report with "The call is still ringing." only if the refreshed status shows THAT call ringing with `can_accept`. After it: a backend refusal releases media and refreshes; an UNKNOWN outcome keeps the media and reports nothing, and the backend's frames end or connect the call (M4 guarantees an ending within `CALL_START_WAIT`). `startCall` and `joinMeeting` take the same rule for an unknown outcome ("the page acts on a refusal, never on a lost answer"). New `declineCall()` has the same body as `hangUpCall()` (callAttempt bump, stopCallMedia) with intent `decline`; `hangUpCall()` sends `end`. `refreshCallStatus`, on SUCCESS only: if this page holds media and the status has no live call, or a different call id (the id is recorded at `adoptCallMedia`), stop the media and say "teams-lite restarted, so you left the call." when `backend_started_ms` changed, else "The call ended while this page was disconnected." The error path stays non-destructive. |
| `web/src/lib/ws-client.ts` | `callHangup(callId, intent)`; `CALL_START_REQUEST_TIMEOUT_MS` (M4); `restartBackend({force, confirmCall})`; `updateApply(confirmCall = false)`. All still go through `writeRequest`. |
| `web/src/components/call-bar.tsx`, `call-stage.tsx` | The ringing card's Decline (`call-bar.tsx:151-154`) calls `declineCall()`. The stage's End (`call-stage.tsx:894`) keeps `hangUpCall()`. |
| `web/src/lib/maintenance.ts`, `protocol.ts` (M6) | `BackendRestartResult.blocked: "agent" \| "call"` plus `blockers`. `RestartPhase` armed `{runs, call}`; the press params come from the armed state. `restartPhaseFor` handles `call`. |
| `web/src/lib/update.ts`, `components/update-button.tsx` (M6) | A page-local `updateBlockedByCall`, set by a blocked answer and cleared when the call ends or the progress moves. In phase `ready` with it set: label "Restart anyway", the detail line, and action `apply_confirm_call`. |
| `web/mock/server.ts` | `mockCallStatus` adds `registration` (registered) and `backend_started_ms`. `{kind:"calling", registration:"waiting_for_sign_in"\|"unavailable"\|"connecting"\|"refused"}` (`ready:false` for the first three). `call_hangup` mirrors `intent` exactly as `hangup_door`, including the refusal of an unknown value, and records `{intent, declined}` for a `{kind:"call_hangups"}` read hook. `call_prepare`'s refusal text matches the backend's new sentence (`:10215`). `{kind:"call_start", hold:"accept"}` joins `prepare` and `place`. `{kind:"call_start", refuse:"prepare"}` refuses the next `call_prepare` once. `restart_backend` and `update_apply` derive `blocked:"call"` from `mockCall` and honour `confirm_call`; confirmed, they end the mock call with `CallEndReasonBackendStopping` before closing the sockets. `{kind:"calling", lose:true}` drops `mockCall` WITHOUT a broadcast, bumps `backend_started_ms` and closes the sockets (a SIGKILLed or restarted backend as a page sees it). Every hook is reset by `call_invite {reset:true}` / `maintenance {reset:true}`. |
| `web/e2e/incoming-call.spec.ts` | "A failed Answer leaves the call ringing and never declines it". It counts `call_hangups` rather than reading the screen. Then Decline → exactly one `{intent:"decline", declined:true}`. "End during an answer that is still travelling never declines" (`hold:"accept"`). |
| `web/e2e/calling.spec.ts` | The per-state reasons, in flow. A lost backend (`lose:true`) takes the stage down after the reconnect with the restarted sentence. A `call_status` that fails during a live call keeps the media. `callPlace` held past a socket drop keeps the media (unknown outcome). "declines a ringing call without opening the microphone" (`:392`) stays green. |
| `web/e2e/maintenance.spec.ts`, `update.spec.ts` (M6) | Ask in a call; the second press restarts and the stage says "teams-lite stopped, so you left the call."; a blocker that appears between presses re-arms. |
| `web/scripts/preview.ts` | `--call`: `-call-unregistered-{light,dark}`, `-answer-failed-{light,dark}`. `--maintenance`: `-restart-armed-call-{light,dark}`. `--update`: `-update-armed-call-{light,dark}`. Each resets its hooks. |
| `web/scripts/sandbox-live.ts`, `join-live.ts`, `call-live.ts` (M7) | `CONTAINER_ORIGIN = "http://127.0.0.1:19443"`. `--container` is a FRONT choice: beside `--local`/`--released` in join-live, and beside `--local` in call-live (which has no `--released`). The target stays the pinned constant, proved from the app's own state. Preflight PRINTS `registration` ("unknown" from an older backend) and refuses only on `!ready`. No guard change: the scripts are allowlisted by name, and 19443 is a live port to both hooks. |
| `.claude/hooks/guard-live-automation.sh` (M5, M6) | New rule 3a-iii, "a stop may not end a live call". It reads `${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/teams-lite/calls/*`; a marker counts only if its `pid=` answers `kill -0`. While one does, it refuses: `teams-lite-service.sh stop\|uninstall`; `systemctl --user stop\|kill\|disable --now\|mask --now` naming a teams-lite unit or the target; the D-Bus `StopUnit`/`KillUnit` spellings; and `kill`/`pkill`/`killall` naming the backend or the launcher. The message names the call's phase and says the user must be asked. Its header states the blind spots: a kill by pid, and a backend with no `XDG_RUNTIME_DIR`, which in practice is the container, whose stops rules 3d and 3e already refuse. M6 adds a hardening of rule 3a: a `TEAMS_LITE_AGENT_WAIT=<n>` assignment on a service-script command line is refused as `--now` is (it skips the agent wait the same way). |
| `.claude/hooks/guard-live-automation.test.py` (M5, M6) | Cases with a temporary `XDG_RUNTIME_DIR`: a live marker (the test's own pid) blocks each stop spelling; a dead-pid marker and an empty directory allow them; status, logs and journalctl stay allowed; the AGENT_WAIT prefix blocks. |
| `bin/teams-lite-service.sh` (M6) | `CALL_RUN_DIR`. `live_calls_for <unit>` classifies each live marker by the unit in its `/proc/<pid>/cgroup` (`teams-lite-backend.service`, `teams-lite-app.service`, or neither). `wait_for_quiet_call` is bounded by `CALL_WAIT_SECONDS=${TEAMS_LITE_CALL_WAIT:-10800}` and DEFERS at the bound: it exits 0 with a line, building, staging and restarting nothing, and the next run tries again. `cmd_update`: (1) wait for the backend unit's call BEFORE `build_artifacts`, so `BUILD_REV` is read after the wait and is never stale; (2) build; (3) agent wait; (4) wait again for a call that started during the build (defer at the bound); (5) stage and restart. `teams-lite-app.service` is still try-restarted unconditionally, which keeps refreshing an older released binary's frozen broker bus (CLAUDE.md, WHAT THE MIGRATION COSTS), unless a live marker belongs to it; then it is skipped with a line. `--now` skips both waits and the skip. `cmd_status` prints live calls per unit. Header updated. |
| `src/update.rs` tests | `the_installer_waits_before_it_replaces_a_live_artifact` (`:2017`) also finds `wait_for_quiet_call` before `build_artifacts` and before `stage_artifacts`, and finds that the bound defers (no `stage_artifacts` on that path). `the_released_build_runs_beside_the_staged_one_and_never_over_it` (`:1151`) pins the live-call skip of the app unit. |
| `.claude/hooks/sync-service-to-master.sh` | Header guardrail 5 says the update waits for a live call and defers rather than ending it. |
| `packaging/helm/teams-lite/values.yaml`, `README.md`, `packaging/helm/chart.test.ts` (M5, M7) | The grace note (`values.yaml:521-531`) and the README's clean-stop bullet (`:240-247`) say the stop now leaves a live call and hands the registration back, within the grace. `chart.test.ts:1957-1974` pins the new truth: TERM, then wait `BACKEND_STOP_GRACE_MS`, the backend's signal arm, and the words. `README.md:409` is corrected: the backend SIGNALS a call, and the audio flows between the BROWSER and Teams' media relays. New "Calls from a pod" bullets and a troubleshooting entry ("calls never ring here" → `kubectl logs teams-lite-0 \| grep '\[calling\]'`). |
| `packaging/docker/compose.yaml` | The `stop_grace_period` comment counts the backend's own stop (launcher 7 s, then 25 + 20 s for the Intune container = 52 s ≤ 60 s). |

## 6. Gating and consent

**No new RPC, event or method name.** The guard's method list needs no edit, and
`every_gated_method_is_named_in_the_automation_guard` (`src/bin/server.rs:17954`) stays green.
Existing methods change as follows:

- `call_hangup` (`OUTWARD_METHODS`) gains `intent`, which NARROWS it: with `end`, a ringing incoming call posts nothing. An absent intent keeps today's behaviour. It is designed together with media-robustness' `reason` and advanced-call-control's waiting-call `call_id`, as one `call_hangup {call_id, intent, reason}` with one validation table (README § 3, collision 4).
- `restart_backend` and `update_apply` (`MACHINE_METHODS`) gain `confirm_call` and can answer `blocked:"call"`. Both keep the write token and the read-only refusal.
- `call_status` / `call_state` (open) gain `registration`: a state word plus a registrar HTTP status number, with no URL and no token. They also gain `backend_started_ms`: a process start time, not an identity. Both say less than `ready` already implies.

**Read-only backend.** `calling_available()` is false, so there is no start loop, no registration,
no extension, no marker and nothing to leave. The signal arm only exits.

**Consent, act by act.**

1. Registering, retrying, extending and unregistering reach nobody (CLAUDE.md § Audio calls: "registering reaches nobody by itself"). They are the backend's own act, as today's single attempt is.
2. **A decline is server-enforced to come only from the Decline press.** A failed Answer, a page timeout, a transport failure, a shutdown or a timer can never show the caller a decline. That is stricter than the real client, which declines a ringing call on page close.
3. **Leaving the call at stop is one of the stated exceptions to "nothing in this feature acts on its own"**, and it is not the only one: the full list, which other designs add to (relink, reconnect, the 409 redirect, the ring-time claim, the outgoing ring cancel of M10, the keep-alives), is put to the user one by one in README § 5, D2. It applies only to a call the user is already IN (Dialing, Connecting, Connected), never to a ring. Who can cause that stop:
   - **the user's own stop**: `systemctl`, `docker`/`kubectl` stop, Ctrl+C, a Settings restart or an update pressed twice. The call dies either way, and the honest alternative to telling the service is a phantom participant everybody sees until the service times out. The real client leaves on page close (§ 4.8).
   - **the idle shutdown**, listed explicitly. In the launcher shapes the launcher's own keepalive socket is a client and reconnects every second (`launcher/src/launch.ts:139-170`), so the idle shutdown fires only when the launcher itself is gone, or for a hand-started backend with no page for 10 s. In that state no surface can end, mute or control the call any more, and the process is leaving whatever happens.
   - **tooling**: REFUSED by the new guard rule 3a-iii while a live call marker exists. A stop with no live call stays tooling's, as § The always-on service says.
4. **The automatic re-stage never ends a call.** `update` waits for the staged backend's call, and at the bound it DEFERS rather than proceeds. Only `update --now` proceeds, and that is the user's switch (already refused to tooling, `guard-live-automation.sh:2172-2186`). A call on the released build delays nothing; that unit's restart is skipped instead.
5. The gated outgoing ring cancel (M10) ends a ring this machine started for the user's own click, and ships only if the measurement shows the service leaves the callee ringing. It is a timer-driven hangup the callee is shown, so it is on the README § 5 D2 list and in § 12 item 11 below, never shipped on this argument alone.
6. **Restart and update name what they will cut.** The confirmation carries `confirm_call` separately from `force`, and the backend re-checks the blockers on every press. A call started between the two presses is asked about again.

The mock mirrors every refusal and decision: `intent` (including the unknown-value refusal),
`blocked:"call"` with `confirm_call`, the registration states and the new `call_prepare` sentence.
Live verification goes only through join-live and call-live (a `--container` FRONT aims nothing
new) or the user's own click. Starting a send-capable container or pod stays the user's (guard
rules 3d and 3e).

## 7. UI and copy (including mobile)

Every sentence is drawn in flow; a phone has no hover.

**A disabled call row and Join button** (`callUnavailableReason` / `meetingUnavailableReason`):

| State | Sentence |
| --- | --- |
| `waiting_for_sign_in` | "Calls connect on their own once sign-in works again." |
| `unavailable` | "This account's Teams directory names no calling service, so this machine cannot take calls." |
| `starting`, `connecting` | "Connecting for calls…" (no promise of seconds: a network can be down for hours) |
| `registered`/`refused` with `ready:false` | "This machine is not registered for calls yet." |
| no `registration` field (an older backend) | "This machine is not registered for calls yet." (unchanged) |
| `enabled:false` | unchanged ("This window cannot take calls, so it cannot place one." / "… cannot join a meeting.") |

**The ringing card after a failed Answer.** The card STAYS (Answer and Decline), and an error
notice (`CALL_NOTICE`, stacked over the card through `--notice-inset-bottom`) says
`callFailureMessage`. It adds " The call is still ringing." only when the re-read status shows that
call ringing and answerable. For example: "teams-lite could not open the microphone. Allow it for
this site, then try again. The call is still ringing." When the call stopped ringing meanwhile,
the sentence is the failure alone.

**End notices** (`callEndLabel`):
- `CallEndReasonBackendStopping` → "teams-lite stopped, so you left the call."
- A reconnect that finds no call while this page held media → "teams-lite restarted, so you left the call." when `backend_started_ms` moved, else "The call ended while this page was disconnected."
- `CallEndReasonLeftRinging` → "".
- `CallEndReasonUnanswered` → unchanged ("The call ended.") until M10. With M10, an outgoing call gets "Nobody answered."

**Backend refusal through `call_prepare`** → "Calls are not connected on this machine yet. They
connect on their own — try again in a moment."

**Settings › This app, the restart row armed** (label "Restart anyway"):
- call alone: "You are in a call. Restarting the backend takes you out of it, and a one-to-one call ends for both of you."
- call and agent: "You are in a call, and a local agent is writing a reply. Restarting takes you out of the call and ends the reply where it stands; the thread is told it was interrupted."
- agent alone: unchanged.

**Sidebar update button, ready and refused by a call:** label "Restart anyway", with the detail
line "You are in a call. Updating restarts the app and takes you out of it." It never names a
commit (`update.test.ts` already scans for that).

**Mobile.** The ring card and its notice keep the measured inset, so the notice never covers
Answer. The armed lines wrap inside their rows without moving the button the user is aiming at
(the update button's box is pinned by `update.spec.ts`). Nothing is drawn for `refused` before
M9; whether M9 adds a quiet line is § 12.

## 8. Tests, mock and captures

- **Rust (`cargo test`):** everything listed in § 5. That includes the trouter tables, `hangup_door` over every direction × phase × intent (absent included), the registration transitions, the `call_state` shape, the start-loop behaviour (a fake provider proves the timer path never mints and that a wake attempts at once), the whitespace-normalized copy scan, the bounded and late-answer `post_signal_within` tests, the detached-start orphan test, the behavioural `leave_calls_for_exit` against a stub, the order scans, `the_pages_call_start_ceiling_outlasts_the_backends_wait`, `the_stop_budgets_close`, and the update.rs script pins. Every scan is bounded at markers its own text cannot contain, asserts exact spellings, and is proved by mutation in both directions, with the mutation's landing checked (CLAUDE.md § A COMPANION's sixteen defects).
- **Launcher (`bun test`, `bun run typecheck` in `launcher/`):** `backend-stop.test.ts`, `update.test.ts` (new order) and `backend-supervise.test.ts`.
- **Chart (`packaging/helm/check.sh`):** the rewritten stop-cost test and the README and values words. Run `cargo test` too, since `packaging/docker/` files are read by Rust tests.
- **Web (`bun run test`, `bun run typecheck`):** `call.test.ts` (reasons per state, the absent field, labels), `call-failure.test.ts` (never-sent and unknown outcomes, the still-ringing condition, the mapping), `maintenance.test.ts` (armed call / call+agent / agent and the press params), `update.test.ts` (armed by call, no commit).
- **E2E (`bun run test:e2e`, with free ports `E2E_MOCK_PORT`/`E2E_WEB_PORT` and stale listeners killed first):** the specs in § 5. Each resets its hooks, because one mock process serves the run.
- **Python:** `guard-live-automation.test.py` (new stop rule and AGENT_WAIT cases) and `sync-service-to-master.test.py`.
- **Captures:** `cd web && bun run preview -- --out /tmp/call --call --dpr 3` (unregistered, answer-failed, both themes); `--maintenance` (restart-armed-call); `--update` (update-armed-call).

## 9. Live verification plan

| Step | Driver | Who acts | Success signal |
| --- | --- | --- | --- |
| M1 on the always-on service: the status is read; socket lifetime and frames recorded | `bin/teams-lite-service.sh update`, then `journalctl --user -u teams-lite-backend -u teams-lite-app \| grep -E '\[calling\]'` | agent | `registering as SkypeSpacesWeb_2.6 …`, `registered: 2xx (ttl 3600 s)`, `connected`; later `connection ended after …s (allocate ttl …; heartbeat … close …; frames …)`. A non-2xx stops M9 and becomes the next question. |
| M1: recovery after a sign-in outage, with no restart | The user's own `intune-container stop && intune-container start` (refused to tooling), or the next real outage; then the journal | user (cycle), agent (journal) | `could not start yet` once (or `registration` going `waiting_for_sign_in`), then `registering as …` within seconds of the messaging feed's recovery, no `[lifecycle] restarting`, and the call row enabled without a reload. |
| M1: the embedded image | The user's own `docker compose -f packaging/docker/compose.yaml up -d`; `docker logs teams-lite 2>&1 \| grep -E '\[calling\]\|\[ok\]\|\[offline\]'` | user, agent | `[offline]` at boot, then `registering as …` and `registered: 2xx` once the broker answers, with no container restart. |
| M2 gate, and M2 itself | Leave the service up for 75 min or more; read the lifetime lines; then the authorized colleague rings the account. M2 then runs with the user's `TEAMS_LITE_CALLING_REREGISTER=1` in backend.env, plus a second run with a `join-live --local --hold` call spanning the extension | agent (journal, driver); the user arranges the colleague; the colleague's click | Either sockets never live 3570 s (M2 closed), or `re-registered: 2xx on the same connection after 3570s`, the call rings after 75 min, and the held call survives the extension. |
| M3: a failed Answer never declines | The user blocks this site's microphone permission in their browser, and the authorized colleague calls; the user presses Answer | user, colleague | The notice ends "The call is still ringing."; the colleague keeps ringing and is never shown "declined"; the user answers on another device; the journal shows no reject POST. |
| M5/M6: a restart during a meeting asks first, then leaves cleanly | `cd web && bun run join-live -- --local --hold 240`; during the hold the user presses Settings › This app › Restart twice in their own tab | agent (driver), user (presses) | The armed sentence on the first press. After the second: `[lifecycle] SIGTERM — leaving the call…` then `the call is over: CallEndReasonBackendStopping`; join-live reports that reason; the roster seen from the user's phone shows the account leaving at once. |
| M5: the DIALING shutdown hangup | `cd web && bun run call-live -- --local`; while the authorized person's phone rings, the user presses Restart twice | agent, user, person | The person's ringing stops at once and shows a missed call, which measures `hangup_payload` during Dialing. If it keeps ringing, M10's measurement inherits that finding. |
| M5: an INCOMING connected shutdown (measures `callLeg`, shared with **incoming-and-conversation**) | The colleague calls; the user answers in teams-lite, then presses Restart twice | user, colleague | The colleague's call ends at once. If not, it is recorded under incoming-and-conversation's callLeg gap. |
| M6: the re-stage waits | While a `bun run call-live -- --local` call is up, run `bin/teams-lite-service.sh update` | agent, person | `Waiting for the call to end…`; it builds, stages and restarts only after call-live hangs up; the call is never cut. A second run with `TEAMS_LITE_CALL_WAIT=60` defers with a line and changes nothing. |
| M5 in a container: a stop hands the registration back | The user's own `docker compose -f packaging/docker/compose.yaml stop` (no call up); then the colleague calls | user, colleague | `[calling] unregistered <epid> — calls stop ringing here` BEFORE the Intune container's stop lines; the colleague's call rings only the user's other devices. |
| M7: calling from the image | With the user's container up: `cd web && bun run join-live -- --container`, then `bun run call-live -- --container` | agent; user (start); authorized person | join-live prints `media: connected/connected via …` with RTP sent; on call-live both people hear each other and End ends it for both. |
| M7: Kubernetes | The user's own `helm install …` and `kubectl port-forward -n <ns> pod/teams-lite-0 19443:19443`; `bun run join-live -- --container`; `kubectl logs teams-lite-0 \| grep '\[calling\]'`; later the user's own `helm upgrade` (pinned `--version`, § Running teams-lite on Kubernetes) | user, agent | The same signals; on the upgrade the OLD pod logs the unregister within its grace, and the new pod registers under the same epid (the data PVC). |
| M8 gate | A week of `connection ended … frames …` lines; then the user's `TEAMS_LITE_TROUTER_WATCHDOG=1` | agent (reading), user (switch) | Ack or heartbeat counts track the pings; with the switch on for a week, no `reconnecting after missed pings` while the connection is really alive. |
| M10 gate | `cd web && bun run call-live -- --local`; the person lets it ring out and says whether their device still rings after 60 s | agent, person | Whether a service ending frame arrives before 60 s, and whether the callee still rings after teams-lite's local drop, decide which branch of § 4.10 is built. |

## 10. Milestones

Each milestone ships alone. Dependencies on other workstreams are named in bold.

1. **Calling recovers by itself, and says where it stands.** The wake-driven start loop that never mints from its own timer, the registration states, the registrar status / ttl / handshake / frame diagnostics (log-only), trouter request bounds, the backoff reset, the copy and the placing-path parity, plus the page reasons, mock, e2e and captures. *Effort S. Risk low:* no wire change and `ready` unchanged; the trouter bounds also touch messaging and use the client's own numbers. *Depends on:* nothing.
2. **Extend the registration on a long-lived socket** (gated by M1's lifetime lines; behind a switch). *Effort S. Risk low to medium:* the same POST, off the read loop. *Depends on:* M1.
3. **A failed Answer never declines; a lost answer is not a refusal; a reconnect reconciles.** `intent` and `hangup_door`, `declineCall`, the answer and start rules, the reconcile on a successful status, the mock, e2e and captures. *Effort S/M. Risk low:* Decline is pinned by the existing and new e2e, and an older page keeps today's behaviour. *Depends on:* **incoming-and-conversation** for the callLeg door used by an End during an in-flight accept (shipping does not wait), **media-robustness-devices** (`media-owner-lost` owns the rest of the reconnect story).
4. **(Landed as PR4 — see § 4.6, "As implemented".) Bound every signaling request; never cancel one whose answer matters.** The classes, detached creating ops and hangup, the bounded waits, the page ceilings pinned across the boundary, and **the one transport API** (`post_signal` / `put_signal` / `delete_signal` with a `SignalClass`, the extracted header builder and reply reader, `SignalRefused`), which lands before incoming-and-conversation M4a/M4b and meeting-participation M1 because both build on it (README § 3, collision 7). *Effort M. Risk medium:* it restructures the three verified handlers without changing a byte on the wire, guarded by the orphan test, a stub-server test and the existing call e2e. *Depends on:* **video-send** M1 (golden pins; shares `call_start_sharing`'s class; coordinate the edit).
5. **A stop leaves the call and hands the registration back; tooling cannot stop a backend in a call.** The signal arm with repeats ignored, `leave_calls_for_exit` (cached credentials, concurrent requests, budget, flush, `exit`), the idle and supervisor paths, the launcher's graceful stop and new relaunch order, call markers, guard rule 3a-iii, the chart, values, README, compose comment, chart test and cross-process budget tests, and CLAUDE.md's stated exception. *Effort M. Risk medium:* lifecycle code on every install, bounded by pinned budgets. *Depends on:* M4 (bounded requests), **incoming-and-conversation** (the incoming hangup door).
6. **Ask before a restart, an update or a re-stage ends a call.** `confirm_call`, `blocked:"call"`, page armed states and copy, the service script's deferring call wait, the app-unit skip, update.rs pins and the AGENT_WAIT guard hardening. *Effort S/M. Risk low:* a phantom call can defer a re-stage until the user ends it or runs `--now`, and `status` names it. *Depends on:* M5 (markers and the leave).
7. **Calling from the container image and Kubernetes, verified and documented.** The `--container` front, the driver preflight, chart README corrections and the CLAUDE.md container and Kubernetes sections. *Effort M. Risk low for code;* the outcome is unknown until the user runs it. *Depends on:* M1, M5.
8. **Detect a half-open calling socket** (gated by M1's counts; behind a switch). *Effort S. Risk medium if wrong:* a false positive reconnects. *Depends on:* M1, **media-robustness-devices** (`trouter-reconnect-keeps-call` lowers the cost of a false positive; the in-call timeout set is theirs to weigh).
9. **Handle a refused registration** (gated by M1's statuses; `ready` untouched, no reconnect). *Effort S. Risk low. Depends on:* M1.
10. **Cancel an outgoing ring nobody answered** (gated by one call-live measurement, and by the user's approval in README § 5, D2). *Effort S. Risk low:* the End press's measured body, at the existing timer (checked by `CALL_TICK` once incoming-and-conversation M6 lands). *Depends on:* M1; **ringing-alerts-history** M5 (soft: it publishes `remote_progress_at_ms`, which this milestone consumes) and the missed-call notice for incoming expiry (coordinate "Nobody answered.").

If **advanced-call-control** adds a hold or park phase, `call_holds_the_user` and
`live_call_here` must include it, or a stop would not leave a held call.

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A status check on the registrar (or a failed re-registration) flips `ready` and blocks outgoing calls that work today | M1 and M2 only log. M9 is gated on 2xx today and never touches `ready`; messaging stays log-only throughout. |
| The trouter bounds or the backoff reset change the messaging live feed | They are the client's own numbers (10 s fetch, 30 s connect); the reset needs a connection that lived 60 s, so a flapping service cannot loop tightly; extension and refusal are calling-only. |
| The calling start loop adds broker load during an outage | Its timer never mints (`session_if_fresh`); recovery rides `adopt_session`, which the messaging feed's own 30 s retry already drives; it logs on change only. |
| Repeated signals cut the stop short | The backend ignores every repeat; the launcher ignores repeats within 1 s and KILLs only on a later one, which is a human's second ask; `KillMode=control-group` is pinned in the budget test. |
| The stop outlives a supervisor's timeout | 5 s budget + 250 ms flush < 7 s launcher grace < 10 s `TimeoutStopSec`, Docker's 10 s, and the chart's 30 s and 90 s, all pinned by `the_stop_budgets_close`. |
| A stop at exit reaches the broker (an interactive rescue with no process left to take its window back) | Cached credentials only; with none, the POSTs are skipped with one line. |
| The final `call_state` frame never reaches the pages | Emitted first, the requests take up to 4 s, then a 250 ms flush; the relaunch now stops the backend before the web server. |
| Detaching the creating ops changes the verified handlers | Bodies, order and orphan logic are unchanged; the orphan test, a stub-server test and the existing 1:1 and meeting e2e pin it; live runs in M7 cover it again. |
| An unknown outcome keeps media for a call the backend refused, with the answer lost | The backend's own ending frame arrives on the next reconnect or within `CALL_START_WAIT`, and the successful-status reconcile stops the media with a sentence. |
| A phantom call (a page that died mid-call) defers every re-stage | Visible in `status` and in the hook's log; one End press, or the user's `--now`, clears it. **media-robustness-devices** (`media-owner-lost`) removes the cause. |
| Re-registering on a live socket disturbs a call | Same registrationId, same surl, off the read loop; behind a switch until a held call spans an extension. |
| The guard rule reads runtime state, which no rule did before | A marker counts only with a live pid; the blind spots (a kill by pid, a backend with no `XDG_RUNTIME_DIR`) are stated in its header; it is pinned by tests with a temporary runtime directory. |
| An older page meets the new answers | No `intent` keeps today's behaviour; an older `force` never confirms a call; `blocked:"call"` re-emits `update_progress(ready)`, and `restartPhaseFor`'s fallback for an unknown block reads "Nothing restarted the backend", which is true. |
| The incoming hangup at stop and after an in-flight accept posts to the unmeasured `callLeg` | A refusal is today's outcome (a local drop) plus a journal line; it is measured in § 9 together with **incoming-and-conversation**. |

## 12. Open questions and decisions the user must make

1. **Measured by M1:** does this tenant's registrar answer 2xx? What `ttl` does the calling allocate state, how long do calling sockets live, and does the handshake carry a heartbeat timeout?
2. **Measured by M10's run:** does the service end an unanswered outgoing ring, and does the callee keep ringing after teams-lite's 60 s local drop?
3. **Decision — the idle shutdown leaves the call.** It is listed as part of the stated exception (§ 6). The alternative is to defer the idle exit while a call holds the user, which keeps a call alive that no surface can control. Do you accept leaving it?
4. **Decision — an End during an answer that is still travelling never declines.** It drops locally, and the accept's own answer is hung up through the attach answer's `callLeg`, which is unmeasured. Today it posts the invite's reject link (also unmeasured live), which shows the caller "declined". Keep the new rule?
5. **Decision — the automatic re-stage defers for as long as a call is live**, and never proceeds on its own after the 3 h bound. A phantom call therefore holds the always-on service on its old build until you end the call or run `update --now`. Acceptable?
6. **Decision — a ringing incoming call does not block a restart or a re-stage.** A ring lasts at most a minute and still rings on your other devices.
7. **Decision — tooling may not stop a backend that holds a live call** (guard rule 3a-iii). This amends CLAUDE.md § The always-on service ("What is yours: … stop") and § Automation safety.
8. Where should a `refused` registration be shown if M9 ships: a quiet line in Settings › This app, the sidebar status line, or nowhere beyond the journal and the drivers?
9. Should the signal arm also withdraw the "Always available" presence registration? It is out of scope here, but the arm is the place.
10. When should the `TsCallingVersion` override be exercised? Coordinate with **video-send**.
11. **Decision — the outgoing ring cancel (M10) posts a hangup on a timer.** If the measurement shows the callee keeps ringing after teams-lite's 60 s local drop, M10 posts the End press's measured hangup at that timer: the callee is shown the ring ending, and nobody pressed anything. It completes an act you started (you placed the call). Do you approve it as one of the automatic acts in README § 5, D2? Without it, and if the measurement shows the callee's device still ringing, the callee keeps ringing after teams-lite has already dropped the call locally, for however long the service lets it (unmeasured).

**CLAUDE.md sections to update with these milestones:**
- § Audio calls (M1, M3, M4, M5, M10). This includes the bullet "It is NOT taken back when the launcher stops its backend … a known gap" and the stated stop exception.
- § A broken sign-in costs the LIVE feed (M1: the calling start loop is wake-driven and never polls the broker).
- § Settings › This app and § Updating the app from inside it (M5, M6).
- § The always-on service (M6: the deferring call wait, the app-unit skip; M5: stop during a call).
- § Automation safety (M5: rule 3a-iii, the `--now` sentence; M6: AGENT_WAIT).
- § Running teams-lite in a container image and § Running teams-lite on Kubernetes (M5: remove the KNOWN GAP; M7: verification, and media flows from the browser).
- § Joining a meeting and § A call is a PAGE (the new end notices).
- NATIVE-CALLING.md § 2.1 (the registration timer facts, with round 1's note corrected) and § 8 "Still open" (registrar status, connection ttl, heartbeats, the Dialing hangup, the unanswered outgoing ring).

## 13. Review notes

- **The review of M4 as built (2026-11-06; four reviewers, a skeptic per finding; 22 agents).** Eight findings survived and were fixed before it landed. (1) THE ACCEPTANCE HAD NO ORPHAN PATH, found by two reviewers: once the attach answered, the call was held, and an acceptance still travelling when the handler's 150 s wait ran out — or when the user hung up — merged into nothing or did nothing, so the caller could be answered by a machine that held nothing. Its late success and its failure now hang the leg up, and the acceptance is not posted for a call that went meanwhile; pinned end to end against stand-in services (`an_acceptance_that_answers_after_its_handler_gave_up_is_hung_up`, with `a_placed_call_whose_handler_gave_up_is_hung_up_on_its_late_answer` beside it), each proved by mutation. (2) reqwest's error printed the FULL signaling URL after the redacted one, so a routine timeout put a link's query in the journal; stripped (`without_url`), pinned with a `?i=secret-token` URL on a silent and on a refused connection, and the relay-token GET takes the same strip. (3) A 2xx whose body was cut off had been made an error, which would have ended a placed call while the callee rang; reverted to the empty answer it always was, with a journal line and a test. (4) The give-up blamed "the calling service" for a wait that, by the pinned arithmetic, was mostly the credential read; it now names no cause. (5) Three negative source scans still looked only for `post_call_signal`, and one window had shrunk to an inner block when the code moved out of a match arm; they now share one `SIGNAL_POSTERS` list and read the whole branch. Ten more findings were refuted by their skeptics (among them: Create's 65 s abort does not stop place and join reaching the orphan path; the ring expiry cannot cut a request inside its bound unfairly; the probe's own join is unchanged).

Two reviews, one on code and protocol accuracy and one on project rules, safety and regressions.
Every claim below was re-checked against the code and the bundle before it was accepted or
rejected.

**Accepted (the design changed):**

- **Repeated signals skipped the stop (both reviewers, blocker/major).** Confirmed. `KillMode=control-group` with `KillSignal=SIGTERM` (`teams-lite-app.service:118-122`), a non-detached spawn (`backend.ts:420-425`), and `killChildOnExit` handling SIGHUP too. The backend now ignores repeats and handles SIGHUP; the launcher ignores repeats within 1 s; `KillMode` is pinned.
- **The outgoing ring is already expired at 60 s (both).** Confirmed (`server.rs:15168-15186` matches `Ringing | Dialing`). M10 was rewritten around the existing timer; "Nobody answered." moved to M10; the incoming label stays "The call ended." until ringing-alerts-history.
- **The per-POST bound raced the page ceiling, and a timeout loses a creating answer (both).** Confirmed (`ws-client.ts:228`, two sequential POSTs in `call_accept`, orphans fed by the answers at `:7707/:7787/:7878`). Checking it found a sharper, existing hazard: `serve_conn` keeps RPC futures in a local `FuturesUnordered` (`:5176-5214`), so a dropped page socket already cancels a creating POST mid-flight. M4 now detaches the creating ops and the hangup, bounds the handler's WAIT rather than the request's answer, sets longer page ceilings for the four start RPCs (pinned), and keeps a transport bound at or above the client's operation bounds.
- **Credentials can reach the broker inside a bound (both).** Accepted. The credential phase is never wrapped in a dropping timeout (a rescue's window must be taken back, `auth.rs:587-640`); the shutdown uses cached credentials only.
- **`refreshCallStatus` through `onCallState` would kill media on a transient error (code review).** Confirmed (`store.ts:3025-3031`, `call.ts:160`). Only a successful status reconciles, and only when the call is gone or different. That also avoids killing an answer in flight, whose backend phase is still `ringing` (`holdsMicrophone`, `call.ts:175-177`).
- **The shutdown ran sequentially past its budget; no flush; the relaunch stopped the web server first (both).** Accepted: concurrent requests, a 5 s budget, a flush, `process::exit`, and the backend stopped before the web server in `relaunch()`.
- **Tooling stops become an outward hangup (both).** Accepted and decided rather than left open: guard rule 3a-iii, landing with the signal arm.
- **The automatic re-stage would end a call after 3 h; stale build; the app-unit restart (both).** Accepted with changes. The call wait runs BEFORE the build and defers at the bound; markers are classified by unit through the pid's cgroup; the app unit is still restarted unconditionally (keeping the frozen-bus refresh) unless it holds a call; markers are written on transitions only.
- **M2 inside the read loop, and its no-ttl reading (both).** Confirmed (`trouter.rs:393-416`; `getRegistrationTtl` returns `[registrarTtlSec, false]` without a connection ttl, and `startRegistrationTimer` then arms only an expiry). The extension is spawned, follows the client exactly, and is gated by M1 and a switch; deviating for an unknown ttl needs evidence.
- **The start loop's timer would mint through the broker (both).** Confirmed (`session()` → `teams::connect`, `:4287-4299`). The timer only acts on a fresh cached session; recovery rides `adopt_session`.
- **Mixed states and over-promising copy (both).** Accepted: `Unavailable` for a directory without calling endpoints, `no_credentials` on `Disconnected`, and "Connecting for calls…" with no promise.
- **"Still ringing" and "restarted" could be false (both).** Accepted: the first is conditional on a re-read status; the second needs `backend_started_ms` to have moved, otherwise the neutral sentence.
- **The copy scan proved nothing (both).** Confirmed (the literal is split at `server.rs:7411-7412`, `:7517-7518`); the scan normalizes continuations and reaches `web/src` and `web/mock`; the mock's sentence and the new `intent` refusal are mirrored.
- **One `force` for two blockers (safety review).** Accepted: `confirm_call` is separate, and blockers are re-checked per press.
- **Weak scans and missing budgets (safety review).** Accepted: a behavioural stop test against a stub, mutation-proven scans, and the host-broker compose file and the chart's host-mode grace in the budget test.
- **Citations (code review).** Corrected: `spawn_calling` is at `:4938`; the extension is the THIRD `select!` arm; call-live has no `--released`; the `TsCallingVersion` functions are `On()`/`UH()` and `Kn()`/`x7()`; M9's reconnect was not the client's rule and is dropped; the extended trouter test uses an unroutable loopback port; the driver preflight treats an absent `registration` as unknown and refuses only on `!ready` (safety review too).
- **The handshake already carries the heartbeat (code review).** Accepted: M1 logs it, and it decides M8 sooner than a week of counts.
- **Dialing's shutdown hangup is unmeasured (code review).** Accepted: marked unmeasured, with a live Dialing step.
- **Docs (safety review).** Added § A broken sign-in, § Automation safety, § The always-on service and the § Audio calls known-gap bullet.

**Rejected or changed in the other direction:**

- **"After `posted`, call `hangUpCall('end')` on any failure" (safety review).** Rejected in favour of the code review's alternative. A timeout or a closed socket says nothing about whether the accept worked, the backend's accept now survives the page (M4), and hanging up would end a call that is about to connect. The page keeps its media and lets the backend's frames decide. M4 guarantees an ending frame within `CALL_START_WAIT` if the accept never completes, and the successful-status reconcile covers a page that missed it. The safety review's scenario (caller connected to silence) is exactly what keeping the media prevents.
- **"Idle shutdown happens when a page's socket blips" (safety review).** Factually narrowed. The launcher's keepalive socket counts as a client and reconnects every second (`launch.ts:139-170`), so in the launcher shapes the idle shutdown fires only when the launcher is gone. The suggestion to DEFER it was not taken, because a deferred backend keeps a call alive that no surface can end. It is instead listed in the stated exception (§ 6) and put to the user (§ 12, item 3).
- **"Guard-refuse `TEAMS_LITE_CALL_WAIT=<n>`" (safety review).** Not needed. The call wait DEFERS at its bound rather than proceeding, so a shorter value can only defer sooner and can never end a call. The `TEAMS_LITE_AGENT_WAIT` half of the same finding is a real, older hole and is fixed in M6.
- **"A per-POST bound of 10-12 s" (code review's first option).** Not taken. It would sit below the client's own 35 s and 65 s operation bounds and lose answers. The longer page ceiling, its own suggested alternative, was taken instead.
- **"Keep today's decline for an End during an in-flight accept" (code review's first option).** Not taken: `end` never declines. The trade (an unmeasured `callLeg` orphan hangup against an equally unmeasured reject that shows "declined") is stated and put to the user (§ 12, item 4).
- **"Proceeding after the bound should stay a manual-`update` behaviour, if at all" (safety review).** Taken to its strict end: it never proceeds. Only `--now`, which is the user's, proceeds.

**Where M1 as built differs from § 5, and why** (found by the implementation review; AGENTS.md
§ Audio calls carries the same):

- **No fallback timer in the start loop.** `start_calling`'s one failure is `calling::endpoints`,
  a pure read of the session it is handed, and `adopt_session` is the only writer of the session
  cell and always rings `calling_wake` — so a timer could only re-run the same function on the same
  input. Both waiting states wait on the wake alone. `CallingStartError` has no `NoSession`
  variant: a missing session is the loop's to notice before it calls a start.
- **`unavailable` holds until a new session reads the directory again**, including through a
  sign-in outage that follows it, because nothing newer exists to contradict the last directory
  read. The outage itself is the sign-in banner's to say.
- **`no_credentials` is a SIGN-IN failure, not every credentials failure.** A provider error whose
  chain carries a `reqwest::Error` (the authz POST a stale session is rebuilt with; the broker is
  D-Bus) is the network: it reads `connecting` and logs `could not get a session`. Both
  credentials lines go through the repeat filter like every other pre-socket failure.
- **The calendar's and the incoming-call card's Join pill draw their reason in flow**
  (`MeetingJoinReason`), and their label is the action plus the reason, as § 7 asks of every
  disabled call or Join control.
