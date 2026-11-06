// Real-time trouter client as a library module (slice 6).
//
// Promoted from the proven src/bin/trouter.rs spike. Instead of printing, it emits
// parsed chat `Message`s and lifecycle `Status` over channels, and reconnects with
// backoff. The backend spawns `run` and persists each emitted message into the
// store, then tells every client — that's live delivery, end to end.
//
// Flow (reverse-engineered from EionRobb/purple-teams teams_trouter.c):
//   1. POST go.trouter.teams.microsoft.com/v4/a?epid={epid}  (x-skypetoken)
//        -> { socketio, surl, connectparams, ccid? }
//   2. GET  {socketio}socket.io/1/?v=v4&{connectparams}&...   -> sessionId
//   3. WS   wss://{socketio}socket.io/1/websocket/{sessionId}?...  (+X-Skypetoken)
//   4. on "1::" -> user.authenticate (Bearer ic3) + user.activity + registrar POST
//   5. messages arrive as "3:::{...}"; ack every request, then decode
//
// TWO connections, not one, because that is what the real web client runs: the
// messaging worker (chat, typing, read receipts) and — only when the user turned
// calling on — the CALLING worker, which is a connection of its own to its own
// regional host, registered under its own template (see [`Endpoint::calling`] and
// NATIVE-CALLING.md § 2.1). A `Role` says which of the two a connection is, so the
// handshake, the acking and the reconnect backoff are written once.
//
// Every step of an attempt is BOUNDED, at the real web client's own numbers (see
// `FETCH_TIMEOUT`), and every attempt is REPORTED in the journal (see `run`): a step that
// hung used to stop the reconnect loop for good, and an attempt that failed left no trace.
//
// No raw tokens are ever logged (Status carries only human-readable state, and every
// request error is stripped of its URL before it is logged).

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::store::Message;
use crate::teams::Session;

const TROUTER_BEGIN: &str = "https://go.trouter.teams.microsoft.com/v4/a";
/// The registrar every endpoint registers with. Public because the calling plane
/// falls back to it when the directory does not name `calling_registrarUrl`.
pub const REGISTRAR: &str = "https://teams.microsoft.com/registrar/prod/V2/registrations";
const TCCV: &str = "2024.23.01.2";
const CLIENT_VERSION: &str = "1415/26061118216";
const UA: &str = crate::USER_AGENT;

/// The messaging worker's registration — the one this app has always used.
const MESSAGING_APP_ID: &str = "TeamsCDLWebWorker";
const MESSAGING_TEMPLATE: &str = "TeamsCDLWebWorker_2.1";
/// A day: a messaging endpoint is long-lived and re-registered on every reconnect.
const MESSAGING_TTL: u64 = 86400;

/// The bounds on one connection attempt, and they are the real web client's own numbers
/// (`timeoutOptions` in cmd-trouter.js: `fetchTimeoutMs:1e4`, `connectionTimeoutMs:3e4` —
/// see docs/calling-roadmap/plane-reliability.md § 4.2).
///
/// They exist because NOTHING bounded an attempt before: the allocate POST, the socket.io
/// handshake, the registrar POST and the websocket upgrade could each wait for ever, and a
/// connection loop stuck inside one of them never reaches its own reconnect. A hung allocate
/// therefore took the live feed — or the calling plane — down for the life of the process,
/// with nothing in the journal. Each request carries its OWN timeout rather than the client
/// carrying one: a client-wide total would also cut the websocket's long life, which is not a
/// request at all.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Establishing the TCP (and TLS) connection under any of those requests.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The websocket upgrade, which the client bounds with its `connectionTimeoutMs`.
const WEBSOCKET_TIMEOUT: Duration = Duration::from_secs(30);

/// The reconnect backoff's floor and ceiling, in seconds. The ceiling is the one this loop
/// has always had (the client's own `maxBackoffMs` outside its slower tier is 3e4).
const MIN_BACKOFF_SECS: u64 = 1;
const MAX_BACKOFF_SECS: u64 = 30;
/// A connection whose websocket stayed open at least this long was a GOOD one, so the next
/// attempt starts from the floor again rather than from wherever an outage days ago left the
/// backoff. Without it a socket that dropped after a week reconnected up to 30 s late, which
/// for the calling plane is 30 s in which an incoming call rings nowhere.
const LONG_LIVED: Duration = Duration::from_secs(60);

/// The CALLING worker's registration, taken from the real web client's own
/// configuration rather than from the desktop client's: `pnhTemplate` is
/// `SkypeSpacesWeb_2.6` and `webRegistrarTtlInSeconds` is 3600
/// (config-prod-<hash>.js — see NATIVE-CALLING.md § 2.1).
///
/// The desktop client's `NGCallManagerWin` / `DesktopNgc_2.3` pair is deliberately
/// NOT used: its path suffix belongs to a Windows endpoint, and Teams routes a call
/// to the endpoints it believes are running.
const CALLING_APP_ID: &str = "SkypeSpacesWeb";
const CALLING_TEMPLATE: &str = "SkypeSpacesWeb_2.6";
const CALLING_TTL: u64 = 3600;

/// Which trouter this connection talks to, and how it registers there.
///
/// Two instances exist: [`Endpoint::messaging`], hard-coded to the global allocate
/// host this app has always used, and [`Endpoint::calling`], built from the calling
/// URLs in the authz directory so the connection lands on the user's own region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The allocate URL (`…/v4/a`), which the connection POSTs to first.
    pub allocate: String,
    /// The registrar the registration is POSTed to.
    pub registrar: String,
    pub app_id: &'static str,
    pub template_key: &'static str,
    pub ttl_secs: u64,
}

impl Endpoint {
    /// The messaging worker: chat, typing and read receipts.
    pub fn messaging() -> Self {
        Self {
            allocate: TROUTER_BEGIN.to_string(),
            registrar: REGISTRAR.to_string(),
            app_id: MESSAGING_APP_ID,
            template_key: MESSAGING_TEMPLATE,
            ttl_secs: MESSAGING_TTL,
        }
    }

    /// The calling worker, addressed from the directory's own `calling_trouterUrl`
    /// and `calling_registrarUrl`.
    pub fn calling(trouter_url: &str, registrar: &str) -> Self {
        Self {
            allocate: allocate_url_for(trouter_url),
            registrar: registrar.to_string(),
            app_id: CALLING_APP_ID,
            template_key: CALLING_TEMPLATE,
            ttl_secs: CALLING_TTL,
        }
    }
}

/// Map a trouter URL from the directory onto the allocate endpoint this client
/// speaks.
///
/// The directory states the CONNECT form (`https://go-eu.trouter.teams.microsoft.com/v3/c`)
/// while this client speaks the allocate-then-socket.io flow (`…/v4/a`) that the
/// messaging connection has used all along. The real client converts between the
/// two forms the same way — it rewrites `/v4/c` to `/v4/a` and back — so only the
/// host matters, and keeping the directory's host is what puts the connection in the
/// user's own region. A URL that already names an allocate path is left alone.
pub fn allocate_url_for(trouter_url: &str) -> String {
    let trimmed = trouter_url.trim_end_matches('/');
    if trimmed.ends_with("/v4/a") {
        return trimmed.to_string();
    }
    // Keep scheme + host, drop whatever version path the directory stated.
    let without_scheme = trimmed.strip_prefix("https://").or_else(|| trimmed.strip_prefix("wss://"));
    match without_scheme {
        Some(rest) => {
            let host = rest.split('/').next().unwrap_or(rest);
            format!("https://{host}/v4/a")
        }
        // Not a URL we recognise: fall back to the host that is known to work rather
        // than to something malformed.
        None => TROUTER_BEGIN.to_string(),
    }
}

/// The live calling connection's own address, published upward the moment it is
/// registered.
///
/// The signaling layer needs it: every callback link a call publishes is built on
/// this surl (`{surl}callAgent/{sessionId}/{causeId}{path}`), so a call cannot be
/// placed or answered before this arrives — and a reconnect that changes the surl
/// invalidates the links of any call still up, which is why it is a channel rather
/// than a one-shot.
///
/// It is published once the registrar has ANSWERED, whatever it answered: a non-2xx is
/// carried in `registrar_status` and changes nothing else yet. That status had never been
/// read by this app, so whether this tenant's registrar refuses anything is unmeasured, and
/// acting on a refusal before it is measured would risk blocking calls that work today
/// (plane-reliability.md § 4.4 is the gated follow-up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallingChannel {
    /// The trouter surl this connection was allocated.
    pub surl: String,
    /// The registration id (endpoint id) it registered under.
    pub endpoint_id: String,
    /// The registrar's HTTP status code for that registration.
    pub registrar_status: u16,
}

/// What a connection does with the pushes it receives.
///
/// One enum rather than two `run` functions with different parameter lists: the
/// handshake, the ack and the backoff are identical, and the only real difference is
/// where a decoded push goes.
pub enum Role {
    /// The messaging worker. `calls` still exists here to catch a calling push that
    /// arrives on this socket anyway (the service has routed one before), so a frame
    /// is never silently dropped.
    Messaging {
        events: mpsc::UnboundedSender<Vec<Message>>,
        typing: mpsc::UnboundedSender<crate::trouter_events::TypingEvent>,
        receipts: mpsc::UnboundedSender<crate::trouter_events::ReadReceiptEvent>,
        calls: mpsc::UnboundedSender<crate::trouter_events::CallFrame>,
    },
    /// The calling worker. Every push here is calling traffic — the invite, the
    /// media answer, the roster, the ending — so nothing is filtered by URL.
    Calling {
        frames: mpsc::UnboundedSender<crate::trouter_events::CallFrame>,
        channel: mpsc::UnboundedSender<CallingChannel>,
    },
}

impl Role {
    /// Decode one push and fan it out. Returns false when the consumer is gone, so
    /// the connection can stop instead of decoding for nobody.
    fn deliver(&self, request: &Value) -> bool {
        match self {
            Role::Messaging { events, typing, receipts, calls } => {
                let Ok(rt) = crate::trouter_events::realtime_from_request(request) else {
                    return true; // a frame we cannot decode is not a reason to hang up
                };
                if !rt.messages.is_empty() && events.send(rt.messages).is_err() {
                    return false; // consumer gone
                }
                // Typing, receipts and calls are best-effort: a dropped receiver must
                // never take down the chat stream.
                rt.typing.into_iter().for_each(|t| {
                    let _ = typing.send(t);
                });
                rt.read_receipts.into_iter().for_each(|r| {
                    let _ = receipts.send(r);
                });
                rt.calls.into_iter().for_each(|c| {
                    let _ = calls.send(c);
                });
                true
            }
            Role::Calling { frames, .. } => {
                match crate::trouter_events::call_frame_from_request(request) {
                    Ok(Some(frame)) => frames.send(frame).is_ok(),
                    // An empty or undecodable body is nothing to act on; the ack has
                    // already gone out, which is what keeps the socket alive.
                    _ => true,
                }
            }
        }
    }

    /// Whether the consumer of this role has gone away.
    fn is_closed(&self) -> bool {
        match self {
            Role::Messaging { events, .. } => events.is_closed(),
            Role::Calling { frames, .. } => frames.is_closed(),
        }
    }

    /// Announce the connection's own address, for the roles that need it.
    fn on_registered(&self, surl: &str, endpoint_id: &str, registrar_status: u16) {
        if let Role::Calling { channel, .. } = self {
            let _ = channel.send(CallingChannel {
                surl: surl.to_string(),
                endpoint_id: endpoint_id.to_string(),
                registrar_status,
            });
        }
    }

    /// The journal prefix this connection's lines carry. `realtime` for messaging is the
    /// spelling every existing line already used, so a reader grepping an old journal and a
    /// new one finds both; `calling` is the calling plane's own, which the server's calling
    /// lines already use.
    fn label(&self) -> &'static str {
        match self {
            Role::Messaging { .. } => "realtime",
            Role::Calling { .. } => "calling",
        }
    }
}

/// Lifecycle signals from the real-time client (for a status line / catch-up hook).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// An attempt is starting. Sent before EVERY attempt, including every retry of an
    /// outage, so a consumer that drew something from it would flicker every 30 s.
    Connecting,
    /// Socket authenticated + endpoint registered — live and receiving. Sent once the
    /// registrar answered, whatever it answered (see [`CallingChannel::registrar_status`]).
    Connected,
    /// The attempt ended — the connection dropped or never came up — and the next one
    /// starts after `retry_in_secs`, which is the sleep really taken (see [`next_backoff`]).
    ///
    /// `no_credentials` says the attempt never got as far as the trouter because the
    /// [`CredentialProvider`] failed FOR SIGN-IN: the cause is the broker or the token, not
    /// the service. A provider whose own request the network did not carry is not that
    /// ([`failed_on_the_network`]) and reads `false`, like any other connection failure. The
    /// calling plane tells a reader "waiting for sign-in" from "connecting" with it, which are
    /// two different next moves.
    Disconnected { retry_in_secs: u64, no_credentials: bool },
}

/// What one connection attempt found out, filled in as the attempt goes so that whatever
/// ended it — an error at any step, the server closing, the consumer going — can be
/// reported in one line with everything it learned.
///
/// The numbers are the ones a later milestone has to decide on (plane-reliability.md
/// § 4.3 and § 4.5): how long the service says an allocation lasts, what heartbeat it
/// expects, how long sockets really live, and which frames they carry. Nothing about them
/// changes behaviour yet; they are measured first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ConnectionReport {
    /// The allocate answer's own `ttl`, in seconds.
    allocate_ttl_secs: Option<u64>,
    /// The socket.io handshake's heartbeat and close timeouts, in seconds.
    heartbeat_secs: Option<u64>,
    close_secs: Option<u64>,
    /// When the websocket opened. `None` means the attempt failed before it did, which is
    /// what decides both how the failure is reported and whether the backoff resets.
    websocket_opened: Option<tokio::time::Instant>,
    frames: FrameTally,
}

/// The socket.io text frames one connection received, by type.
///
/// socket.io v1 names a frame by its first character; the read loop acts on `1` (connect)
/// and `3` (message), and the counts of `2` (heartbeat) and `6` (ack) are what say whether
/// the service is answering the pings this client sends — the question a half-open socket
/// detector has to be built on (§ 4.5). Only TEXT frames are counted: socket.io v1 speaks
/// nothing else, and websocket control frames are the transport's, not the protocol's.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct FrameTally {
    connect: u64,
    heartbeat: u64,
    message: u64,
    ack: u64,
    other: u64,
}

impl FrameTally {
    fn count(&mut self, frame_type: u8) {
        let slot = match frame_type {
            b'1' => &mut self.connect,
            b'2' => &mut self.heartbeat,
            b'3' => &mut self.message,
            b'6' => &mut self.ack,
            _ => &mut self.other,
        };
        *slot += 1;
    }
}

/// The one line a connection whose websocket had opened leaves behind when it ends.
///
/// One line per END rather than per frame or per ping: a calling socket that never rings
/// is diagnosed from how long its sockets lived and what they carried, and that is a fact
/// about the whole connection. An unknown number is written `?` rather than `0`, because
/// "the service stated no ttl" and "the service stated a ttl of zero" are different answers.
fn connection_ended_line(
    label: &str,
    lived: Duration,
    report: &ConnectionReport,
    error: Option<&anyhow::Error>,
) -> String {
    fn secs(value: Option<u64>) -> String {
        value.map_or_else(|| "?".to_string(), |v| v.to_string())
    }
    let f = report.frames;
    let mut line = format!(
        "[{label}] connection ended after {}s (allocate ttl {}s; heartbeat {}s close {}s; \
         frames 1:{} 2:{} 3:{} 6:{} other:{})",
        lived.as_secs(),
        secs(report.allocate_ttl_secs),
        secs(report.heartbeat_secs),
        secs(report.close_secs),
        f.connect,
        f.heartbeat,
        f.message,
        f.ack,
        f.other,
    );
    if let Some(e) = error {
        line.push_str(&format!(" — {e:#}"));
    }
    line
}

/// The sleep before the next attempt, in seconds.
///
/// `prev_secs` is the sleep taken before the attempt that just ended (0 before the very
/// first one), and `lived` is how long that attempt's WEBSOCKET stayed open, or `None` when
/// it never opened. A connection that lived [`LONG_LIVED`] or more was a good one, so the
/// next attempt waits the floor again; anything shorter is a failure in a row, and the
/// sleep doubles up to the ceiling. So the first failure waits 1 s, then 2, 4, 8, 16 and 30
/// — exactly the sequence this loop always had — and a socket that dropped after days is
/// back within a second instead of within thirty.
fn next_backoff(prev_secs: u64, lived: Option<Duration>) -> u64 {
    if lived.is_some_and(|lived| lived >= LONG_LIVED) {
        return MIN_BACKOFF_SECS;
    }
    prev_secs.saturating_mul(2).clamp(MIN_BACKOFF_SECS, MAX_BACKOFF_SECS)
}

/// The allocate answer's `ttl`, in seconds: a number, or a numeric string — the client's own
/// reading accepts both (`typeof e.ttl=="string"?parseInt(e.ttl,10):e.ttl`, in
/// `populateConnectionStateFields`). Anything else is unknown, never zero.
fn connection_ttl_secs(allocate: &Value) -> Option<u64> {
    let ttl = allocate.get("ttl")?;
    ttl.as_u64().or_else(|| ttl.as_str().and_then(|s| s.trim().parse().ok()))
}

/// What a socket.io v1 handshake answered: `sid:heartbeat:close:transports`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Handshake {
    session_id: String,
    /// The heartbeat timeout the service expects, in seconds.
    heartbeat_secs: Option<u64>,
    /// The close timeout, in seconds.
    close_secs: Option<u64>,
}

/// Parse a handshake body. Only the session id is required — the connection cannot be made
/// without it — and the two timeouts are read when they are numbers: they are measured, not
/// acted on, so a service that stopped stating them must not stop the connection.
fn parse_handshake(body: &str) -> Result<Handshake> {
    let mut fields = body.trim().split(':');
    let session_id = fields.next().unwrap_or("").trim().to_string();
    if session_id.is_empty() {
        return Err(anyhow!("empty socket.io session id"));
    }
    let mut number = || fields.next().and_then(|f| f.trim().parse::<u64>().ok());
    let heartbeat_secs = number();
    let close_secs = number();
    Ok(Handshake { session_id, heartbeat_secs, close_secs })
}

/// A fresh set of credentials for one trouter connection attempt.
pub struct Credentials {
    /// The Teams session (carries the skypetoken + region endpoints).
    pub session: Session,
    /// A Bearer token for the ic3 audience (socket authenticate + registrar).
    pub ic3: String,
}

/// Supplies fresh credentials on demand. The trouter calls this before EVERY
/// connection attempt, so a reconnection after a long-lived socket dropped gets
/// a freshly-minted skypetoken and ic3 token instead of the boot-time ones —
/// which is what keeps the real-time feed alive past the ~1h token lifetime.
///
/// This is a dependency-inversion boundary: the trouter states what it needs
/// (fresh credentials) without knowing HOW they are obtained (broker, cache,
/// TTL, session rebuild — all owned by the caller).
pub trait CredentialProvider: Send + Sync {
    /// Return freshly-valid credentials, refreshing through whatever backing
    /// mechanism the implementor owns. Errors abort this attempt; the caller
    /// backs off and asks again.
    fn credentials(&self)
        -> impl std::future::Future<Output = Result<Credentials>> + Send;
}

/// Run one real-time connection forever, reconnecting with capped exponential
/// backoff.
///
/// - `creds` supplies fresh credentials before every connection attempt (so a
///   reconnection past the ~1h token lifetime re-mints the skypetoken + ic3).
/// - `endpoint` says which trouter this is and how to register there
///   ([`Endpoint::messaging`] or [`Endpoint::calling`]).
/// - `role` receives whatever the pushes decode into.
/// - `status` receives lifecycle transitions (Connecting/Connected/Disconnected).
/// - `epid` is the stable endpoint id; persist it across runs so the server keeps
///   routing to the same registration. The two connections use DIFFERENT ids: one
///   registration per worker, or the second would replace the first.
///
/// Returns only if the channels close (i.e. the UI is gone).
///
/// Every attempt is REPORTED, which it was not: `connect_once`'s result used to be dropped
/// on the floor, so a calling socket that never came up, or one that came up and dropped
/// every few minutes, left nothing in the journal to diagnose it from. Two shapes, because
/// the two kinds of ending need different amounts of noise:
///
/// - an attempt whose WEBSOCKET OPENED ends with one line naming how long it lived and what
///   it carried ([`connection_ended_line`]) — a real connection ending is always news;
/// - an attempt that failed BEFORE it opened (credentials, allocate, handshake, upgrade) is
///   logged only when its failure differs from the previous attempt's, so an outage is one
///   line rather than one every 30 s for as long as it lasts.
///
/// Both decisions, the backoff and the reason are [`after_attempt`]'s, which is pure.
pub async fn run(
    creds: impl CredentialProvider,
    epid: String,
    endpoint: Endpoint,
    role: Role,
    status: mpsc::UnboundedSender<Status>,
) {
    let http = match reqwest::Client::builder()
        .user_agent(UA)
        .http1_only()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    let label = role.label();
    let mut retry = RetryState::default();
    loop {
        let _ = status.send(Status::Connecting);
        // Fresh credentials for THIS attempt. If minting fails (broker down, etc.) treat it
        // like a disconnect and back off.
        //
        // Say WHY when it fails. This loop retries forever and the error used to be
        // dropped on the floor, so a broker that stopped minting tokens left the live
        // feed dead with an empty journal — while mail and the calendar both logged
        // their failures. A backend that runs for weeks is diagnosed from that
        // journal, so the one line that names the cause has to be in it.
        let outcome = match creds.credentials().await {
            Ok(Credentials { session, ic3 }) => {
                let mut report = ConnectionReport::default();
                let result = connect_once(
                    &http, &session, &ic3, &epid, &endpoint, &role, &status, &mut report,
                )
                .await;
                Attempt::Connected { report, result }
            }
            Err(e) => Attempt::NoCredentials(e),
        };
        // If the consumer is gone, stop.
        if role.is_closed() || status.is_closed() {
            return;
        }
        // Everything this loop DECIDES about the attempt is `after_attempt`, which is pure:
        // the sleep, the reset, the line and the reason. What is left here is carrying it
        // out — and the sleep taken is the one the status announces, by construction.
        let after = after_attempt(&mut retry, label, outcome, tokio::time::Instant::now());
        if let Some(line) = &after.line {
            eprintln!("{line}");
        }
        let _ = status.send(after.status());
        tokio::time::sleep(Duration::from_secs(after.retry_in_secs)).await;
    }
}

/// How one attempt of [`run`] went: it never had credentials, or it had them and tried.
#[derive(Debug)]
enum Attempt {
    NoCredentials(anyhow::Error),
    Connected { report: ConnectionReport, result: Result<()> },
}

/// What [`run`] carries from one attempt to the next.
#[derive(Debug, Default)]
struct RetryState {
    /// The sleep taken before the attempt that just ended; 0 before the first one.
    backoff_secs: u64,
    failures: RepeatFilter,
}

/// What [`run`] does once an attempt has ended: the journal line it writes, if any, and the
/// `Disconnected` it announces — whose `retry_in_secs` is the sleep it then takes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AfterAttempt {
    line: Option<String>,
    retry_in_secs: u64,
    no_credentials: bool,
}

impl AfterAttempt {
    fn status(&self) -> Status {
        Status::Disconnected {
            retry_in_secs: self.retry_in_secs,
            no_credentials: self.no_credentials,
        }
    }
}

/// Decide everything about an attempt that has just ended, at `now`.
///
/// Pure (the clock is handed in), and that is why it exists apart from [`run`]: the backoff
/// reset, the repeat filter and the reason a page is told are each a rule, and a rule that
/// lives inside an endless network loop is one no test can reach. Four decisions:
///
/// - **how long the websocket lived** — from the report's own moment it opened, so a
///   connection that never opened has no lifetime at all;
/// - **the sleep** — [`next_backoff`] over that lifetime;
/// - **the reason** — `no_credentials` is true when the provider failed for SIGN-IN, and
///   false when what failed was one of the provider's own requests getting no answer
///   ([`failed_on_the_network`]): a session rebuilt after its 50 minutes is an authz POST,
///   and a network that is down fails it exactly as it fails the allocate. Reading that as
///   sign-in told the reader to wait for a sign-in that had never broken;
/// - **the line** — a connection that OPENED always says how it ended; every failure before
///   that, sign-in included, is said once per distinct failure, so an outage is one line
///   rather than one per worker every 30 s for as long as it lasts. The first line of an
///   outage still names its cause — the broker, the network — which is the line a journal is
///   diagnosed from; and a connection that opened clears the memory, so the next outage's
///   first failure is news again.
fn after_attempt(
    retry: &mut RetryState,
    label: &str,
    outcome: Attempt,
    now: tokio::time::Instant,
) -> AfterAttempt {
    let lived = match &outcome {
        Attempt::Connected { report, .. } => {
            report.websocket_opened.map(|opened| now.saturating_duration_since(opened))
        }
        Attempt::NoCredentials(_) => None,
    };
    retry.backoff_secs = next_backoff(retry.backoff_secs, lived);
    let backoff = retry.backoff_secs;
    const REPEATS: &str = "(the same failure again is not logged)";
    let (line, no_credentials) = match outcome {
        Attempt::NoCredentials(e) if failed_on_the_network(&e) => {
            let text = format!("{e:#}");
            let line = retry.failures.is_news(format!("no session: {text}")).then(|| {
                format!(
                    "[{label}] could not get a session, retrying in {backoff}s — {text} \
                     {REPEATS}"
                )
            });
            (line, false)
        }
        Attempt::NoCredentials(e) => {
            let text = format!("{e:#}");
            let line = retry.failures.is_news(format!("no credentials: {text}")).then(|| {
                format!(
                    "[{label}] no credentials, retrying in {backoff}s — is the identity broker \
                     up? ({text}) {REPEATS}"
                )
            });
            (line, true)
        }
        Attempt::Connected { report, result } => match lived {
            Some(lived) => {
                retry.failures.forget();
                let line = connection_ended_line(label, lived, &report, result.as_ref().err());
                (Some(line), false)
            }
            None => {
                let line = result.err().and_then(|e| {
                    let text = format!("{e:#}");
                    retry.failures.is_news(text.clone()).then(|| {
                        format!(
                            "[{label}] could not connect, retrying in {backoff}s — {text} \
                             {REPEATS}"
                        )
                    })
                });
                (line, false)
            }
        },
    };
    AfterAttempt { line, retry_in_secs: backoff, no_credentials }
}

/// Whether a credentials failure was one of the provider's own REQUESTS getting no answer —
/// the network — rather than sign-in.
///
/// It is read off the error chain, because that is the one place the two are told apart
/// with no guessing: the identity broker is reached over D-Bus and never over HTTP, so a
/// `reqwest::Error` anywhere in the chain is a request this process made (the authz exchange
/// that turns a token into a session) that never came back as an answer it could read — the
/// network, or a service in no state to answer. Everything else is sign-in: the broker
/// refusing, not answering or not being there — which is also what the app's own sign-in
/// banner reports, so the call row and the banner tell one story — and an authz host that
/// ANSWERED with a refusal, which is the token it was handed being turned down.
fn failed_on_the_network(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<reqwest::Error>())
}

/// The previous attempt's failure, so a failure that merely repeats is not logged again.
///
/// A connection that cannot be made fails identically every 30 s for as long as the cause
/// lasts — an hour of no network was 120 identical lines — while the line worth having is
/// the first one, and the one that says the cause CHANGED. A connection that opened clears
/// it ([`RepeatFilter::forget`]), so the first failure of the NEXT outage is news again.
#[derive(Debug, Default)]
struct RepeatFilter {
    last: Option<String>,
}

impl RepeatFilter {
    /// Whether `failure` differs from the previous attempt's, remembering it either way.
    fn is_news(&mut self, failure: String) -> bool {
        let news = self.last.as_deref() != Some(failure.as_str());
        self.last = Some(failure);
        news
    }

    fn forget(&mut self) {
        self.last = None;
    }
}

/// One full connect → listen cycle. Returns when the socket closes or errors.
///
/// Whatever it learns on the way is written into `report` as it learns it, so the caller
/// can say how far the attempt got whichever step ended it.
#[allow(clippy::too_many_arguments)]
async fn connect_once(
    http: &reqwest::Client,
    sess: &Session,
    ic3: &str,
    epid: &str,
    endpoint: &Endpoint,
    role: &Role,
    status: &mpsc::UnboundedSender<Status>,
    report: &mut ConnectionReport,
) -> Result<()> {
    // Every request error below is stripped of its URL before it can reach the journal
    // (`reqwest::Error::without_url`): the handshake's query carries the allocation's own
    // `connectparams`, a signature among them, and this module logs no credential. It also
    // keeps the failure TEXT stable from one attempt to the next — those parameters change
    // per allocation — which is what lets `run` log a repeated failure once.

    // 1. trouter connect
    let begin_url = format!("{}?epid={}", endpoint.allocate, urlencoding::encode(epid));
    let r = http
        .post(&begin_url)
        .header("x-skypetoken", &sess.skypetoken)
        .header("content-length", "0")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("trouter allocate")?;
    let body = r
        .text()
        .await
        .map_err(reqwest::Error::without_url)
        .context("trouter allocate body")?;
    let info: Value = serde_json::from_str(&body).context("trouter begin body")?;
    report.allocate_ttl_secs = connection_ttl_secs(&info);
    let socketio = info.get("socketio").and_then(|v| v.as_str()).context("no socketio")?;
    let surl = info.get("surl").and_then(|v| v.as_str()).context("no surl")?.to_string();
    let connectparams = info.get("connectparams").cloned().unwrap_or(Value::Null);
    let ccid = info.get("ccid").and_then(|v| v.as_str());

    // 2. socket.io v1 handshake
    let q = socketio_query(&connectparams, epid, ccid);
    let hs = http
        .get(format!("{socketio}socket.io/1/?{q}"))
        .header("X-Skypetoken", &sess.skypetoken)
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("socket.io handshake")?;
    if !hs.status().is_success() {
        return Err(anyhow!("socket.io handshake -> {}", hs.status()));
    }
    let hs_body = hs
        .text()
        .await
        .map_err(reqwest::Error::without_url)
        .context("socket.io handshake body")?;
    let handshake = parse_handshake(&hs_body)?;
    report.heartbeat_secs = handshake.heartbeat_secs;
    report.close_secs = handshake.close_secs;
    let session_id = handshake.session_id;

    // 3. websocket connect
    let ws_url = format!("{socketio}socket.io/1/websocket/{session_id}?{q}")
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let mut req = ws_url.as_str().into_client_request().context("build ws request")?;
    req.headers_mut().insert("X-Skypetoken", sess.skypetoken.parse()?);
    req.headers_mut().insert("User-Agent", UA.parse()?);
    let (ws, _resp) = tokio::time::timeout(WEBSOCKET_TIMEOUT, tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| anyhow!("ws connect timed out after {}s", WEBSOCKET_TIMEOUT.as_secs()))?
        .context("ws connect")?;
    report.websocket_opened = Some(tokio::time::Instant::now());
    let (mut write, mut read) = ws.split();

    let mut count = 1u32;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            maybe = read.next() => {
                let Some(msg) = maybe else { return Ok(()); }; // stream ended -> reconnect
                let text = match msg.context("ws read")? {
                    WsMessage::Text(t) => t.to_string(),
                    WsMessage::Ping(p) => { write.send(WsMessage::Pong(p)).await.ok(); continue; }
                    WsMessage::Close(_) => return Ok(()),
                    _ => continue,
                };
                if text.is_empty() { continue; }
                report.frames.count(text.as_bytes()[0]);

                match text.as_bytes()[0] {
                    b'1' => {
                        // authenticate + activity + register
                        let auth_msg = json!({
                            "name": "user.authenticate",
                            "args": [{
                                "headers": {
                                    "X-Ms-Test-User": "False",
                                    "Authorization": format!("Bearer {ic3}"),
                                    "X-MS-Migration": "True"
                                },
                                "connectparams": connectparams.clone()
                            }]
                        });
                        write.send(WsMessage::Text(format!("5:::{auth_msg}"))).await?;

                        let act = json!({"name":"user.activity","args":[{"state":"active","cv":"teamslite000000000000.0.1"}]});
                        write.send(WsMessage::Text(format!("5:{count}+::{act}"))).await?;
                        count += 1;

                        let registrar =
                            register(http, &sess.skypetoken, ic3, &surl, epid, endpoint).await?;
                        eprintln!(
                            "[{}] registered: {registrar} (ttl {} s)",
                            role.label(),
                            endpoint.ttl_secs
                        );
                        // The surl is only ours once the registrar answered, so this is
                        // where a call may start building links on it. A non-2xx answer
                        // proceeds exactly as a 2xx does — it is logged and carried, never
                        // acted on, until a refusal has been measured (see
                        // `CallingChannel::registrar_status`).
                        role.on_registered(&surl, epid, registrar.as_u16());
                        let _ = status.send(Status::Connected);
                    }
                    b'3' => {
                        if let Some(payload) = after_third_colon(&text)
                            && let Ok(reqv) = serde_json::from_str::<Value>(payload) {
                                // ack EVERY request (the server drops us otherwise)
                                let id = reqv.get("id").cloned().unwrap_or(json!(0));
                                let ack = json!({"id": id, "status": 200, "body": ""});
                                write.send(WsMessage::Text(format!("3:::{ack}"))).await?;

                                if !role.deliver(&reqv) {
                                    return Ok(()); // consumer gone
                                }
                            }
                    }
                    _ => {}
                }
            }
            _ = ping.tick() => {
                if write.send(WsMessage::Text(format!("5:{count}+::{{\"name\":\"ping\"}}"))).await.is_err() {
                    return Ok(()); // write failed -> reconnect
                }
                count += 1;
            }
        }
    }
}

fn socketio_query(connectparams: &Value, epid: &str, ccid: Option<&str>) -> String {
    let mut q = String::from("v=v4&");
    if let Some(obj) = connectparams.as_object() {
        for (k, v) in obj {
            if let Some(val) = v.as_str() {
                q.push_str(&format!("{k}={}&", urlencoding::encode(val)));
            }
        }
    }
    let tc = format!("{{\"cv\":\"{TCCV}\",\"ua\":\"TeamsCDL\",\"hr\":\"\",\"v\":\"{CLIENT_VERSION}\"}}");
    q.push_str(&format!("tc={}&", urlencoding::encode(&tc)));
    q.push_str("con_num=1234567890123_1&");
    q.push_str(&format!("epid={}&", urlencoding::encode(epid)));
    if let Some(c) = ccid {
        q.push_str(&format!("ccid={}&", urlencoding::encode(c)));
    }
    q.push_str("auth=true&timeout=40&");
    q
}

fn after_third_colon(s: &str) -> Option<&str> {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if c == ':' {
            n += 1;
            if n == 3 {
                return Some(&s[i + 1..]);
            }
        }
    }
    None
}

/// Register this connection's surl with the registrar, and say what the registrar answered.
///
/// The status is RETURNED rather than judged here: it had never been read before, so
/// whether this tenant's registrar ever refuses is unmeasured, and the caller logs it and
/// carries it on. An error is a request that got no answer at all (bounded at
/// [`FETCH_TIMEOUT`], the client's own `fetchTimeoutMs`).
async fn register(
    http: &reqwest::Client,
    skypetoken: &str,
    ic3: &str,
    surl: &str,
    epid: &str,
    endpoint: &Endpoint,
) -> Result<reqwest::StatusCode> {
    let body = registration_body(surl, epid, endpoint);
    let response = http
        .post(&endpoint.registrar)
        .header("content-type", "application/json")
        .header("X-Skypetoken", skypetoken)
        .header("authorization", format!("Bearer {ic3}"))
        .body(body.to_string())
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("registrar")?;
    Ok(response.status())
}

/// Remove a registration: `DELETE {registrar}/{registrationId}`, the same call the
/// web client makes when it stops.
///
/// It matters most for the CALLING worker. A registration Teams still believes in
/// keeps routing the user's calls to a client that is no longer listening, and a call
/// offered to a device that never rings is a call they miss.
///
/// `bound` is the caller's: an ordinary stop takes the signaling `End` class's ten seconds, and a
/// backend that is going away takes less, inside the stop budget its supervisor grants it.
pub async fn unregister(
    http: &reqwest::Client,
    skypetoken: &str,
    ic3: &str,
    registrar: &str,
    registration_id: &str,
    bound: Duration,
) -> Result<()> {
    let url = format!(
        "{}/{}",
        registrar.trim_end_matches('/'),
        urlencoding::encode(registration_id)
    );
    let response = http
        .delete(&url)
        .header("X-Skypetoken", skypetoken)
        .header("authorization", format!("Bearer {ic3}"))
        .timeout(bound)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("unregister -> {}", response.status()));
    }
    Ok(())
}

/// The registration body, pure so both workers' shapes can be pinned by a test.
///
/// `path` is the BARE surl for either worker: the real web client appends no suffix
/// to it (`transports:{TROUTER:[{context, path: allocateResult.surl, ttl}]}`), and a
/// suffix here is what made this app look like a Windows endpoint.
fn registration_body(surl: &str, epid: &str, endpoint: &Endpoint) -> Value {
    json!({
        "clientDescription": {
            "appId": endpoint.app_id,
            "aesKey": "",
            "languageId": "en-US",
            "platform": "edge",
            "templateKey": endpoint.template_key,
            "platformUIVersion": CLIENT_VERSION
        },
        "registrationId": epid,
        "nodeId": "",
        "transports": { "TROUTER": [{ "context": "", "path": surl, "ttl": endpoint.ttl_secs }] }
    })
}

/// Load a persisted endpoint id from `path`, or generate + save a fresh one.
/// A stable epid lets the trouter keep routing to the same registration across runs.
pub fn load_or_create_epid(path: &std::path::Path) -> String {
    if let Ok(s) = std::fs::read_to_string(path) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let epid = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::write(path, &epid);
    epid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_third_colon_extracts_payload() {
        assert_eq!(after_third_colon("3:::{\"id\":1}"), Some("{\"id\":1}"));
        assert_eq!(after_third_colon("3:42+:/ep:{\"a\":1}"), Some("{\"a\":1}"));
        // "1::" has only two colons -> no third-colon payload
        assert_eq!(after_third_colon("1::"), None);
        // but "1:::" (the connect frame form) yields an empty payload
        assert_eq!(after_third_colon("1:::"), Some(""));
        assert_eq!(after_third_colon("no-colons"), None);
    }

    #[test]
    fn socketio_query_encodes_and_includes_epid() {
        let cp = json!({ "tenant": "af1b bf3d", "sr": "x/y" });
        let q = socketio_query(&cp, "epid-123", Some("cc id"));
        assert!(q.starts_with("v=v4&"));
        assert!(q.contains("tenant=af1b%20bf3d"));
        assert!(q.contains("sr=x%2Fy"));
        assert!(q.contains("epid=epid-123"));
        assert!(q.contains("ccid=cc%20id"));
        assert!(q.contains("auth=true"));
    }

    /// The registration is what tells Teams which client is running, and the
    /// calling one decides whether a call is routed here at all. Both shapes are
    /// pinned: the bare surl as the path, the web client's own template, and its
    /// own TTL.
    #[test]
    fn each_worker_registers_the_way_the_web_client_registers_it() {
        let messaging = registration_body("https://tr/v4/f/abc/", "epid-1", &Endpoint::messaging());
        assert_eq!(messaging["clientDescription"]["appId"], "TeamsCDLWebWorker");
        assert_eq!(messaging["clientDescription"]["templateKey"], "TeamsCDLWebWorker_2.1");
        assert_eq!(messaging["registrationId"], "epid-1");
        assert_eq!(messaging["transports"]["TROUTER"][0]["path"], "https://tr/v4/f/abc/");
        assert_eq!(messaging["transports"]["TROUTER"][0]["ttl"], 86400);

        let calling = Endpoint::calling("https://go-eu.trouter.teams.microsoft.com/v3/c", REGISTRAR);
        let body = registration_body("https://tr/v4/f/xyz/", "epid-2", &calling);
        assert_eq!(body["clientDescription"]["appId"], "SkypeSpacesWeb");
        assert_eq!(body["clientDescription"]["templateKey"], "SkypeSpacesWeb_2.6");
        assert_eq!(body["transports"]["TROUTER"][0]["ttl"], 3600);
        // The BARE surl: the desktop client's worker suffix is what made a call
        // routed to a Windows endpoint that does not exist here.
        let path = body["transports"]["TROUTER"][0]["path"].as_str().unwrap();
        assert_eq!(path, "https://tr/v4/f/xyz/");
        assert!(!path.contains("NGCallManager"), "the web client appends no worker suffix");
    }

    /// The desktop client's templates must not come back: Teams routes a call to
    /// the endpoints it believes are running, and claiming to be a Windows client
    /// sends the user's calls to a client that is not there.
    #[test]
    fn no_worker_ever_claims_to_be_the_desktop_client() {
        // Built rather than written, because this test's own source is part of what
        // it scans: a literal here would make the assertion find itself.
        let desktop = [
            format!("NGCallManager{}", "Win"),
            format!("NGCallManager{}", "Osx"),
            format!("Desktop{}", "Ngc"),
        ];
        let source = include_str!("trouter.rs");
        for spelling in desktop {
            assert!(
                !source.contains(&format!("\"{spelling}\"")),
                "{spelling} must never be a value this app sends"
            );
        }
        // And the values that ARE sent are the web client's own.
        assert_eq!(Endpoint::calling("https://t/v3/c", REGISTRAR).app_id, "SkypeSpacesWeb");
        assert_eq!(
            Endpoint::calling("https://t/v3/c", REGISTRAR).template_key,
            "SkypeSpacesWeb_2.6"
        );
    }

    #[test]
    fn a_directory_trouter_url_becomes_this_clients_allocate_url() {
        // The directory states the connect form; we speak allocate, same host.
        assert_eq!(
            allocate_url_for("https://go-eu.trouter.teams.microsoft.com/v3/c"),
            "https://go-eu.trouter.teams.microsoft.com/v4/a"
        );
        assert_eq!(
            allocate_url_for("wss://go-eu.trouter.teams.microsoft.com/v4/c"),
            "https://go-eu.trouter.teams.microsoft.com/v4/a"
        );
        // Already an allocate URL: left alone (trailing slash trimmed).
        assert_eq!(
            allocate_url_for("https://go.trouter.teams.microsoft.com/v4/a/"),
            "https://go.trouter.teams.microsoft.com/v4/a"
        );
        // Nonsense falls back to the host that is known to work rather than to a
        // malformed URL: a calling connection that cannot allocate is a feature
        // that silently does nothing.
        assert_eq!(allocate_url_for("not a url"), TROUTER_BEGIN);
    }

    /// A call cannot publish a link before it knows its own surl, so the calling
    /// role must announce it — and the messaging role must not (it has no links).
    #[tokio::test(flavor = "current_thread")]
    async fn the_calling_role_publishes_its_surl_once_registered() {
        let (frames_tx, _frames_rx) = mpsc::unbounded_channel();
        let (chan_tx, mut chan_rx) = mpsc::unbounded_channel();
        let calling = Role::Calling { frames: frames_tx, channel: chan_tx };
        calling.on_registered("https://tr/v4/f/abc/", "epid-2", 201);
        let published = chan_rx.recv().await.expect("the surl");
        assert_eq!(published.surl, "https://tr/v4/f/abc/");
        assert_eq!(published.endpoint_id, "epid-2");
        // The registrar's answer travels with the address, whatever it was.
        assert_eq!(published.registrar_status, 201);
        calling.on_registered("https://tr/v4/f/abc/", "epid-2", 403);
        let refused = chan_rx.recv().await.expect("a refused registration is still published");
        assert_eq!(refused.registrar_status, 403);

        let (ev_tx, _ev_rx) = mpsc::unbounded_channel();
        let (ty_tx, _ty_rx) = mpsc::unbounded_channel();
        let (rr_tx, _rr_rx) = mpsc::unbounded_channel();
        let (call_tx, _call_rx) = mpsc::unbounded_channel();
        Role::Messaging { events: ev_tx, typing: ty_tx, receipts: rr_tx, calls: call_tx }
            .on_registered("https://tr/v4/f/abc/", "epid-1", 201);
        // Nothing to assert but the absence of a panic: the messaging role has no
        // channel to publish on.
    }

    /// Everything on the calling socket is calling traffic, whatever URL it names —
    /// filtering it by URL is what dropped the web client's own frame shape.
    #[tokio::test(flavor = "current_thread")]
    async fn the_calling_role_forwards_every_push_it_gets() {
        let (frames_tx, mut frames_rx) = mpsc::unbounded_channel();
        let (chan_tx, _chan_rx) = mpsc::unbounded_channel();
        let role = Role::Calling { frames: frames_tx, channel: chan_tx };
        let request = json!({
            "url": "https://tr/v4/f/abc/callAgent/s/c/call/mediaAnswer/",
            "body": "{\"mediaAnswer\":{}}"
        });
        assert!(role.deliver(&request));
        let frame = frames_rx.recv().await.expect("a frame");
        assert!(frame.url.contains("callAgent"));
    }

    #[test]
    fn epid_persists_across_calls() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("teams-lite-epid-test-{}.txt", uuid::Uuid::new_v4()));
        let a = load_or_create_epid(&path);
        let b = load_or_create_epid(&path);
        assert_eq!(a, b, "epid must be stable once written");
        assert!(!a.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// The core of the token-refresh fix: `run` must ask the provider for fresh
    /// credentials on EVERY connection attempt, not once at startup. We make the
    /// provider fail every time (so no real network happens) and count its calls;
    /// after it has been asked twice — i.e. it was re-asked on the reconnect — we
    /// close the channels to end the loop and assert the re-invocation happened.
    #[tokio::test(flavor = "current_thread")]
    async fn run_asks_provider_for_fresh_credentials_each_attempt() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingProvider {
            calls: Arc<AtomicUsize>,
        }
        impl CredentialProvider for CountingProvider {
            async fn credentials(&self) -> Result<Credentials> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                // Never hand back real creds: fail so `run` treats it as a failed
                // attempt and loops into its backoff → reconnect path.
                Err(anyhow!("test: no credentials"))
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { calls: calls.clone() };
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<Vec<Message>>();
        let (ty_tx, _ty_rx) = mpsc::unbounded_channel::<crate::trouter_events::TypingEvent>();
        let (rr_tx, _rr_rx) = mpsc::unbounded_channel::<crate::trouter_events::ReadReceiptEvent>();
        let (call_tx, _call_rx) = mpsc::unbounded_channel::<crate::trouter_events::CallFrame>();
        let (st_tx, mut st_rx) = mpsc::unbounded_channel::<Status>();
        let role = Role::Messaging {
            events: ev_tx,
            typing: ty_tx,
            receipts: rr_tx,
            calls: call_tx,
        };

        let handle = tokio::spawn(async move {
            run(provider, "epid-test".to_string(), Endpoint::messaging(), role, st_tx).await;
        });

        // Wait until the provider has been asked at least twice (proves it was
        // re-invoked on the reconnect, not reused from a first attempt). The
        // backoff after the first failure is 1s, so allow a little headroom.
        let mut saw_reconnect = false;
        for _ in 0..40 {
            if calls.load(Ordering::SeqCst) >= 2 {
                saw_reconnect = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_reconnect, "provider must be asked again on reconnect");

        // Close the consumer side so `run` observes it and returns.
        drop(ev_rx);
        st_rx.close();
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

    /// Drive `run` until its first `Disconnected`, and hand it back. The provider is asked
    /// once before that; the consumer side is closed afterwards so `run` returns.
    async fn first_disconnect(
        provider: impl CredentialProvider + 'static,
        endpoint: Endpoint,
    ) -> Status {
        let (frames_tx, frames_rx) = mpsc::unbounded_channel();
        let (chan_tx, _chan_rx) = mpsc::unbounded_channel();
        let (st_tx, mut st_rx) = mpsc::unbounded_channel::<Status>();
        let role = Role::Calling { frames: frames_tx, channel: chan_tx };
        let handle = tokio::spawn(async move {
            run(provider, "epid-test".to_string(), endpoint, role, st_tx).await;
        });
        let disconnect = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                match st_rx.recv().await.expect("run keeps the status channel open") {
                    status @ Status::Disconnected { .. } => return status,
                    _ => continue,
                }
            }
        })
        .await
        .expect("an attempt that cannot connect ends quickly");
        drop(frames_rx);
        st_rx.close();
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        disconnect
    }

    /// `no_credentials` is what lets the calling plane say "waiting for sign-in" rather than
    /// "connecting", so both branches of the credentials match are driven: a provider that
    /// fails, and one that hands back credentials for an endpoint nobody listens on. The
    /// loopback port 1 refuses the connection at once, so the SUCCESS branch — the whole of
    /// `connect_once`'s allocate step — runs with no network at all.
    #[tokio::test(flavor = "current_thread")]
    async fn a_disconnect_says_whether_the_attempt_had_credentials() {
        struct Failing;
        impl CredentialProvider for Failing {
            async fn credentials(&self) -> Result<Credentials> {
                Err(anyhow!("test: the broker is down"))
            }
        }
        struct Loopback {
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        impl CredentialProvider for Loopback {
            async fn credentials(&self) -> Result<Credentials> {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Credentials {
                    session: Session {
                        skypetoken: "test-skypetoken".into(),
                        region: "test".into(),
                        gtms: json!({}),
                        self_name: String::new(),
                        self_mri: String::new(),
                    },
                    ic3: "test-ic3".into(),
                })
            }
        }
        // An endpoint whose allocate host nothing listens on.
        let unroutable =
            Endpoint { allocate: "http://127.0.0.1:1/v4/a".into(), ..Endpoint::messaging() };

        let without = first_disconnect(Failing, unroutable.clone()).await;
        assert_eq!(without, Status::Disconnected { retry_in_secs: 1, no_credentials: true });

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let with = first_disconnect(Loopback { calls: calls.clone() }, unroutable).await;
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 1, "the provider was asked");
        assert_eq!(
            with,
            Status::Disconnected { retry_in_secs: 1, no_credentials: false },
            "credentials were had: the attempt failed at the network, not at sign-in"
        );

        // And a provider that failed because ITS OWN request got no answer — the authz
        // exchange a stale session is rebuilt with — failed at the network too, not at
        // sign-in: the reader must not be told to wait for a sign-in that never broke.
        struct NoNetwork;
        impl CredentialProvider for NoNetwork {
            async fn credentials(&self) -> Result<Credentials> {
                Err(anyhow::Error::from(request_error()).context("test: the authz exchange"))
            }
        }
        let offline = first_disconnect(NoNetwork, Endpoint::messaging()).await;
        assert_eq!(offline, Status::Disconnected { retry_in_secs: 1, no_credentials: false });
    }

    /// A `reqwest::Error` made with no network at all: a URL that does not parse is refused
    /// when the request is built.
    fn request_error() -> reqwest::Error {
        reqwest::Client::new().get("not a url").build().expect_err("an invalid URL is refused")
    }

    /// Everything `run` DECIDES about an attempt that ended, rule by rule, through the one
    /// pure function that decides it: the sleep (doubling in a row, back to the floor after a
    /// connection that lived), the line (a repeated failure said once, a connection's own end
    /// always said, and that end clearing the memory), and the reason a page is told
    /// (sign-in, or not). Each assertion is the one a broken rule fails.
    #[test]
    fn after_an_attempt_the_sleep_the_line_and_the_reason_are_decided_together() {
        let mut retry = RetryState::default();
        let t0 = tokio::time::Instant::now();
        let secs = Duration::from_secs;

        // Sign-in: the first failure names the broker and reads as no credentials…
        let broker = || Attempt::NoCredentials(anyhow!("acquire skype token: no answer"));
        let first = after_attempt(&mut retry, "calling", broker(), t0);
        assert_eq!((first.retry_in_secs, first.no_credentials), (1, true));
        assert_eq!(
            first.line.as_deref(),
            Some(
                "[calling] no credentials, retrying in 1s — is the identity broker up? (acquire \
                 skype token: no answer) (the same failure again is not logged)"
            )
        );
        // …and the same failure again says nothing, while the sleep doubles.
        assert_eq!(
            after_attempt(&mut retry, "calling", broker(), t0),
            AfterAttempt { line: None, retry_in_secs: 2, no_credentials: true }
        );

        // The provider's own REQUEST got no answer: the network, so not sign-in — news, once.
        let network =
            || Attempt::NoCredentials(anyhow::Error::from(request_error()).context("authz"));
        let offline = after_attempt(&mut retry, "calling", network(), t0);
        assert_eq!((offline.retry_in_secs, offline.no_credentials), (4, false));
        let line = offline.line.expect("a new failure is said");
        assert!(
            line.starts_with("[calling] could not get a session, retrying in 4s — authz: "),
            "{line}"
        );
        assert_eq!(
            after_attempt(&mut retry, "calling", network(), t0),
            AfterAttempt { line: None, retry_in_secs: 8, no_credentials: false }
        );

        // A failure before the websocket opened: said once, then silent at the ceiling.
        let refused = || Attempt::Connected {
            report: ConnectionReport::default(),
            result: Err(anyhow!("connection refused").context("trouter allocate")),
        };
        assert_eq!(
            after_attempt(&mut retry, "calling", refused(), t0).line.as_deref(),
            Some(
                "[calling] could not connect, retrying in 16s — trouter allocate: connection \
                 refused (the same failure again is not logged)"
            )
        );
        assert_eq!(
            after_attempt(&mut retry, "calling", refused(), t0),
            AfterAttempt { line: None, retry_in_secs: 30, no_credentials: false }
        );

        // A websocket that opened and died YOUNG is still a failure in a row — the backoff holds
        // at the ceiling — but its end is always said, and it clears the memory…
        let opened_at_t0 = ConnectionReport { websocket_opened: Some(t0), ..Default::default() };
        let young = after_attempt(
            &mut retry,
            "calling",
            Attempt::Connected { report: opened_at_t0.clone(), result: Err(anyhow!("reset")) },
            t0 + secs(5),
        );
        assert_eq!(young.retry_in_secs, 30);
        let line = young.line.expect("a connection's end is always said");
        assert!(line.starts_with("[calling] connection ended after 5s ("), "{line}");
        assert!(line.ends_with(" — reset"), "{line}");
        // …so the very failure that was silent a moment ago is news again.
        assert!(after_attempt(&mut retry, "calling", refused(), t0).line.is_some());

        // A connection that LIVED puts the next attempt back at the floor, wherever the backoff
        // had got to, and its line says how long it lived.
        let lived = after_attempt(
            &mut retry,
            "realtime",
            Attempt::Connected { report: opened_at_t0, result: Ok(()) },
            t0 + secs(3 * 86_400),
        );
        assert_eq!((lived.retry_in_secs, lived.no_credentials), (1, false));
        let line = lived.line.expect("a connection's end is always said");
        assert!(line.starts_with("[realtime] connection ended after 259200s ("), "{line}");
        assert!(line.ends_with(')'), "a connection that ended cleanly names no error: {line}");
    }

    /// `run` SLEEPS what it announces: the sequence an outage takes (1, 2, 4, 8, 16, then 30 s)
    /// is the time that really passes between two attempts, measured on a paused clock — so a
    /// status that says one number while the loop waits another cannot pass.
    #[tokio::test(start_paused = true)]
    async fn run_sleeps_the_backoff_it_announces() {
        struct Failing;
        impl CredentialProvider for Failing {
            async fn credentials(&self) -> Result<Credentials> {
                Err(anyhow!("test: the broker is down"))
            }
        }
        let (frames_tx, _frames_rx) = mpsc::unbounded_channel();
        let (chan_tx, _chan_rx) = mpsc::unbounded_channel();
        let (st_tx, mut st_rx) = mpsc::unbounded_channel::<Status>();
        let role = Role::Calling { frames: frames_tx, channel: chan_tx };
        let handle = tokio::spawn(async move {
            run(Failing, "epid-test".to_string(), Endpoint::messaging(), role, st_tx).await;
        });

        let mut attempts_at = Vec::new();
        let mut retries = Vec::new();
        while retries.len() < 8 {
            match st_rx.recv().await.expect("run keeps the status channel open") {
                Status::Connecting => attempts_at.push(tokio::time::Instant::now()),
                Status::Disconnected { retry_in_secs, no_credentials } => {
                    assert!(no_credentials, "the provider failed for sign-in");
                    retries.push(retry_in_secs);
                }
                Status::Connected => panic!("nothing can connect here"),
            }
        }
        assert_eq!(retries, [1, 2, 4, 8, 16, 30, 30, 30]);
        let waited: Vec<u64> =
            attempts_at.windows(2).map(|pair| (pair[1] - pair[0]).as_secs()).collect();
        assert_eq!(waited, retries[..attempts_at.len() - 1], "the sleep is the one announced");
        handle.abort();
    }

    /// The backoff resets after a GOOD connection and doubles after a bad one, and the first
    /// sleep of all is the floor — so the sequence of an outage is the one this loop always
    /// had (1, 2, 4, 8, 16, 30, 30…), and a socket that lived for days is back within a second.
    #[test]
    fn the_backoff_resets_after_a_connection_that_lived() {
        let secs = Duration::from_secs;
        let table: &[(u64, Option<Duration>, u64)] = &[
            // The first attempt, which never opened a websocket.
            (0, None, 1),
            // An outage: doubling to the ceiling, and holding there.
            (1, None, 2),
            (2, None, 4),
            (8, None, 16),
            (16, None, 30),
            (30, None, 30),
            // A websocket that opened but died young is still a failure in a row.
            (4, Some(secs(0)), 8),
            (30, Some(secs(59)), 30),
            // A connection that lived reset it, wherever the backoff had got to.
            (30, Some(secs(60)), 1),
            (8, Some(secs(60 * 60 * 24 * 3)), 1),
            (0, Some(secs(60)), 1),
            // Arithmetic that cannot overflow on a nonsense input.
            (u64::MAX, None, 30),
        ];
        for &(prev, lived, expected) in table {
            assert_eq!(next_backoff(prev, lived), expected, "prev {prev}, lived {lived:?}");
        }
    }

    /// The rule above, carried out by `run` itself against a trouter of this test's own on
    /// loopback — an allocate, a socket.io handshake, a websocket and a registrar, which is the
    /// whole of one real attempt.
    ///
    /// A pure table cannot see the half that DECIDES whether a connection lived: the moment
    /// `connect_once` stamps once the websocket upgrade succeeds. Without that stamp every
    /// connection reads as one that never opened, so a socket that dropped after a week
    /// reconnects as late as the thirtieth failure of an outage, and its end is never said —
    /// with every pure test green. The service here also REFUSES the registration (403), which
    /// `run` must carry to the address it publishes rather than judge.
    ///
    /// The first attempt fails at the allocate, so the backoff has something to reset FROM; the
    /// second connects, and the clock is paused and moved past [`LONG_LIVED`] before the service
    /// closes the socket. After the pause nothing here waits on a short timer: a paused clock
    /// jumps to the next timer whenever the runtime waits on a socket, so a short bound would
    /// fire on its own.
    #[tokio::test(flavor = "current_thread")]
    async fn a_websocket_that_lived_resets_the_backoff_through_run() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
        let port = listener.local_addr().expect("its address").port();
        let close = std::sync::Arc::new(tokio::sync::Notify::new());
        let allocations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let service = {
            let close = close.clone();
            let allocations = allocations.clone();
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let close = close.clone();
                    let allocations = allocations.clone();
                    tokio::spawn(async move {
                        // The request line decides what this connection is. It is PEEKED, so an
                        // upgrade reaches the websocket handshake with its request intact.
                        let mut line = [0u8; 64];
                        let mut seen = 0;
                        while !line[..seen].contains(&b'\n') && seen < line.len() {
                            seen = stream.peek(&mut line).await.unwrap_or(0);
                            if seen == 0 {
                                return;
                            }
                            tokio::task::yield_now().await;
                        }
                        if line[..seen].starts_with(b"GET /socket.io/1/websocket/") {
                            let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else {
                                return;
                            };
                            let _ = ws.send(WsMessage::Text("1::".into())).await;
                            close.notified().await;
                            let _ = ws.send(WsMessage::Close(None)).await;
                            return;
                        }
                        // Any other request: read its head and its body, then answer it.
                        let mut request = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let head_end = loop {
                            if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                                break at + 4;
                            }
                            match stream.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => request.extend_from_slice(&chunk[..n]),
                            }
                        };
                        let head = String::from_utf8_lossy(&request[..head_end]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        while request.len() < head_end + length {
                            match stream.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => request.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let (status, body) = if head.starts_with("post /v4/a") {
                            if allocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                                ("200 OK", "this is not an allocation".to_string())
                            } else {
                                let allocation = json!({
                                    "socketio": format!("http://127.0.0.1:{port}/"),
                                    "surl": "https://tr.test/v4/f/abc/",
                                    "connectparams": {},
                                    "ttl": "3600",
                                });
                                ("200 OK", allocation.to_string())
                            }
                        } else if head.starts_with("get /socket.io/1/") {
                            ("200 OK", "sid-test:40:60:websocket".to_string())
                        } else if head.starts_with("post /registrar") {
                            ("403 Forbidden", String::new())
                        } else {
                            ("404 Not Found", String::new())
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    });
                }
            })
        };

        struct Granted;
        impl CredentialProvider for Granted {
            async fn credentials(&self) -> Result<Credentials> {
                Ok(Credentials {
                    session: Session {
                        skypetoken: "test-skypetoken".into(),
                        region: "test".into(),
                        gtms: json!({}),
                        self_name: String::new(),
                        self_mri: String::new(),
                    },
                    ic3: "test-ic3".into(),
                })
            }
        }
        let endpoint = Endpoint {
            allocate: format!("http://127.0.0.1:{port}/v4/a"),
            registrar: format!("http://127.0.0.1:{port}/registrar"),
            ..Endpoint::messaging()
        };
        let (frames_tx, _frames_rx) = mpsc::unbounded_channel();
        let (chan_tx, mut chan_rx) = mpsc::unbounded_channel();
        let (st_tx, mut st_rx) = mpsc::unbounded_channel::<Status>();
        let role = Role::Calling { frames: frames_tx, channel: chan_tx };
        let client = tokio::spawn(run(Granted, "epid-test".to_string(), endpoint, role, st_tx));
        async fn next_disconnect(st_rx: &mut mpsc::UnboundedReceiver<Status>) -> Status {
            loop {
                match st_rx.recv().await.expect("run keeps the status channel open") {
                    status @ Status::Disconnected { .. } => return status,
                    _ => continue,
                }
            }
        }
        let real_bound = Duration::from_secs(20);

        // 1. The allocate answered nothing usable: a failure before any websocket, at the floor.
        let failed = tokio::time::timeout(real_bound, next_disconnect(&mut st_rx))
            .await
            .expect("the first attempt ends quickly");
        assert_eq!(failed, Status::Disconnected { retry_in_secs: 1, no_credentials: false });

        // 2. The next attempt connects: the registrar's refusal travels with the address, and the
        //    socket says it is up.
        let published = tokio::time::timeout(real_bound, chan_rx.recv())
            .await
            .expect("the second attempt registers")
            .expect("the address is published");
        assert_eq!(published.surl, "https://tr.test/v4/f/abc/");
        assert_eq!(published.registrar_status, 403, "a refusal is carried, never judged");
        tokio::time::timeout(real_bound, async {
            loop {
                match st_rx.recv().await.expect("run keeps the status channel open") {
                    Status::Connected => return,
                    Status::Connecting => continue,
                    other => panic!("the connected attempt reported {other:?}"),
                }
            }
        })
        .await
        .expect("the attempt reports itself connected");

        // 3. It lives past LONG_LIVED, and then the service closes it: a GOOD connection, so the
        //    next attempt waits the floor again rather than double the last failure's 1 s.
        tokio::time::pause();
        tokio::time::advance(LONG_LIVED + Duration::from_secs(1)).await;
        close.notify_one();
        let after = tokio::time::timeout(Duration::from_secs(86_400), next_disconnect(&mut st_rx))
            .await
            .expect("the closed connection is reported");
        assert_eq!(
            after,
            Status::Disconnected { retry_in_secs: 1, no_credentials: false },
            "a connection whose websocket lived resets the backoff"
        );
        client.abort();
        service.abort();
    }

    /// The allocate answer's `ttl` arrives as a number or a numeric string, and the client
    /// reads both; anything else is unknown rather than zero.
    #[test]
    fn the_allocate_ttl_is_read_as_a_number_or_a_numeric_string() {
        assert_eq!(connection_ttl_secs(&json!({ "ttl": 3600 })), Some(3600));
        assert_eq!(connection_ttl_secs(&json!({ "ttl": "86400" })), Some(86400));
        assert_eq!(connection_ttl_secs(&json!({ "ttl": " 120 " })), Some(120));
        assert_eq!(connection_ttl_secs(&json!({ "ttl": "soon" })), None);
        assert_eq!(connection_ttl_secs(&json!({ "ttl": -5 })), None);
        assert_eq!(connection_ttl_secs(&json!({ "ttl": null })), None);
        assert_eq!(connection_ttl_secs(&json!({ "surl": "https://tr/" })), None);
    }

    /// socket.io v1 answers `sid:heartbeat:close:transports`. The id is required; the two
    /// timeouts are measured, so a body without them must still connect.
    #[test]
    fn the_handshake_keeps_its_heartbeat_and_close_timeouts() {
        let full = parse_handshake("abc123:40:60:websocket,xhr-polling").expect("a handshake");
        assert_eq!(
            full,
            Handshake { session_id: "abc123".into(), heartbeat_secs: Some(40), close_secs: Some(60) }
        );
        let bare = parse_handshake("abc123").expect("only the id is required");
        assert_eq!((bare.session_id.as_str(), bare.heartbeat_secs, bare.close_secs), ("abc123", None, None));
        let blank = parse_handshake("abc123::x:websocket").expect("unreadable numbers are unknown");
        assert_eq!(blank.heartbeat_secs, None);
        assert_eq!(blank.close_secs, None);
        // An empty id is the error it always was: there is nothing to connect to.
        assert!(parse_handshake(":40:60:websocket").is_err());
        assert!(parse_handshake("").is_err());
    }

    /// Frames are counted by their socket.io type, and the line a connection leaves behind
    /// states each number — `?` where the service stated none, never a made-up zero.
    #[test]
    fn a_connection_that_ended_says_how_long_it_lived_and_what_it_carried() {
        let mut frames = FrameTally::default();
        for frame in b"1222333336x" {
            frames.count(*frame);
        }
        assert_eq!(frames, FrameTally { connect: 1, heartbeat: 3, message: 5, ack: 1, other: 1 });

        let report = ConnectionReport {
            allocate_ttl_secs: Some(3600),
            heartbeat_secs: Some(40),
            close_secs: None,
            websocket_opened: None,
            frames,
        };
        assert_eq!(
            connection_ended_line("calling", Duration::from_millis(125_900), &report, None),
            "[calling] connection ended after 125s (allocate ttl 3600s; heartbeat 40s close ?s; \
             frames 1:1 2:3 3:5 6:1 other:1)"
        );
        let error = anyhow!("reset by peer").context("ws read");
        let nothing_known = ConnectionReport::default();
        assert_eq!(
            connection_ended_line("realtime", Duration::from_secs(7), &nothing_known, Some(&error)),
            "[realtime] connection ended after 7s (allocate ttl ?s; heartbeat ?s close ?s; \
             frames 1:0 2:0 3:0 6:0 other:0) — ws read: reset by peer"
        );
    }

    /// The same failure twice is one line; a different one is news; and a connection that
    /// opened in between makes the next failure news even when its text is the same.
    #[test]
    fn a_repeated_failure_is_logged_once() {
        let mut failures = RepeatFilter::default();
        assert!(failures.is_news("trouter allocate: connection refused".into()));
        assert!(!failures.is_news("trouter allocate: connection refused".into()));
        assert!(failures.is_news("socket.io handshake -> 503 Service Unavailable".into()));
        assert!(failures.is_news("trouter allocate: connection refused".into()));
        failures.forget();
        assert!(failures.is_news("trouter allocate: connection refused".into()));
    }

    /// The bounds are the web client's own numbers, and every request of an attempt carries
    /// one: an unbounded step is what stopped the reconnect loop for good.
    #[test]
    fn every_step_of_an_attempt_is_bounded_at_the_clients_own_numbers() {
        assert_eq!(FETCH_TIMEOUT, Duration::from_secs(10), "fetchTimeoutMs:1e4");
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(WEBSOCKET_TIMEOUT, Duration::from_secs(30), "connectionTimeoutMs:3e4");

        let source = include_str!("trouter.rs");
        let between = |from: &str, to: &str| -> &str {
            let start = source.find(from).unwrap_or_else(|| panic!("{from} is in the source"));
            let end = start + source[start..].find(to).unwrap_or_else(|| panic!("{to} follows {from}"));
            &source[start..end]
        };
        // Built rather than written, so these needles do not find themselves here.
        let per_request = format!(".timeout({})", "FETCH_TIMEOUT");
        let attempt = between("async fn connect_once(", "fn socketio_query(");
        assert_eq!(attempt.matches(&per_request).count(), 2, "the allocate and the handshake");
        let upgrade =
            format!("tokio::time::timeout({}, tokio_tungstenite::connect_async", "WEBSOCKET_TIMEOUT");
        assert!(attempt.contains(&upgrade), "the websocket upgrade is bounded");
        let registration = between("async fn register(", "pub async fn unregister(");
        assert_eq!(registration.matches(&per_request).count(), 1, "the registrar POST");
        let client = between("pub async fn run(", "let label = role.label();");
        assert!(client.contains(&format!(".connect_timeout({})", "CONNECT_TIMEOUT")));
    }
}
