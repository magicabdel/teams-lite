// Native audio calling — the signaling plane, mimicking the Teams WEB client.
//
// NATIVE-CALLING.md is the protocol map this module implements; read it first. The
// short version:
//
//   * A call is a set of LINKS. We publish ours on our calling trouter socket
//     (`{surl}callAgent/{sessionId}/{causeId}{path}`) and the service publishes its
//     own in every frame it sends us. After setup neither side needs a fixed API:
//     each answer names the URLs for the next step.
//   * Placing a call is ONE POST to `calling_conversationServiceUrl`, read from the
//     authz directory this app already fetches (see `endpoints`).
//   * The media description is a stock WebRTC SDP wearing the label
//     `application/sdp-ngc-1.0`. This module never looks inside it: the browser
//     makes it and the browser consumes it (see `web/src/lib/call-media.ts`).
//
// This module is the network half only. It holds no state and starts nothing: the
// backend owns the live call (see `CallSession` in `src/bin/server.rs`), which is
// what keeps "who may place a call" one decision in one place.
//
// NOTHING here rings anybody on its own. Every function is called from an RPC the
// user's own click reached, and each of those is an `OUTWARD_METHODS` entry.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::teams::Session;

/// The content type the web client labels its SDP with. It is a LABEL on an
/// ordinary WebRTC offer/answer, not a dialect: the client's own stack produces the
/// blob with `RTCPeerConnection.createOffer` and reads it back with
/// `setRemoteDescription`. `…-0.5` is the older spelling the service still accepts.
pub const SDP_CONTENT_TYPE: &str = "application/sdp-ngc-1.0";

/// The capability masks a working client sent, and this app repeats.
///
/// Not computed: the conversation service refuses a request with `400` and an empty body
/// and never names the field it disliked, so a value nobody has seen accepted is a guess
/// with no feedback. These two came off a real request from this tenant (see
/// NATIVE-CALLING.md § 2.3a). Revisit them together with the join payload, never alone.
const CAPTURED_ENDPOINT_CAPABILITIES: u32 = 73463;
const CAPTURED_CLIENT_CAPABILITIES: u32 = 63928042;

/// The four modalities the service names (`I.MEDIA_TYPES` in the client's own bundle).
///
/// The capitals are the client's; its comparison lowercases both sides, which is why the
/// `"audio"` this app has always sent is accepted. Sending a screen and WATCHING one are two
/// modalities, not one, and that split is why they cost so differently (NATIVE-CALLING.md
/// § 10.5).
pub const MODALITY_AUDIO: &str = "audio";
/// A camera, sent or received.
pub const MODALITY_VIDEO: &str = "Video";
/// Sending a screen. Nothing in this app declares it yet.
pub const MODALITY_SCREEN_SHARER: &str = "ScreenSharer";
/// WATCHING somebody else's screen — the modality a viewer declares, which publishes
/// nothing about the user.
pub const MODALITY_SCREEN_VIEWER: &str = "ScreenViewer";

/// The media labels the service reads, one per section (`MEDIA_LABEL` in the bundle).
///
/// The label is how a shared screen is told from a camera: both are `m=video` sections, and
/// only this says which. Measured on the wire in both directions (NATIVE-CALLING.md § 10.2).
pub mod labels {
    pub const AUDIO: &str = "main-audio";
    pub const VIDEO: &str = "main-video";
    pub const SHARING: &str = "applicationsharing-video";
    pub const DATA: &str = "data";
}

/// The trouter path segment every callback link is built under (`URL_BASE.CALLAGENT`
/// in the web client's own calling bundle).
const CALL_AGENT: &str = "callAgent";

/// Who this client says it is, in the format the CALLING service takes.
///
/// Not the Skype-era `os=…; clientName=…` shape the messaging services want: the captured
/// calling request sends `SkypeSpaces/{build}/{platform}/TsCallingVersion=…`, and this
/// mirrors it. `TsCallingVersion` is the version of the calling stack the real client
/// runs, which is what the service reads to decide what a client understands.
const CLIENT_IDENTITY: &str = "SkypeSpaces/1415/26061118216/os=linux; osVer=undefined; \
     deviceType=computer; browser=chrome; browserVer=150.0.0.0/TsCallingVersion=2026.24.01.6";

/// The partition a region's users live in, as the captured request states it (`fr` →
/// `fr01`).
///
/// Derived from ONE observation, so it is a function rather than a constant: if the
/// service ever refuses it, this is the one line to correct, and the region it is built
/// from comes from the user's own authz answer.
fn teams_partition(region: &str) -> String {
    format!("{region}01")
}

/// Trailing paths of the callback links we publish. The service POSTs to these on
/// our trouter socket, so the set we publish IS the set of frames we can be sent.
/// Names match the web client's `CALLBACK_PATHS` one for one.
pub mod paths {
    pub const CALL_ACCEPTANCE: &str = "/call/acceptance/";
    pub const CALL_PROGRESS: &str = "/call/progress/";
    pub const CALL_END: &str = "/call/end/";
    pub const CALL_MEDIA_ANSWER: &str = "/call/mediaAnswer/";
    pub const CALL_MEDIA_ACKNOWLEDGEMENT: &str = "/call/mediaAcknowledgement/";
    pub const CALL_MEDIA_RENEGOTIATION: &str = "/call/mediaRenegotiation/";
    /// Where the service refuses media we offered. `/call/rejection/` in the client's own
    /// table — the same path a refused CALL arrives on, which is why it is spelled once.
    pub const CALL_MEDIA_REJECTION: &str = "/call/rejection/";
    pub const CALL_REDIRECTION: &str = "/call/redirection/";
    pub const CALL_TRANSFER: &str = "/call/transfer/";
    pub const CALL_REPLACEMENT: &str = "/call/replacement/";
    // The four the acknowledgement of an acceptance publishes beside the three above.
    // This app acts on none of them — it neither transfers a call nor sends video — but
    // the client sends all seven, and a link we do not publish is a frame the service
    // has nowhere to deliver.
    pub const CALL_BALANCE_UPDATE: &str = "/call/balanceUpdate/";
    pub const CALL_RETARGET_COMPLETION: &str = "/call/retargetCompletion/";
    pub const CALL_CONTROL_VIDEO_STREAMING: &str = "/call/controlVideoStreaming/";
    pub const CALL_UPDATE_MEDIA_DESCRIPTIONS: &str = "/call/updateMediaDescriptions/";
    pub const CONVERSATION_END: &str = "/conversation/conversationEnd/";
    pub const CONVERSATION_UPDATE: &str = "/conversation/conversationUpdate/";
    pub const CONVERSATION_ROSTER_UPDATE: &str = "/conversation/rosterUpdate/";
    pub const CONVERSATION_LOCAL_PARTICIPANT_UPDATE: &str =
        "/conversation/localParticipantUpdate/";
    pub const CONVERSATION_ADD_PARTICIPANT_SUCCESS: &str =
        "/conversation/addParticipantSuccess/";
    pub const CONVERSATION_ADD_PARTICIPANT_FAILURE: &str =
        "/conversation/addParticipantFailure/";
    pub const CONVERSATION_ADD_MODALITY_SUCCESS: &str = "/conversation/addModalitySuccess/";
    pub const CONVERSATION_ADD_MODALITY_FAILURE: &str = "/conversation/addModalityFailure/";
    pub const CONVERSATION_CONFIRM_UNMUTE: &str = "/conversation/confirmUnmute/";
    pub const CONVERSATION_RECEIVE_MESSAGE: &str = "/conversation/receiveMessage/";
    /// A media offer the service makes on its own, when a modality is not already
    /// negotiated. Measured: it POSTs `mediaRenegotiation` unprompted, and its offer holds
    /// the section for a shared screen (NATIVE-CALLING.md § 10.3a) — so a link we do not
    /// publish is the shared screen we never see.
    pub const CALL_NEW_MEDIA_OFFER: &str = "/call/newMediaOffer/";
    /// Who is talking, and the contributing sources behind a mixed stream. Neither is acted
    /// on yet; both are published so the service has somewhere to put them.
    pub const CALL_DOMINANT_SPEAKER_INFO: &str = "/call/dominantSpeakerInfo/";
    pub const CALL_CSRC_INFO: &str = "/call/csrcInfo/";
    /// A content-sharing session starting and ending, which is how a meeting says whose
    /// screen it is looking at.
    pub const CONVERSATION_CONTENT_SHARING_UPDATE: &str = "/conversation/contentSharingUpdate/";
    pub const CONVERSATION_CONTENT_SHARING_END: &str = "/conversation/contentSharingEnd/";
}

/// The calling plane's endpoints, read from the authz directory (`regionGtms`) this
/// app already fetches for `chatService`.
///
/// Every key is the one the real web client reads
/// (`serviceUrls.calling_conversationServiceUrl` and friends), which is what makes
/// this a lookup rather than a guess — see NATIVE-CALLING.md § 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// Where a call is created: one POST with the whole invitation.
    pub conversation_service: String,
    /// The trouter the calling endpoint registers on — a different regional host
    /// from the messaging one, and a connection of its own.
    pub trouter: String,
    /// The registrar the calling endpoint registers with (same one messaging uses).
    pub registrar: String,
    /// The media transport host, as `udp://host:port`. Its host is a STUN/TURN
    /// server: we use it for STUN, which needs no credentials.
    pub udp_transport: Option<String>,
}

/// Read the calling endpoints out of a live session's directory.
pub fn endpoints(session: &Session) -> Result<Endpoints> {
    let conversation_service = session
        .endpoint("calling_conversationServiceUrl")
        .context("no calling_conversationServiceUrl in regionGtms")?
        .to_string();
    let trouter = session
        .endpoint("calling_trouterUrl")
        .context("no calling_trouterUrl in regionGtms")?
        .to_string();
    // The registrar is the same host messaging registers with, and the directory
    // names it too; fall back to the messaging constant rather than failing, because
    // a call can be set up without ever re-reading this key.
    let registrar = session
        .endpoint("calling_registrarUrl")
        .unwrap_or(crate::trouter::REGISTRAR)
        .to_string();
    Ok(Endpoints {
        conversation_service,
        trouter,
        registrar,
        udp_transport: session.endpoint("calling_udpTransportUrl").map(String::from),
    })
}

/// Our own identity for one call. `id` is our mri; the two ids are generated per
/// call and per endpoint and mean nothing outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalParticipant {
    pub id: String,
    pub display_name: String,
    /// The calling endpoint's registration id — stable while the endpoint is up.
    pub endpoint_id: String,
    /// This participant leg, fresh per call.
    pub participant_id: String,
}

impl LocalParticipant {
    /// The `sender` / `participants.from` object every payload repeats.
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "displayName": self.display_name,
            "endpointId": self.endpoint_id,
            "participantId": self.participant_id,
            "languageId": "en-us",
        })
    }
}

/// A media description as it travels: an SDP blob plus its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContent {
    pub blob: String,
    pub content_type: String,
}

impl MediaContent {
    /// Wrap an SDP the browser just produced.
    pub fn sdp(blob: impl Into<String>) -> Self {
        Self { blob: blob.into(), content_type: SDP_CONTENT_TYPE.to_string() }
    }

    fn json(&self) -> Value {
        json!({ "blob": self.blob, "contentType": self.content_type })
    }

    /// Read one out of a frame, accepting either spelling of the SDP label and
    /// refusing a content type we cannot hand a browser.
    fn parse(value: &Value) -> Option<Self> {
        let blob = value.get("blob").and_then(Value::as_str)?;
        if blob.trim().is_empty() {
            return None;
        }
        let content_type = value
            .get("contentType")
            .and_then(Value::as_str)
            .unwrap_or(SDP_CONTENT_TYPE);
        if !content_type.starts_with("application/sdp") {
            return None;
        }
        Some(Self { blob: blob.to_string(), content_type: content_type.to_string() })
    }
}

/// Where our callback links live: our calling trouter's surl plus the signaling
/// session id we made for this call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackBase {
    /// The trouter surl of the CALLING connection, with a trailing slash.
    pub surl: String,
    /// One signaling session per call (a uuid), so links of two calls never collide.
    pub session_id: String,
    /// An 8-hex cause id, exactly as the web client's `ti()` makes one.
    pub cause_id: String,
}

/// A surl with its allocation id replaced by an ellipsis, for a journal line.
///
/// The links a join publishes live on this address, and a refusal that names nothing
/// makes the address the first thing to compare with a working client's. Only its
/// SHAPE is printed — the host and the path, never the id: that id is what routes a
/// frame to this connection, so it is a key and the journal is not the place for it.
pub fn surl_shape(surl: &str) -> String {
    let Some((prefix, rest)) = surl.split_once("/v4/") else {
        return surl.to_string();
    };
    // `/v4/f/{id}/` — the flavour letter says which allocate flow answered, and the
    // segment after it is the id.
    let mut parts = rest.splitn(2, '/');
    let flavour = parts.next().unwrap_or_default();
    match parts.next() {
        Some(_) => format!("{prefix}/v4/{flavour}/…/"),
        None => format!("{prefix}/v4/{flavour}"),
    }
}

impl CallbackBase {
    /// Build one callback link: `{surl}callAgent/{sessionId}/{causeId}{path}`.
    pub fn link(&self, path: &str) -> String {
        let surl = if self.surl.ends_with('/') {
            self.surl.clone()
        } else {
            format!("{}/", self.surl)
        };
        format!("{surl}{CALL_AGENT}/{}/{}{path}", self.session_id, self.cause_id)
    }
}

/// The links the SERVICE hands us, flattened into one map.
///
/// The frames nest `links` objects at several depths and under several names
/// (`callNotification.links`, `conversation.links`, `mediaNegotiation.links`), and
/// the exact nesting differs between an invite, an acceptance and a renegotiation.
/// Collecting every `links` object in the frame into one map is what makes the
/// caller independent of that shape — and the names inside are stable, because they
/// are the web client's own `LINKS` table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Links(BTreeMap<String, String>);

impl Links {
    /// Walk a frame and merge every `links` object found in it. A later (deeper)
    /// occurrence wins, because a per-call link is nested under the per-conversation
    /// one and is the more specific of the two.
    pub fn collect(frame: &Value) -> Self {
        let mut map = BTreeMap::new();
        collect_links(frame, &mut map);
        Self(map)
    }

    /// The first of `names` this frame carried. Several spellings are tried per
    /// action because the service names the same action differently depending on
    /// which frame carries it (`accept` on one, `acceptance` on another).
    ///
    /// Nothing here has to check that the value is postable: `collect_links` admits only a
    /// URL into the table in the first place, so a shorthand like this tenant's own
    /// `mediaAnswer: "cc://ma"` is absent by the time any accessor asks. That filter stays
    /// in ONE place — a second copy here would be a second answer to "what can be POSTed
    /// to", and the two would drift.
    pub fn get(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|n| self.0.get(*n).map(String::as_str))
    }

    /// Every link name this frame carried, for a journal line that has to explain
    /// why an action was not available.
    pub fn names(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Accept an incoming call (we take the call; media is negotiated separately).
    pub fn accept(&self) -> Option<&str> {
        self.get(&["accept", "acceptance"])
    }

    /// ATTACH this endpoint to a forked incoming call, which is how a call is taken on this
    /// tenant — and it is a DIFFERENT door from `accept`, with a body of its own.
    ///
    /// A real one-to-one invite, measured 2026-09-04, carried `attach`, `progress`, `reject`
    /// and no `accept` at all — its `to.endpointId` was all zeroes, so the invite is
    /// addressed to the USER rather than to one device and forked to every endpoint they are
    /// signed in on. The link's own path says so (`/cc/v1/forked/…/attach`).
    ///
    /// It is deliberately NOT a third spelling of `accept`. That was tried first and the
    /// service refused it `400` with the field named — `{"errors":{"Attach":["The Attach
    /// field is required."]}}` — because the two doors want two envelopes
    /// ([`attach_payload`] against [`acceptance_payload`]). Folding them back together would
    /// post the wrong body to whichever link happened to win.
    pub fn attach(&self) -> Option<&str> {
        self.get(&["attach"])
    }

    /// Refuse an incoming call.
    pub fn reject(&self) -> Option<&str> {
        self.get(&["reject", "rejection", "mediaRejection"])
    }

    /// Answer the media offer with our own SDP.
    pub fn media_answer(&self) -> Option<&str> {
        self.get(&["mediaAnswer"])
    }

    /// Acknowledge an answer we received (the caller's side of the handshake).
    pub fn media_acknowledgement(&self) -> Option<&str> {
        self.get(&["mediaAcknowledgement"])
    }

    /// End the call. `hangup` ends OUR leg; `leave` leaves the conversation.
    ///
    /// `callLeg` is LAST and is the only one of these an incoming one-to-one ever offers.
    /// Measured 2026-09-04 over every frame of a call that really connected: the whole set of
    /// link names the service ever named on that path holds no `hangup`, no `end`, no `leave`
    /// and no `conversationEnd` — so the user's call ended here, their microphone was
    /// released, and the CALLER was told nothing (§ 8a). An outgoing call is answered a
    /// hangup link of its own, which is why only this direction was ever affected.
    ///
    /// **The leg is UNMEASURED as a hangup**, and it is last so it can only ever be used
    /// where the four proper spellings are absent. [`hangup_payload`] already carries a
    /// `callTransactionEnd`, which is what a leg would want; if the service refuses it, it
    /// will name the reason and the call still ends locally — the POST failing and there
    /// being no link at all are the same outcome for the user, so this can be no worse than
    /// the silence it replaces.
    pub fn hangup(&self) -> Option<&str> {
        self.get(&["hangup", "end", "leave", "conversationEnd", "callLeg"])
    }

    /// Keep the service from tearing the call down while it is still up.
    /// Where OUR own new media is offered — the link the service hands us on the
    /// acceptance, and the one an outgoing renegotiation is POSTed to.
    pub fn media_renegotiation(&self) -> Option<&str> {
        self.get(&["mediaRenegotiation"])
    }

    /// Where media of OUR OWN is offered, when the service names a door for it.
    ///
    /// The acceptance names this beside `mediaRenegotiation` and nothing had ever posted to it.
    /// The two read as a pair: one starts a negotiation this endpoint wants, the other answers
    /// or updates one already going. Every video section this app has offered on the
    /// renegotiation link has come back with a zeroed port, a camera as much as a screen
    /// (§ 10.8), so this is the cheapest untried thing.
    pub fn start_outgoing_negotiation(&self) -> Option<&str> {
        self.get(&["startOutgoingNegotiation"])
    }

    /// Where a MODALITY is added to a live conversation — a group modality on a 1:1, and the
    /// content-sharing session a screen share needs (see [`content_sharing_payload`]). It is
    /// not the second half of a JOIN, which is what reading it as one cost a debugging round.
    pub fn add_modality(&self) -> Option<&str> {
        self.get(&["addModality"])
    }

    pub fn keep_alive(&self) -> Option<&str> {
        self.get(&["keepAlive"])
    }

    pub fn mute(&self) -> Option<&str> {
        self.get(&["mute"])
    }

    /// Where a source request goes — the NEWER of the two spellings, which is what the
    /// client's own configuration uses on this tenant.
    ///
    /// Neither of these is in a join answer: both arrive on the `callAcceptance` frame, so
    /// they exist only because `merge` keeps every link the service has ever named
    /// (NATIVE-CALLING.md § 10.2).
    pub fn apply_channel_parameters(&self) -> Option<&str> {
        self.get(&["applyChannelParameters"])
    }

    /// The older spelling of the same thing, kept because the service offers both and a
    /// tenant that stops offering the newer one must not lose video with it.
    pub fn control_video_streaming(&self) -> Option<&str> {
        self.get(&["controlVideoStreaming"])
    }

    pub fn unmute(&self) -> Option<&str> {
        self.get(&["unmute"])
    }

    /// Where the user's own endpoint state — whether they are muted — is published, the way
    /// the client publishes it (see [`endpoint_state_payload`]). A CONVERSATION door: it is
    /// read off [`ConversationLeg`] first, and off this merged table only where no conversation
    /// was read.
    pub fn update_endpoint_state(&self) -> Option<&str> {
        self.get(&["updateEndpointState"])
    }

    /// The links of ONE object — its own top-level `links` and nothing nested under it —
    /// with the same URL-only filter [`Links::collect`] applies.
    ///
    /// It is what a conversation's doors are read with, because the client reads them so
    /// (`this.links[I.LINKS.LEAVE]=e.links.leave`, one level): a `leave` nested under the
    /// call leg or a sharing session is a different door with the same name, and
    /// [`Links::collect`], which lets the deepest one win, would hand it over.
    pub fn top_level(object: &Value) -> Self {
        let mut map = BTreeMap::new();
        if let Some(links) = object.get("links").and_then(Value::as_object) {
            for (name, url) in links {
                if let Some(url) = url.as_str()
                    && url.starts_with("http")
                {
                    map.insert(name.clone(), url.to_string());
                }
            }
        }
        Self(map)
    }

    /// These links minus every one whose URL starts with `prefix` — this call's own callback
    /// surl.
    ///
    /// The service echoes the callbacks this app published back inside the objects it
    /// answers with (`conversationRequest.links` carries `conversationUpdate`, and so on), and
    /// a door is somewhere this app POSTs to: posting a mute to our own trouter address would
    /// send it to ourselves and tell the meeting nothing. An empty prefix filters nothing.
    pub fn without_prefix(&self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self.clone();
        }
        Self(self.0.iter().filter(|(_, url)| !url.starts_with(prefix)).map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    /// Merge newer links over these. Every frame may refresh them, and a link the
    /// new frame does not mention stays valid.
    pub fn merge(&mut self, other: &Links) {
        for (k, v) in &other.0 {
            self.0.insert(k.clone(), v.clone());
        }
    }
}

/// Merge this level's own `links` FIRST, then recurse — so a deeper (more specific)
/// link overwrites the one above it whatever order the keys happen to be in.
fn collect_links(value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            if let Some(links) = map.get("links").and_then(Value::as_object) {
                for (name, url) in links {
                    // A `links` object also carries flags; only a URL may enter the
                    // table, because every entry is later POSTed to.
                    if let Some(url) = url.as_str()
                        && url.starts_with("http")
                    {
                        out.insert(name.clone(), url.to_string());
                    }
                }
            }
            map.values().for_each(|child| collect_links(child, out));
        }
        Value::Array(items) => items.iter().for_each(|i| collect_links(i, out)),
        _ => {}
    }
}

/// A call somebody is offering us, read out of a decoded trouter frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingCall {
    /// The service's own id for this call, used to tell one call's frames apart.
    pub call_id: String,
    pub caller_mri: String,
    pub caller_name: String,
    /// The chat or channel the call belongs to, when the invite names one.
    pub thread_id: Option<String>,
    /// The modalities the caller offered; audio-only when it holds just `"audio"`.
    pub modalities: Vec<String>,
    /// The caller's SDP offer, when the invite carried one. A call with no offer is
    /// answered by asking for one (`attach`), which this app does not do yet.
    pub offer: Option<MediaContent>,
    pub links: Links,
    /// The participant leg the service assigned us, echoed back in our answers.
    pub participant_id: Option<String>,
    /// What the invite says about the CONVERSATION the call belongs to, read beside the
    /// notification (`conversationInvitation`, `meetingInfo`, the frame's own `evt`).
    ///
    /// **It is read and journaled, and decides nothing yet.** Every field is read-from-bundle
    /// rather than measured (docs/calling-roadmap/incoming-and-conversation.md § 4.13), so the
    /// first live invites say what this tenant really sends before a ring is drawn as a group
    /// call or a meeting on the strength of it. An invite that carries none of it is the
    /// one-to-one this app has always answered.
    pub conversation: InviteConversation,
}

/// The conversation half of an invite. See [`IncomingCall::conversation`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InviteConversation {
    /// The invite's own `evt` — 107 for a one-to-one call, 109 for a group one, in the client's
    /// own table. `None` when the frame named none or named it as something else.
    pub evt: Option<i64>,
    /// The keys `conversationInvitation` carried, sorted, for the journal — a shape, never a
    /// value. Empty when the invite carried no such object.
    pub invitation_keys: Vec<String>,
    /// Where the conversation lives, when the invite names it: the door a join at answer time
    /// would post to.
    pub controller: Option<String>,
    /// `isMultiParty` and `isHostless`, as the invite states them.
    pub multi_party: Option<bool>,
    pub hostless: Option<bool>,
    /// The conversation's own subject, when it has one — a meeting's title, a group chat's
    /// name. User content: it is never written to the journal, only whether there is one.
    pub subject: Option<String>,
    /// Whether the invite belongs to a MEETING: it carries `meetingInfo` or `meetingData`, or
    /// its thread is a meeting's own (`19:meeting_…`).
    pub meeting: bool,
}

impl InviteConversation {
    /// One journal line's worth of shape: the event number, the keys, the two flags, and
    /// whether there is a subject and a meeting — no URL, no subject, no id.
    pub fn shape(&self) -> String {
        let flag = |value: Option<bool>| match value {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unstated",
        };
        format!(
            "evt {}, conversationInvitation {:?}, multi-party {}, hostless {}, subject {}, \
             meeting {}, controller {}",
            self.evt.map(|evt| evt.to_string()).unwrap_or_else(|| "unstated".into()),
            self.invitation_keys,
            flag(self.multi_party),
            flag(self.hostless),
            if self.subject.is_some() { "yes" } else { "no" },
            if self.meeting { "yes" } else { "no" },
            if self.controller.is_some() { "yes" } else { "no" },
        )
    }
}

/// Read the conversation half of an invite. Each field is looked for under `_decoded` first
/// and at the root after it, because whether a frame arrives wrapped depends on its size.
fn invite_conversation(frame: &Value, thread_id: Option<&str>) -> InviteConversation {
    let at = |path: &str| -> Option<&Value> {
        frame.pointer(&format!("/_decoded{path}")).or_else(|| frame.pointer(path))
    };
    let invitation = at("/conversationInvitation").filter(|v| v.is_object());
    let field = |name: &str| invitation.and_then(|inv| inv.get(name));
    let evt = frame.get("evt").or_else(|| frame.pointer("/_decoded/evt")).and_then(|evt| {
        evt.as_i64().or_else(|| evt.as_str().and_then(|s| s.trim().parse().ok()))
    });
    InviteConversation {
        evt,
        invitation_keys: invitation
            .and_then(Value::as_object)
            .map(sorted_keys)
            .unwrap_or_default(),
        controller: field("conversationController").and_then(Value::as_str).map(String::from),
        multi_party: field("isMultiParty").and_then(Value::as_bool),
        hostless: field("isHostless").and_then(Value::as_bool),
        subject: field("subject")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        meeting: at("/meetingInfo").is_some()
            || at("/meetingData").is_some()
            || thread_id
                .or_else(|| at("/groupChat/threadId").and_then(Value::as_str))
                .is_some_and(|t| t.starts_with("19:meeting_")),
    }
}

impl IncomingCall {
    /// True when the caller offered audio and nothing else. Video is not refused —
    /// it is simply not answered with a camera (see `MODALITY_AUDIO`).
    pub fn has_audio(&self) -> bool {
        self.modalities.iter().any(|m| m.eq_ignore_ascii_case(MODALITY_AUDIO))
    }
}

/// Read an incoming-call invite out of a decoded calling frame, or `None` when the
/// frame is any of the other things the calling socket carries (progress, roster,
/// hangup, telemetry).
///
/// The frame is what `trouter_events::CallFrame` produced: the outer envelope, with
/// the nested `cp`/`gp` payload expanded under `_decoded`. The notification is
/// looked for in both places, because whether it arrives wrapped depends on its
/// size rather than on its kind.
pub fn incoming_call_from_frame(frame: &Value) -> Option<IncomingCall> {
    let notification = ["/_decoded/callNotification", "/callNotification"]
        .iter()
        .find_map(|p| frame.pointer(p))?;

    let caller_mri = notification
        .pointer("/from/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // A caller we cannot name is still a caller: the backend resolves the name from
    // its own store afterwards, exactly as it does for a message.
    let caller_name = notification
        .pointer("/from/displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if caller_mri.is_empty() {
        return None;
    }

    let call_id = ["/callId", "/debugContent/callId", "/conversationId"]
        .iter()
        .find_map(|p| notification.pointer(p).and_then(Value::as_str))
        .or_else(|| frame.get("callId").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();

    let thread_id = ["/groupChat/threadId", "/threadId", "/conversationId"]
        .iter()
        .find_map(|p| notification.pointer(p).and_then(Value::as_str))
        .filter(|t| t.starts_with("19:"))
        .map(String::from);

    let modalities = notification
        .get("callModalities")
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().filter_map(|i| i.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_else(|| vec![MODALITY_AUDIO.to_string()]);

    let conversation = invite_conversation(frame, thread_id.as_deref());
    Some(IncomingCall {
        call_id,
        caller_mri,
        caller_name,
        thread_id,
        conversation,
        modalities,
        offer: notification.get("mediaContent").and_then(MediaContent::parse),
        links: Links::collect(notification),
        participant_id: notification
            .pointer("/to/participantId")
            .and_then(Value::as_str)
            .map(String::from),
    })
}

/// Why a call we were in is over, read out of a `callEnd` / `conversationEnd` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEnded {
    pub call_id: String,
    /// The service's own code, kept verbatim for the journal.
    pub code: i64,
    /// The service's own sub-code, kept for the journal: it is what tells two endings with
    /// one code apart (`5003` "Removing modality controller" against `5013` "no one else has
    /// joined", both code 0, measured on an unreachable call). 0 when the frame named none.
    pub sub_code: i64,
    /// The service's own phrase (e.g. `"CallEndReasonHangup"`), or empty.
    pub phrase: String,
    /// Who took the call instead, when the service says it was answered somewhere else —
    /// `(mri, the display name the frame carried)`. The name is the frame's and may be empty;
    /// the backend names the person from its own store before anything is drawn.
    pub accepted_elsewhere_by: Option<(String, String)>,
}

/// Read a call-ending frame, or `None` when the frame is something else.
///
/// Both endings are one case on purpose: whichever of the two arrives, the call is
/// over and the browser must stop holding the microphone.
///
/// **Three shapes, and the third is why `url` is a parameter.** The ending is a `callEnd` or a
/// `conversationEnd` object, wrapped (`/_decoded/…`) or not; or it is the BARE BODY of a frame
/// posted to one of our two end callbacks, which carries no such key at all. The client reads
/// it the same way — `let i = e.body.callEnd || e.body` — and this app missed it, so an ending
/// sent that way left the call drawn as live until something else ended it. The bare body is
/// only ever read on our own end paths: anywhere else a body with no ending key is not one.
///
/// **`callControllerTransactionEnd` overrides the outer ending**, which is again the client's
/// own reading (`i.callControllerTransactionEnd && (i = i.callControllerTransactionEnd)`): when
/// the call controller states how its transaction ended, that is the ending. Each field is read
/// off the override first and off the outer object where the override is silent, so a field
/// the service put on only one of the two is never lost.
pub fn call_ended_from_frame(url: &str, frame: &Value) -> Option<CallEnded> {
    let named = ["/_decoded/callEnd", "/callEnd", "/_decoded/conversationEnd", "/conversationEnd"]
        .iter()
        .find_map(|p| frame.pointer(p));
    let end = match named {
        Some(end) => end,
        None if url.contains(paths::CALL_END) || url.contains(paths::CONVERSATION_END) => {
            frame.get("_decoded").unwrap_or(frame)
        }
        None => return None,
    };
    let inner = end.get("callControllerTransactionEnd").filter(|v| v.is_object());
    let levels: Vec<&Value> = inner.into_iter().chain(std::iter::once(end)).collect();
    let read = |paths: &[&str]| -> Option<&Value> {
        levels.iter().find_map(|level| paths.iter().find_map(|p| level.pointer(p)))
    };
    let code = read(&["/code", "/transactionEnd/code"]).and_then(Value::as_i64).unwrap_or(0);
    let sub_code =
        read(&["/subCode", "/transactionEnd/subCode"]).and_then(Value::as_i64).unwrap_or(0);
    let phrase = read(&["/phrase", "/transactionEnd/phrase"])
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let accepted_elsewhere_by = read(&["/acceptedElsewhereBy"]).and_then(|by| {
        let id = by.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = by.get("displayName").and_then(Value::as_str).unwrap_or_default();
        (!id.is_empty() || !name.is_empty()).then(|| (id.to_string(), name.to_string()))
    });
    let call_id = read(&["/callId", "/debugContent/callId"])
        .and_then(Value::as_str)
        .or_else(|| frame.get("callId").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    Some(CallEnded { call_id, code, sub_code, phrase, accepted_elsewhere_by })
}

/// A ring this endpoint was offered was ANSWERED on another device — the user's phone, or a
/// delegate picking it up. Code 450, the client's own reading of it.
pub const END_REASON_ANSWERED_ELSEWHERE: &str = "CallEndReasonAnsweredElsewhere";
/// A ring this endpoint was offered was FORWARDED somewhere else. Code 451.
pub const END_REASON_FORWARDED: &str = "CallEndReasonForwarded";
/// A ring this endpoint was offered went to VOICEMAIL. Code 452.
pub const END_REASON_VOICEMAIL: &str = "CallEndReasonForwardedToVoicemail";
/// The CALLER gave up before anybody answered. Code 487.
pub const END_REASON_CALLER_CANCELLED: &str = "CallEndReasonCallerCancelled";
/// Nobody answered within this app's own ring bound (`CALL_RING_TIMEOUT` in
/// src/bin/server.rs), in either direction: a ring the user never took, or a call they placed
/// that nobody picked up. The page tells the two apart by the call's own direction.
pub const END_REASON_UNANSWERED: &str = "CallEndReasonUnanswered";

/// The name of an ending a RING can have, or `None` when the service's own phrase is all there
/// is to say.
///
/// These four codes mean something only about a call that is still being OFFERED to this
/// endpoint (`ringing`: an incoming call nothing here has connected): "answered elsewhere" said
/// about a conversation the user is in would be false. They are the client's own, read out of
/// its `handleCallEnd` with `acceptedElsewhereBy` beside them — which is also taken as a 450
/// when it arrives with another code, because naming who took the call is a statement no other
/// ending makes.
///
/// **Unmeasured in the positive.** An endpoint that never attached to the forked leg is told
/// nothing when a ring ends elsewhere (NATIVE-CALLING.md § 8a), and that is every endpoint of
/// this app today: the names become reachable on the tenant once the ring is claimed when it
/// starts (docs/calling-roadmap/incoming-and-conversation.md, M4b). Until then the mock is what
/// says them.
pub fn end_reason_for(ended: &CallEnded, ringing: bool) -> Option<&'static str> {
    if !ringing {
        return None;
    }
    match ended.code {
        _ if ended.accepted_elsewhere_by.is_some() => Some(END_REASON_ANSWERED_ELSEWHERE),
        450 => Some(END_REASON_ANSWERED_ELSEWHERE),
        451 => Some(END_REASON_FORWARDED),
        452 => Some(END_REASON_VOICEMAIL),
        487 => Some(END_REASON_CALLER_CANCELLED),
        _ => None,
    }
}

/// Whether two mris name one person.
///
/// The ids this app compares come from different services — the session's own identity and a
/// frame's `acceptedElsewhereBy` — and nothing has measured that both spell an oid with the same
/// case or the same `8:orgid:` prefix. Read wrong, "you answered on your phone" becomes "<your
/// own name> answered this call", so the comparison is made on the oid itself. An empty id names
/// nobody.
pub fn same_person(a: &str, b: &str) -> bool {
    fn oid(mri: &str) -> String {
        let lower = mri.trim().to_ascii_lowercase();
        lower.strip_prefix("8:orgid:").map(str::to_string).unwrap_or(lower)
    }
    let (a, b) = (oid(a), oid(b));
    !a.is_empty() && a == b
}

/// The name this app gives an ending the service reported as "no callee endpoints were
/// found": the person has no client signed in, so nothing could ring and the call was over
/// before it began.
///
/// It follows the `CallEndReason…` spelling every other explained ending uses, because the
/// page maps a NAME to a sentence — the service's own phrase is prose, and a sentence keyed
/// on prose breaks the day it is reworded.
pub const END_REASON_UNREACHABLE: &str = "CallEndReasonNobodyReachable";

/// A ringing call this machine let go of WITHOUT declining it: the page ended it with any
/// press but Decline (`call_hangup`'s `intent: "end"`), so nothing went to the service and the
/// caller goes on ringing the user's other devices.
///
/// It is a name of ours for the reason [`END_REASON_UNREACHABLE`] is, and the page draws
/// nothing for it: the user ended it themselves, and saying "declined" would be false.
pub const END_REASON_LEFT_RINGING: &str = "CallEndReasonLeftRinging";

/// A call the user was IN that ended because this backend stopped: a signal (a service stop, a
/// container or pod stop, Ctrl+C), the idle exit, or a restart. The process hangs up on its way
/// out, so the people on the call see the user leave rather than a participant gone silent.
///
/// A name of ours for the reason [`END_REASON_UNREACHABLE`] is. A ring is never ended with it: a
/// call still ringing is dropped as the machine stopping taking calls, and goes on ringing the
/// user's other devices.
pub const END_REASON_BACKEND_STOPPING: &str = "CallEndReasonBackendStopping";

/// The service could not ring anybody it was asked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteFailed {
    /// The service's own sentence, kept verbatim for the journal.
    pub phrase: String,
    /// Whether it says the callee has no endpoint at all — nothing of theirs to ring, as
    /// opposed to a device that rang and refused.
    pub no_endpoints: bool,
}

/// Sub-codes the service uses for "there was nothing to ring". Measured on this tenant, on a
/// one-to-one call to somebody with no client signed in.
const NO_ENDPOINT_SUB_CODES: [i64; 2] = [10037, 5205];

/// Read an `addParticipantFailure` frame — the service saying the invitation reached nobody.
///
/// **It is the only frame that names the CAUSE.** Measured, in order, on a call to somebody
/// with no client signed in:
///
/// ```text
/// /conversation/addParticipantFailure/  code 480 subCode 10037 "No callee endpoints were found."
///                                      code 580 subCode 5205  "Audio-video modality controller …"
/// /call/end/                            code 0   subCode 5003  "Removing modality controller …"
/// /conversation/conversationEnd/        code 0   subCode 5013  "This conversation has ended as
///                                                              no one else has joined …"
/// ```
///
/// `call_ended_from_frame` reads the third of those, whose phrase names the SYMPTOM. So this
/// is read first and remembered, or the only thing the user is ever told about an unreachable
/// colleague is "The call ended." — which is what happened, and it read as this app dropping
/// the call two seconds in.
///
/// The frame is recognised by the callback path it was POSTed to, because its body carries no
/// wrapper naming it; the reason is then found wherever it sits, since the service nests one
/// per participant and one for the modality controller.
pub fn invite_failed(url: &str, body: &Value) -> Option<InviteFailed> {
    if !url.contains("addParticipantFailure") {
        return None;
    }
    let mut phrase = String::new();
    let mut no_endpoints = false;
    walk_failure(body, &mut phrase, &mut no_endpoints);
    Some(InviteFailed { phrase, no_endpoints })
}

/// The first `phrase` at any depth, and whether any sub-code (or any phrase) says there was
/// nothing to ring. Both halves are read, because a sub-code is a stable key and a phrase is
/// what a reader of the journal understands.
fn walk_failure(value: &Value, phrase: &mut String, no_endpoints: &mut bool) {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get("phrase").and_then(Value::as_str) {
                if phrase.is_empty() {
                    *phrase = text.to_string();
                }
                if text.to_lowercase().contains("no callee endpoints") {
                    *no_endpoints = true;
                }
            }
            if let Some(code) = map.get("subCode").and_then(Value::as_i64) {
                if NO_ENDPOINT_SUB_CODES.contains(&code) {
                    *no_endpoints = true;
                }
            }
            for child in map.values() {
                walk_failure(child, phrase, no_endpoints);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_failure(item, phrase, no_endpoints);
            }
        }
        _ => {}
    }
}

/// Read the far side's SDP answer out of a `mediaAnswer` frame, or `None` when the
/// frame is something else. This is what turns a ringing outgoing call into audio.
pub fn media_answer_from_frame(frame: &Value) -> Option<MediaContent> {
    [
        "/_decoded/mediaAnswer",
        "/mediaAnswer",
        "/_decoded/mediaNegotiation",
        "/mediaNegotiation",
        // A `callAcceptance` carries the answer TOO, and for a meeting join it is the
        // only frame that ever does — the service accepts and answers in one. Reading
        // only the two names above left the page holding an offer nothing ever answered,
        // so the call sat at "Joining…" until the service gave up on it.
        "/_decoded/callAcceptance",
        "/callAcceptance",
    ]
    .iter()
    .find_map(|p| frame.pointer(p))
    .and_then(|answer| answer.get("mediaContent"))
    .and_then(MediaContent::parse)
}

/// One line per media SECTION of an SDP: its kind, whether the far side accepted it, its mid
/// and the label that says what it carries.
///
/// **It exists because a screen share failed live and left nothing on this machine to read.**
/// The user shared their screen, the service answered by REJECTING the section, and the
/// journal said only that an offer had gone out — so which section was refused, and whether
/// the answer carried the audio at all, were unanswerable after the fact. The modalities were
/// logged and they are a claim about what we asked for; this is what came back.
///
/// It prints the SHAPE and never the content, which is the rule `web/scripts/join-live.ts`
/// already follows for the same job: no candidate, no fingerprint, no ICE credential and no
/// key ever reaches a log line here. A port is stated only as accepted or rejected, because
/// zero is the whole fact and the number is a relay's address.
pub fn media_sections(sdp: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut section: Option<(String, bool)> = None;
    let mut mid: Option<String> = None;
    let mut label: Option<String> = None;
    let mut flush = |section: &Option<(String, bool)>, mid: &Option<String>, label: &Option<String>, out: &mut Vec<String>| {
        let Some((kind, rejected)) = section else {
            return;
        };
        let mut line = kind.clone();
        if let Some(mid) = mid {
            line.push_str(&format!(" mid={mid}"));
        }
        if let Some(label) = label {
            line.push_str(&format!(" label={label}"));
        }
        line.push_str(if *rejected { " REJECTED" } else { " accepted" });
        out.push(line);
    };
    for line in sdp.lines().map(str::trim_end) {
        if let Some(rest) = line.strip_prefix("m=") {
            flush(&section, &mid, &label, &mut out);
            mid = None;
            label = None;
            let mut fields = rest.split(' ');
            let kind = fields.next().unwrap_or("?").to_string();
            // `m=<kind> <port> …` — a zero port is how either side says the section is gone.
            let rejected = fields.next() == Some("0");
            section = Some((kind, rejected));
        } else if let Some(rest) = line.strip_prefix("a=mid:") {
            mid = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("a=label:") {
            label = Some(rest.trim().to_string());
        }
    }
    flush(&section, &mid, &label, &mut out);
    out
}

/// Whether an SDP blob is an OFFER, read out of the blob itself.
///
/// **It is a rail rather than the primary mechanism, and saying so is the point.** The REAL CLIENT
/// tells the two apart by the CALLBACK a frame arrived on — `pn.MEDIA_NEGOTIATION` routes to
/// `handleMediaNegotiationOffer` and `pn.MEDIA_ANSWER` to `handleMediaAnswer`, read out of
/// `calling-pluginless-<hash>.js` — so the answer to an outgoing renegotiation comes back on
/// `/call/mediaAnswer/` and not on the renegotiation door. An earlier round of this work guessed
/// the opposite and wrote it down as the cause of a lost camera; the client's own code disproves
/// it, and the real cause was the GLARE that follows either way (see
/// `remoteOfferWouldRollBackOurs` in web/src/lib/call-media.ts, measured in a real browser by
/// web/e2e/webrtc-glare.spec.ts).
///
/// What this still buys is that `media_renegotiation_from_frame` and [`media_answer_from_frame`]
/// match the SAME `/mediaNegotiation` pointer, and the first runs first — so a frame the service
/// puts an ANSWER in, on any door, would be handed to the page as an offer and applied as one. The
/// discriminator costs a line scan and closes that whatever the routing does tomorrow.
///
/// `a=setup:actpass` is it, and it is not a guess about this service: RFC 5763 has an offer OFFER
/// both roles and an answer state the one it TOOK. `web/src/lib/ms-sdp.ts`'s own `isAnswer` already
/// reads exactly this, for the same reason and in the same direction — read off the ABSENCE of
/// `actpass` rather than the presence of a role, because one rejected section carrying a role of
/// its own is enough to make a whole offer read as an answer.
pub fn sdp_is_offer(sdp: &str) -> bool {
    sdp.lines().any(|line| line.trim_end() == "a=setup:actpass")
}

/// Which of OUR callbacks the service posted to, as the trailing path alone.
///
/// The surl in front of it is a session key, so only the tail travels — the discipline
/// [`media_sections`] and `web/scripts/join-live.ts` already hold for everything they print. It
/// exists because "a media answer arrived" says nothing about WHICH negotiation it answers on a
/// tenant that uses one callback for two jobs (see [`sdp_is_offer`]).
pub fn callback_path(url: &str) -> &str {
    // `…/callAgent/<session>/<correlation>/call/mediaAnswer/` — the last two segments are the
    // whole of the answer, and everything before them names the call. A query goes first: the
    // service appends one to some of the links it publishes (`?i=<ip>` on the forked leg, § 8a),
    // and a path read through it would print the address rather than the door.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let trimmed = path.trim_end_matches('/');
    match trimmed.rmatch_indices('/').nth(1) {
        Some((at, _)) => &trimmed[at + 1..],
        None => "?",
    }
}

/// The signaling session a frame is addressed to: the segment after `callAgent/` in the URL it
/// arrived on, or `None` for a URL with no such segment.
///
/// Every callback this app publishes is `{surl}callAgent/{sessionId}/{causeId}{path}`
/// ([`CallbackBase::link`]), and the service posts to exactly the links it was handed, so the
/// segment names the call those links were built for. The query goes first, as in
/// [`callback_path`]: the service appends one to some links (`?i=<ip>`), and nothing in a query
/// is a session. An EMPTY segment is still a segment — it names our call agent and no session of
/// ours, so it can match no live call.
pub fn session_of(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let marker = format!("/{CALL_AGENT}/");
    let (_, after) = path.split_once(marker.as_str())?;
    Some(after.split('/').next().unwrap_or_default())
}

/// Where a calling frame belongs, decided from its address alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRoute<'a> {
    /// Addressed to the live call's own callbacks: the frame is about the call this machine
    /// holds, and every branch after the routing may act on it.
    Ours,
    /// Addressed to a signaling session that is not the live call's — a call that has ended, a
    /// ring this machine let go of whose conversation keeps posting, or one it never held. Not
    /// ours to act on: merged in, its links would overwrite the live call's, and its
    /// `conversationEnd` would end a call it is not about.
    Elsewhere { session: &'a str },
    /// The address carries no session at all. Handled as before, because no measurement says
    /// such a frame never acts on a call; the journal is what will.
    Unaddressed,
}

/// Route a frame by the session its address names, against the live call's session.
///
/// The client is safe here for a reason this app was not: it routes each callback to the
/// signaling session that owns its path, and a session that ended is IDLE —
/// `handleCallEnd` returns early with `not handling incoming callEnd since fsmstate =
/// SIGNALING_FSM_STATE.IDLE` (calling-pluginless.js @1389185). This app had one live call and
/// merged EVERY frame into it, so a frame from any other conversation could rewrite its links
/// or end it.
///
/// `live` is the session of the call this machine holds, ended or not: a frame for the call's
/// own session after it ended is still that call's, and the handler's own phase checks are what
/// leave it alone.
pub fn frame_route<'a>(url: &'a str, live: Option<&str>) -> FrameRoute<'a> {
    match session_of(url) {
        None => FrameRoute::Unaddressed,
        Some(session) if live == Some(session) => FrameRoute::Ours,
        Some(session) => FrameRoute::Elsewhere { session },
    }
}

/// How an UNADDRESSED frame's address is named in a journal line: the part after this
/// connection's surl, never the surl itself.
///
/// The surl's allocation id is what routes a frame to this connection, so it is a key — the
/// discipline [`surl_shape`] and [`callback_path`] already hold. A frame on the surl itself is
/// said in words, and one on an address that is not under this connection's surl is not printed
/// at all, because nothing here can say which of its parts is a key.
pub fn unaddressed_frame_shape(url: &str, surl: Option<&str>) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    // The surl must end at a SEGMENT boundary: `…/f/abc` is a prefix of `…/f/abcdef/x` as a
    // string, and stripping it there would print the tail of somebody else's allocation id.
    let rest = surl
        .and_then(|s| path.strip_prefix(s.trim_end_matches('/')))
        .filter(|rest| rest.is_empty() || rest.starts_with('/'));
    let Some(rest) = rest else {
        return "an address that is not under this connection's surl".to_string();
    };
    match rest.trim_matches('/') {
        "" => "the connection's own address".to_string(),
        tail => tail.to_string(),
    }
}

/// The content-sharing SESSION a meeting grants, and the links it hands back.
///
/// **A meeting has ONE screen at a time, so sharing one is a session rather than a track.**
/// Measured on 2026-08-06: a meeting answered an `applicationsharing-video` section of ours
/// by rejecting it outright — no mid, no label, just a zeroed port — with the section
/// negotiated correctly, labelled correctly and offering the codecs a client offers. What
/// this app never did is ASK to be the presenter, which the client does first
/// (`startContentSharingAsync` → the session's `start`, POSTing a `contentSharing` blob to
/// the `addModality` link and setting `isPresenter` on the answer).
///
/// The links come back on that answer, and they are kept APART from the call's own
/// ([`Links::collect`] takes the deepest of a name, and this answer carries a `leave` of its
/// own — merged in, it would overwrite the link this app hangs a CALL up on).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentSharing {
    /// The correlation id this app minted for the session, which every later call to it
    /// carries — in a header on the way out, and in the frames the service sends back.
    pub correlation_id: String,
    /// The service's own id for the session, once it names one.
    pub session_id: Option<String>,
    /// Where to give the session up. Without it a share could be started and never stopped,
    /// which is the shape this project refuses on principle.
    pub leave: Option<String>,
}

impl ContentSharing {
    /// Read the session out of the `addModalitySuccess` FRAME, which is where it really
    /// arrives.
    ///
    /// Measured 2026-08-06 with `bun run join-live -- --share`: the POST answers `{}` with no
    /// links at all, and the service then POSTs to our own `addModalitySuccess` callback
    /// carrying the session's six — `contentSharingController`, `leave`, `notificationLinks`,
    /// `sync`, `takeControl`, `updateSessionState`. The client's own `start` says the same
    /// thing in its shape: it returns a deferred that the frame resolves, so it WAITS for this
    /// before it offers anything.
    pub fn from_frame(correlation_id: &str, frame: &Value) -> Option<Self> {
        // Collected at every DEPTH rather than read off a pointer. The service names the
        // frame's own body after its type and wraps it differently on different paths — the
        // shape was guessed twice and missed twice — and the collector is the reader this crate
        // already trusts for exactly that reason. It is safe HERE because nothing is merged
        // into the call's links: one value is taken out, and the session keeps it.
        let leave = Links::collect(frame).get(&["leave"])?.to_string();
        Some(Self {
            correlation_id: correlation_id.to_string(),
            session_id: session_id_in(frame),
            leave: Some(leave),
        })
    }

    /// Read the session out of the answer to the `addModality` POST.
    ///
    /// Only two things are taken: the id the service named it, and the way out. The other
    /// four links it offers — `contentSharingController`, `takeControl`, `updateSessionState`,
    /// `sync` — belong to features this app does not have, and a link nothing posts to is a
    /// link that goes stale in a struct.
    pub fn from_answer(correlation_id: &str, answer: &Value) -> Self {
        let sharing = ["/_decoded/contentSharing", "/contentSharing"]
            .iter()
            .find_map(|p| answer.pointer(p));
        Self {
            correlation_id: correlation_id.to_string(),
            session_id: sharing
                .and_then(|s| s.get("sessionId"))
                .and_then(Value::as_str)
                .map(String::from),
            leave: sharing
                .and_then(|s| s.get("links"))
                .and_then(|l| l.get("leave").or_else(|| l.get("contentSharingLeave")))
                .and_then(Value::as_str)
                .map(String::from),
        }
    }
}

/// Find `contentSharingSessionId` at any depth, which is where the service really puts it:
/// on the grant's own frame, and on the roster's `contentSharing.sessionInformation`.
fn session_id_in(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(id) = map.get("contentSharingSessionId").and_then(Value::as_str) {
                return Some(id.to_string());
            }
            map.values().find_map(session_id_in)
        }
        Value::Array(items) => items.iter().find_map(session_id_in),
        _ => None,
    }
}

/// Ask a meeting to make this endpoint the presenter of a content-sharing session.
///
/// The client's own `j2`, field for field: the local participant, an empty `to`, the
/// `contentSharing` blob and the two callbacks the service reports the session's own changes
/// on. `subject` and `sessionState` are NULL because the client's builder passes
/// `i || null` / `t || null` and this app has neither to state — a screen has no subject and
/// the state is the service's to decide. `sequenceNumber` is 1, which is the literal the
/// client sends on a start.
///
/// It carries no `payload` envelope, for the reason § Joining a meeting gives: every builder
/// in the client's bundle returns one and its transport strips it, so a wrapped body is
/// refused `400` with `{}` and names nothing.
pub fn content_sharing_payload(
    local: &LocalParticipant,
    callbacks: &CallbackBase,
    identifier: &str,
) -> Value {
    json!({
        "participants": { "from": local.json(), "to": [] },
        "contentSharing": {
            "identifier": identifier,
            "subject": Value::Null,
            "sessionState": Value::Null,
            "sequenceNumber": 1,
            "links": {
                "sessionUpdate": callbacks.link(paths::CONVERSATION_CONTENT_SHARING_UPDATE),
                "sessionEnd": callbacks.link(paths::CONVERSATION_CONTENT_SHARING_END),
            },
        },
        "links": {
            "addModalitySuccess": callbacks.link(paths::CONVERSATION_ADD_MODALITY_SUCCESS),
            "addModalityFailure": callbacks.link(paths::CONVERSATION_ADD_MODALITY_FAILURE),
        },
    })
}

/// Give the session up: the body the client's own `K2` sends, which names the same two
/// callbacks and nothing else.
pub fn content_sharing_leave_payload(local: &LocalParticipant, callbacks: &CallbackBase) -> Value {
    json!({
        "participants": { "from": local.json() },
        "contentSharing": {
            "links": {
                "sessionUpdate": callbacks.link(paths::CONVERSATION_CONTENT_SHARING_UPDATE),
                "sessionEnd": callbacks.link(paths::CONVERSATION_CONTENT_SHARING_END),
            },
        },
    })
}

/// Every line of an SDP that is not a key, an address or a candidate — for a refusal that
/// names no line.
///
/// The service answers `SdpParsingFailure` and says nothing else, so the only way forward is
/// to read what was really posted. It prints the STRUCTURE: `v=`, `o=` reduced to its shape,
/// the session attributes, and each section's own lines — with every candidate, fingerprint,
/// ICE credential and SSRC dropped, which is the discipline `web/scripts/join-live.ts` follows
/// for the same reason.
pub fn sdp_structure(sdp: &str) -> Vec<String> {
    const SECRET: [&str; 7] = [
        "a=candidate:",
        "a=fingerprint:",
        "a=ice-pwd:",
        "a=ice-ufrag:",
        "a=ssrc:",
        "a=x-candidate-ipv6:",
        "o=",
    ];
    sdp.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !SECRET.iter().any(|p| line.starts_with(p)))
        .map(String::from)
        .collect()
}

/// A media OFFER the service made mid-call, and where to answer it.
///
/// **This is how a shared screen arrives, and it arrives unprompted.** Measured: ~9 s into
/// every join the service POSTs a `mediaRenegotiation` whose offer carries the sections it
/// is willing to send — and one second after somebody shares their screen, that offer grows
/// `label:applicationsharing-video` at mid 3, `sendonly`, with its SSRC range declared
/// (NATIVE-CALLING.md § 10.3a). So there is nothing to ask for: the section is offered, and
/// the only question is whether this app answers.
///
/// It was previously read as an ANSWER (`media_answer_from_frame` matches `mediaNegotiation`
/// too), which meant the page was handed an offer where it expected an answer, checked its
/// signaling state, and dropped it — every time, silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRenegotiation {
    pub offer: MediaContent,
    /// The link to POST the answer to. **The frame's OWN link**, never the merged set: the
    /// service names where this negotiation is answered, and an older call's link would
    /// answer nothing.
    pub answer_link: String,
    /// Where to refuse it instead, when it offers something this app cannot carry.
    pub reject_link: Option<String>,
    /// What the offer says it is for, when it says.
    pub modalities: Vec<String>,
}

/// Read a renegotiation offer out of a frame, or `None` when the frame is not one.
///
/// **The test is the SDP's own `a=setup:actpass`, and the link is only where to answer.** It used to
/// be the ANSWER LINK alone — a frame that offered media and said where to answer read as a
/// renegotiation — and this reader and [`media_answer_from_frame`] match the SAME
/// `/mediaNegotiation` pointer, with this one running first. So a body carrying an ANSWER and an
/// answer link beside it was handed to the page as an offer and applied to the connection as one,
/// which in `have-local-offer` costs the offer we were waiting on (see [`sdp_is_offer`], which also
/// records what the real client does and what an earlier round of this got wrong).
///
/// A blob that is an ANSWER therefore falls through to [`media_answer_from_frame`], which is the
/// reader that applies it to the offer it belongs to — whichever door the service used.
pub fn media_renegotiation_from_frame(frame: &Value) -> Option<MediaRenegotiation> {
    let negotiation = [
        "/_decoded/mediaNegotiation",
        "/mediaNegotiation",
        "/_decoded/newMediaOffer",
        "/newMediaOffer",
    ]
    .iter()
    .find_map(|p| frame.pointer(p))?;
    let links = negotiation.get("links");
    let answer_link = links
        .and_then(|l| l.get("mediaAnswer"))
        .and_then(Value::as_str)
        // Some frames put the links beside the negotiation rather than inside it.
        .or_else(|| frame.pointer("/links/mediaAnswer").and_then(Value::as_str))?;
    let offer = negotiation.get("mediaContent").and_then(MediaContent::parse)?;
    // An ANSWER is not a renegotiation, whatever links it carries beside it.
    if !sdp_is_offer(&offer.blob) {
        return None;
    }
    Some(MediaRenegotiation {
        offer,
        answer_link: answer_link.to_string(),
        reject_link: links
            .and_then(|l| l.get("rejection").or_else(|| l.get("mediaRejection")))
            .and_then(Value::as_str)
            .map(String::from),
        modalities: negotiation
            .get("callModalities")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

/// What a source request asks for when the caller names nothing better.
///
/// An H.264 fmtp, and the shape is the client's own capability probe — `max-fps` is in
/// hundredths of a frame per second (RFC 6184), so `3000` is 30. `profile-level-id=42C02A`
/// is constrained baseline at level 4.2, which is what Teams negotiates and what every
/// browser decodes. A page drawing several tiles should ask for less than this per tile; a
/// page drawing one shared screen wants all of it, because a screen is text.
pub const DEFAULT_VIDEO_FMTP: &str = "max-fs=8160;max-mbps=245000;max-fps=3000;\
     profile-level-id=42C02A;packetization-mode=1";

/// One stream to subscribe to: put `source_id` on the section named `mid`.
///
/// `stream_msid` and `mid` are the PAGE's own — the receive stream's id as the browser
/// reported it on the `track` event, and the section it arrived on. Neither exists before the
/// answer is applied, which is why a subscription is strictly after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRequest {
    pub mid: String,
    pub source_id: i64,
    pub stream_msid: String,
    /// An H.264 fmtp, which is how a client asks for a small tile rather than a full stream.
    /// The client's own builder throws without it, so it is not optional here either.
    pub fmt_params: String,
}

/// Build the body that subscribes one of our receive sections to somebody's stream.
///
/// Two spellings, because the client sends whichever its config names and both links arrive
/// on the `callAcceptance` frame (NATIVE-CALLING.md § 10.2). `applyChannelParameters` is the
/// newer one — it addresses the section by mid, and its media parameter is a JSON STRING
/// inside the JSON, which is the service's own shape and not an accident of this port.
pub fn source_request_payload(request: &SourceRequest, sequence: u64, modern: bool) -> Value {
    let control = json!({
        "sourceId": request.source_id,
        "streamMsid": request.stream_msid,
        "fmtParams": request.fmt_params,
    });
    if modern {
        let parameter = json!({
            "controlVideoStreaming": { "sequenceNumber": sequence, "controlInfo": control }
        });
        return json!({
            "applyChannelParameters": {
                "multiChannelParameter": {
                    "mids": [request.mid],
                    // A string, deliberately: `JSON.stringify` in the client's own message
                    // generator, and the service reads it back the same way.
                    "mediaParameter": parameter.to_string(),
                }
            }
        });
    }
    // The older shape, whose `controlInfo` is an ARRAY and which names the control itself.
    json!({
        "controlVideoStreaming": {
            "sequenceNumber": sequence,
            "controlInfo": [{
                "control": "start",
                "sourceId": request.source_id,
                "streamMsid": request.stream_msid,
                "fmtParams": request.fmt_params,
            }]
        }
    })
}

/// The url a `callAcceptance` must be acknowledged on, if it named one.
///
/// It is the acceptance's OWN link and never the merged set: the service waits for this
/// one POST and ends the call without it —
/// `Call Controller timed out while waiting for acknowledgement` after 30 seconds,
/// which is what a joined meeting that never carried audio looked like.
pub fn acceptance_acknowledgement_link(frame: &Value) -> Option<&str> {
    ["/_decoded/callAcceptance/links", "/callAcceptance/links"]
        .iter()
        .find_map(|p| frame.pointer(p))
        .and_then(|links| {
            ["acknowledgement", "acknowledgment"].iter().find_map(|name| links.get(name))
        })
        .and_then(Value::as_str)
}

/// The body of that acknowledgement: the links the rest of the call may use.
///
/// It carries no media and confirms nothing about the answer — it is the client's own
/// `callAcceptanceAcknowledgement`, which publishes where the service may renegotiate,
/// transfer or replace this call. Seven links, exactly the ones the real client sends.
pub fn acceptance_acknowledgement_payload(callbacks: &CallbackBase) -> Value {
    json!({
        "callAcceptanceAcknowledgement": {
            "links": {
                "mediaRenegotiation": callbacks.link(paths::CALL_MEDIA_RENEGOTIATION),
                "transfer": callbacks.link(paths::CALL_TRANSFER),
                "replacement": callbacks.link(paths::CALL_REPLACEMENT),
                "balanceUpdate": callbacks.link(paths::CALL_BALANCE_UPDATE),
                "retargetCompletion": callbacks.link(paths::CALL_RETARGET_COMPLETION),
                "controlVideoStreaming": callbacks.link(paths::CALL_CONTROL_VIDEO_STREAMING),
                "updateMediaDescriptions":
                    callbacks.link(paths::CALL_UPDATE_MEDIA_DESCRIPTIONS),
            }
        }
    })
}

/// Where the MEDIA CONTROLLER may reach this endpoint — the client's own
/// `clientContentForMediaController`.
///
/// **It is the one thing every body that carries an SDP sends and this app sent on none of them,
/// and it is the leading candidate for a subscription the service ACCEPTS and then never sends a
/// byte on.** The calling service and the media controller are two components: signaling accepts
/// `applyChannelParameters` and answers 200, and the controller is what actually puts a source on
/// a section. Read out of the real client (`getClientUrls` in `calling-pluginless-<hash>.js`):
///
/// ```js
/// this.getClientUrls = () => {
///   let i = null;
///   return this.isWebRtcCall && (i = {
///     controlVideoStreaming: yt(this.signalingSession, it.CONTROL_VIDEO_STREAMING),
///     csrcInfo: yt(this.signalingSession, it.CSRC_INFO),
///   }, …), i
/// }
/// ```
///
/// Note the `isWebRtcCall` gate: these urls are how a client DECLARES itself a WebRTC endpoint the
/// controller can drive, which is exactly what this app is. It is sent on the call invitation, on
/// the join, on the acceptance and on every media answer — four bodies, one statement — so it is
/// built once here.
///
/// `dominantSpeakerInfo` is the client's third entry and is behind a config flag
/// (`isDominantSpeakerInfoLinkEnabled`); it is deliberately not sent, because nothing here reads a
/// dominant speaker and a link nothing posts to goes stale — the rule `takeControl` already
/// follows.
pub fn media_controller_content(callbacks: &CallbackBase) -> Value {
    json!({
        "controlVideoStreaming": callbacks.link(paths::CALL_CONTROL_VIDEO_STREAMING),
        "csrcInfo": callbacks.link(paths::CALL_CSRC_INFO),
    })
}

/// True when the frame says the far side accepted (they picked up). Audio still
/// waits for the media answer; this is what stops the ringing tone.
pub fn call_accepted_in_frame(frame: &Value) -> bool {
    ["/_decoded/callAcceptance", "/callAcceptance", "/_decoded/callConnected"]
        .iter()
        .any(|p| frame.pointer(p).is_some())
}

/// What an acceptance says about keeping the call alive: how often the service wants to hear
/// from this endpoint, and whether it named the door to say it on.
///
/// The client reads both off the acceptance's acknowledgement
/// (`this.links[KEEPALIVE] = e.callAcceptanceAcknowledgement.links.callLeg`,
/// `scheduleKeepAlives(e.callAcceptanceAcknowledgement.callKeepAliveInterval)`), which arrives
/// INLINE in the answer to our own acceptance of an incoming call, and on the `callAcceptance`
/// frame of a call we placed. **This app sends no keep-alive the service asked for**, and the
/// interval has never been seen on this tenant — so it is read here for the journal alone, and
/// what a later keep-alive is built on is the value the first live calls write down
/// (docs/calling-roadmap/incoming-and-conversation.md § 4.4 and M6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAliveOffer {
    /// Which object it was read off: `"callAcceptanceAcknowledgement"` or `"callAcceptance"`.
    pub source: &'static str,
    /// `callKeepAliveInterval`, in seconds, as the service stated it.
    pub interval_s: Option<i64>,
    /// Whether the object carried a `callLeg` link — the keep-alive's door.
    pub call_leg: bool,
}

impl KeepAliveOffer {
    /// One journal line's worth: the source, the interval and whether the door is there.
    pub fn shape(&self) -> String {
        format!(
            "{} names keep-alive interval {} and {} callLeg link",
            self.source,
            self.interval_s.map(|s| format!("{s} s")).unwrap_or_else(|| "none".into()),
            if self.call_leg { "a" } else { "no" },
        )
    }
}

/// Read a [`KeepAliveOffer`] out of an answer or a frame, or `None` when it carries neither
/// object. The acknowledgement is preferred, because it is the one the client schedules from.
pub fn keep_alive_offer(body: &Value) -> Option<KeepAliveOffer> {
    ["callAcceptanceAcknowledgement", "callAcceptance"].iter().find_map(|&source| {
        let object = body
            .pointer(&format!("/_decoded/{source}"))
            .or_else(|| body.get(source))
            .filter(|v| v.is_object())?;
        let interval_s = object.get("callKeepAliveInterval").and_then(|v| {
            v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        });
        let call_leg = object.pointer("/links/callLeg").and_then(Value::as_str).is_some();
        Some(KeepAliveOffer { source, interval_s, call_leg })
    })
}

/// The doors a CONVERSATION object names, read for the journal before anything is built on
/// them.
///
/// The client keeps a conversation's own links apart from the call leg's: `leave` is how an
/// answered incoming call is ended so the caller is told, `updateEndpointState` is where its
/// mute is published, `notificationLinks` and `unmute` are the others it reads — all from the
/// TOP-LEVEL `links` of the object (docs/calling-roadmap/incoming-and-conversation.md § 4.3).
/// None of that is measured on this tenant, so the answers that could carry one say which they
/// did: the place answer, the join answer, an attach answer's
/// `/additionalActionResponses/0/output`, and every `conversationUpdate` frame. A key that
/// names a BROKER is listed too, because one has never been seen and the bundle mentions them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationDoors {
    /// Which object it was read off.
    pub source: &'static str,
    /// The names of its top-level `links`, sorted — never a URL.
    pub links: Vec<String>,
}

impl ConversationDoors {
    /// The four doors this workstream will build on, in that order.
    pub const WATCHED: [&'static str; 4] =
        ["leave", "updateEndpointState", "notificationLinks", "unmute"];

    /// One journal line's worth: which of the watched doors are there, then every link name.
    pub fn shape(&self) -> String {
        let has = |name: &str| self.links.iter().any(|link| link == name);
        let watched: Vec<String> = Self::WATCHED
            .iter()
            .map(|name| format!("{name} {}", if has(name) { "yes" } else { "no" }))
            .collect();
        let broker: Vec<&String> = self
            .links
            .iter()
            .filter(|link| link.eq_ignore_ascii_case("subscribe") || link.to_ascii_lowercase().contains("broker"))
            .collect();
        format!(
            "{}: {}; broker-shaped {:?}; links {:?}",
            self.source,
            watched.join(", "),
            broker,
            self.links,
        )
    }
}

/// Read the [`ConversationDoors`] of an answer or a frame, or `None` when it holds no
/// conversation object with links.
pub fn conversation_doors(body: &Value) -> Option<ConversationDoors> {
    let (source, object) = conversation_object(body)?;
    let links = object.get("links")?.as_object()?;
    Some(ConversationDoors { source, links: sorted_keys(links) })
}

/// The CONVERSATION object an answer or a frame carries, and where it was found — the one
/// place that says where to look, read by the journal ([`conversation_doors`]) and by the doors
/// a call posts to ([`ConversationLeg::from_response`]) alike, so the two cannot disagree about
/// which object a body's doors are.
///
/// Three places, first match wins: a `conversationUpdate` key (a frame wrapped the way the
/// trouter wraps one), an attach answer's `/additionalActionResponses/0/output` (the join that
/// rides an attach, docs/calling-roadmap/incoming-and-conversation.md § 4.3), and the body
/// itself — which is what the place and join answers are, and what the client reads a
/// `conversationUpdate` frame as (`handleConversationUpdate`: `const t=e.body`). An object
/// counts only when it carries a top-level `links` OBJECT: the client reads `e.links.leave`
/// unguarded, so a conversation object without one is not a shape it handles.
fn conversation_object(body: &Value) -> Option<(&'static str, &Value)> {
    let decoded = body.get("_decoded").unwrap_or(body);
    let candidates: [(&'static str, Option<&Value>); 3] = [
        ("conversationUpdate", decoded.get("conversationUpdate")),
        ("additionalActionResponses output", decoded.pointer("/additionalActionResponses/0/output")),
        ("conversation", Some(decoded)),
    ];
    candidates.into_iter().find_map(|(source, object)| {
        let object = object?;
        object.get("links")?.as_object()?;
        Some((source, object))
    })
}

/// The CONVERSATION leg of a call: the doors its conversation object names, and how new that
/// object was.
///
/// The client keeps these apart from the call leg's, and REPLACES them wholesale on every
/// conversation object it applies (`processConversationServiceUpdate`:
/// `this.links[I.LINKS.UNMUTE]=e.links.unmute||null`, and so on for every door) — which is how a
/// withdrawn door stops being offered. `CallSession.links` cannot do that: it MERGES, and a link
/// it once held stays for the life of the call. So this is a second table, built from the
/// object's TOP-LEVEL `links` only and replaced whole, and the doors that belong to the
/// conversation are read off it: `updateEndpointState` (where the mute is published),
/// `leave`, `mute` and `unmute`.
///
/// It is the structure docs/calling-roadmap/meeting-awareness.md § 4.1 rule 4 calls
/// `conversation_links`, and it is learned for EVERY call kind: an incoming 1:1 needs its
/// `leave` as much as a meeting needs its `unmute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationLeg {
    /// The conversation's own URL, when the object named one.
    pub controller: Option<String>,
    /// The object's top-level doors, minus anything on this call's own callback surl.
    pub links: Links,
    /// The object's `sequenceNumber`, or -1 when it carried none — the client's own reading
    /// (`a=e.sequenceNumber>=0?e.sequenceNumber:-1`).
    pub sequence_number: i64,
    /// What the service says the conversation is (`state`), as sent; `Null` when absent.
    pub state: Value,
}

impl ConversationLeg {
    /// Read the conversation leg out of an answer or a frame, or `None` when it carries no
    /// conversation object (see [`conversation_object`]). `own_surl` is this call's callback
    /// surl: a link on it is one of ours echoed back, never a door (see
    /// [`Links::without_prefix`]).
    pub fn from_response(body: &Value, own_surl: &str) -> Option<Self> {
        let (_, object) = conversation_object(body)?;
        let sequence_number = object
            .get("sequenceNumber")
            .and_then(|n| n.as_i64().or_else(|| n.as_str().and_then(|s| s.trim().parse().ok())))
            .filter(|n| *n >= 0)
            .unwrap_or(-1);
        Some(Self {
            controller: object.get("conversationController").and_then(Value::as_str).map(String::from),
            links: Links::top_level(object).without_prefix(own_surl),
            sequence_number,
            state: object.get("state").cloned().unwrap_or(Value::Null),
        })
    }

    /// Apply a newer object over this one, under the client's sequence gate: an object whose
    /// sequence is BELOW the last one applied is ignored, and anything else replaces this leg
    /// whole — doors and all. Returns whether it was applied.
    ///
    /// Equal is applied, and an object with no number reads as -1, exactly as the client reads
    /// them (`if(s&&a<this.updateSequenceNumber)…return`): so a frame with no number is ignored
    /// once a numbered one has been applied, and two unnumbered ones apply in arrival order.
    pub fn apply(&mut self, newer: ConversationLeg) -> bool {
        if newer.sequence_number < self.sequence_number {
            return false;
        }
        *self = newer;
        true
    }

    /// Where the user's own mute is published (see [`endpoint_state_payload`]).
    pub fn update_endpoint_state(&self) -> Option<&str> {
        self.links.update_endpoint_state()
    }

    /// Where this participant LEAVES the conversation — the door an answered incoming call is
    /// ended on so the caller is told (docs/calling-roadmap/incoming-and-conversation.md § 4.6).
    pub fn leave(&self) -> Option<&str> {
        self.links.get(&["leave"])
    }

    /// A SERVER mute of the participants it names — never the user's own toggle.
    pub fn mute(&self) -> Option<&str> {
        self.links.mute()
    }

    /// Where a participant the organizer muted asks to be heard again (§ 4.9).
    pub fn unmute(&self) -> Option<&str> {
        self.links.unmute()
    }
}

/// The settings key a backend keeps its [`CallProtocol`] under: one per PORT, because the two
/// send-capable installs on one machine share one store, and the whole point of a per-backend
/// setting is that the released build can try a protocol while the staged one keeps this app's
/// existing flow (docs/calling-roadmap/incoming-and-conversation.md § 6).
pub fn call_protocol_setting_key(port: u16) -> String {
    format!("calling_protocol:{port}")
}

/// How this machine's calls speak to Teams, where it has a choice between the flow this app has
/// always used and the one the Teams web client uses.
///
/// Every protocol change in docs/calling-roadmap/incoming-and-conversation.md sits behind a
/// field here, OFF by default, flipped by the user in Settings (`call_protocol_set`, a
/// write-token method) — never by an environment variable, which tooling could set. A call
/// keeps the protocol it started with, so a change applies from the next call.
///
/// The others the roadmap names (`keepalive`, and meeting-awareness's `server_mute`) join it in
/// the milestone that gives them something to switch: a choice that changes nothing reads as a
/// bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CallProtocol {
    /// How the user's own mute is published to the call.
    pub mute: MuteProtocol,
    /// How an incoming call is taken when the user answers it.
    pub incoming: IncomingProtocol,
}

/// How an incoming call is taken, on the user's own Answer press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IncomingProtocol {
    /// The measured flow (NATIVE-CALLING.md § 8a): attach to the forked leg with our answer in
    /// [`attach_payload`], then accept it. Verified both ways with a colleague on 2026-09-04 — and
    /// it never joins the call's CONVERSATION, so the conversation's `leave` is never learned and
    /// hanging up is posted on the leg (`callLeg`), which is unmeasured as a hangup.
    #[default]
    Answer,
    /// The client's own flow: the attach also JOINS the call's conversation in the same request
    /// ([`attach_join_payload`], the client's `WV`), so the call learns the conversation's doors —
    /// `leave`, which is how the client hangs up a call it joined, and `updateEndpointState`,
    /// where an answered call's mute is published. The acceptance after it is unchanged.
    JoinAtAnswer,
}

impl IncomingProtocol {
    /// Every word the setting takes, in the order the page offers them.
    pub const WORDS: [&'static str; 2] = ["answer", "join-at-answer"];

    pub fn word(self) -> &'static str {
        match self {
            IncomingProtocol::Answer => "answer",
            IncomingProtocol::JoinAtAnswer => "join-at-answer",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "answer" => Some(IncomingProtocol::Answer),
            "join-at-answer" => Some(IncomingProtocol::JoinAtAnswer),
            _ => None,
        }
    }
}

/// Where the user's own mute is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuteProtocol {
    /// `{muteUnmute: {…, scope: "Myself"}}` to the call's `mute` / `unmute` link — what this app
    /// has always sent. Unmeasured, and not the client's toggle; kept as the default because it
    /// is what every install runs today.
    #[default]
    Legacy,
    /// The client's own toggle: `{from, endpointState: {state: {isMuted}, …}}` to the
    /// conversation's `updateEndpointState` (see [`endpoint_state_payload`]), and never the
    /// `mute` link, which is a SERVER mute of the participants it names.
    EndpointState,
}

impl MuteProtocol {
    /// Every word the setting takes, in the order the page offers them.
    pub const WORDS: [&'static str; 2] = ["legacy", "endpoint-state"];

    pub fn word(self) -> &'static str {
        match self {
            MuteProtocol::Legacy => "legacy",
            MuteProtocol::EndpointState => "endpoint-state",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "legacy" => Some(MuteProtocol::Legacy),
            "endpoint-state" => Some(MuteProtocol::EndpointState),
            _ => None,
        }
    }
}

impl CallProtocol {
    /// Read the stored value. Absent, unreadable, or a word this build does not know — each
    /// field on its own — reads as that field's default: this app's existing flow is what
    /// an unrecognised value falls back to, and a key a NEWER build stored is ignored rather than
    /// fatal, because the two installs on one machine may be different builds.
    pub fn from_stored(raw: Option<&str>) -> Self {
        let stored: Value = raw.and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or(Value::Null);
        let word = |key: &str| stored.get(key).and_then(Value::as_str);
        Self {
            mute: word("mute").and_then(MuteProtocol::from_word).unwrap_or_default(),
            incoming: word("incoming").and_then(IncomingProtocol::from_word).unwrap_or_default(),
        }
    }

    /// The shape it is stored in and stated in: `{"incoming": "answer", "mute": "legacy"}`.
    pub fn json(self) -> Value {
        json!({ "mute": self.mute.word(), "incoming": self.incoming.word() })
    }

    /// This protocol with the changes an RPC asked for. Every key must be one this build knows
    /// and every value one of that key's words — a typo that silently changed nothing would
    /// leave the user believing a protocol is in force that is not. `write_token` is the write
    /// lock's own and is ignored.
    pub fn with_changes(self, params: &Value) -> Result<Self> {
        let mut next = self;
        let Some(params) = params.as_object() else {
            return Ok(next);
        };
        for (key, value) in params {
            match key.as_str() {
                "write_token" => {}
                "mute" => {
                    let word = value.as_str().unwrap_or_default();
                    next.mute = MuteProtocol::from_word(word).with_context(|| {
                        format!(
                            "call_protocol_set: `mute` must be one of {:?}",
                            MuteProtocol::WORDS
                        )
                    })?;
                }
                "incoming" => {
                    let word = value.as_str().unwrap_or_default();
                    next.incoming = IncomingProtocol::from_word(word).with_context(|| {
                        format!(
                            "call_protocol_set: `incoming` must be one of {:?}",
                            IncomingProtocol::WORDS
                        )
                    })?;
                }
                other => anyhow::bail!("call_protocol_set: this backend does not know `{other}`"),
            }
        }
        Ok(next)
    }
}

/// The `endpointState` a call this app STARTS — places, or joins — declares in its start body:
/// sequence 0, and how much of itself the endpoint shows in a report.
///
/// Spelled once, because it is read twice: the place and join bodies send it, and it is what a
/// call they start publishes its mute ON TOP OF. The client's `startOrJoinCall` stores the
/// options' `endpointState` as `latestEndpointState`
/// (`e.endpointState&&(this.latestEndpointState=e.endpointState)`), and both start shapes build
/// those options with `configureEndpointStateForStartOrJoin`, which returns this object whenever
/// `additionalEndpointProperties` is given — so the client's first mute publish re-sends the
/// `endpointProperties` too. An answered INCOMING call never goes through `startOrJoinCall` and
/// starts from nothing. The captured value is the only one this app has seen a service accept.
pub fn start_endpoint_state() -> Value {
    json!({
        "endpointStateSequenceNumber": 0,
        "endpointProperties": {
            "additionalEndpointProperties": { "infoShownInReportMode": "FullInformation" }
        }
    })
}

/// The `endpointState` the next mute publish carries: the last one this leg published, its mute
/// replaced, and the next sequence number.
///
/// It is the client's own `m={...E,...a,endpointStateSequenceNumber:++this.endpointStateSequenceNumber}`
/// (`updateEndpointState`), where `E` is `latestEndpointState`: for a call this app placed or
/// joined, the start body's own [`start_endpoint_state`], and for an answered incoming call
/// nothing. The spread is SHALLOW: `state` is replaced whole by `{isMuted}`, and every other key
/// the last publish held — the start body's `endpointProperties` included — travels again.
pub fn next_endpoint_state(
    previous: &serde_json::Map<String, Value>,
    muted: bool,
    sequence: u64,
) -> serde_json::Map<String, Value> {
    let mut next = previous.clone();
    next.insert("state".into(), json!({ "isMuted": muted }));
    next.insert("endpointStateSequenceNumber".into(), json!(sequence));
    next
}

/// Whether an `endpointState` says muted — the client's own reading
/// (`shouldUpdateEndpointState`: `o.state&&o.state.isMuted?1:0`), so an endpoint state that says
/// nothing about it reads as NOT muted. That is what makes the first Unmute of a call, before
/// any Mute, "nothing new to send".
pub fn endpoint_state_is_muted(state: &serde_json::Map<String, Value>) -> bool {
    state.get("state").and_then(|s| s.get("isMuted")).is_some_and(|m| m.as_bool().unwrap_or(false))
}

/// Build the body that publishes the user's own mute: the client's `uG` for a mute, which is
/// `{from, endpointState}` and nothing else (no `captchaContent`, no `publishedStates`, no async
/// links — each of those is a different request the client folds into the same builder).
///
/// It goes to the conversation's `updateEndpointState` (see [`ConversationLeg`]). It publishes
/// the one fact the user's own press decided — whether they can be heard — and the meeting draws
/// it as the crossed-out microphone beside their name.
pub fn endpoint_state_payload(
    local: &LocalParticipant,
    endpoint_state: &serde_json::Map<String, Value>,
) -> Value {
    json!({
        "from": local.json(),
        "endpointState": endpoint_state,
    })
}

/// The top-level key names of a frame's body — what a frame this app does not act on is
/// journaled as, so the first one that arrives says its shape and nothing it carried.
///
/// It is the reading for a `transfer` and a `replacement`: accepting either would ring
/// somebody with no click behind it, so this app does neither
/// (docs/calling-roadmap/advanced-call-control.md), and what the service sends there has never
/// been seen. `_decoded`'s keys are named in place of the wrapper's.
pub fn body_key_names(body: &Value) -> Vec<String> {
    body.get("_decoded")
        .filter(|v| v.is_object())
        .unwrap_or(body)
        .as_object()
        .map(sorted_keys)
        .unwrap_or_default()
}

/// An object's keys, SORTED — a journal line that names a shape must read the same for the same
/// shape whatever order the service wrote it in, or the once-per-shape dedup says it twice.
fn sorted_keys(map: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

/// Build the ONE POST that places a call (NATIVE-CALLING.md § 2.3).
///
/// `to` is one mri per person to ring; `thread_id` is the chat the call belongs to,
/// so the call shows up in that thread for everybody in it. Audio only.
///
/// **Several recipients is a GROUP call**, and it is the same POST: the service rings every
/// mri in `to`, the thread they share is the conversation the call belongs to, and
/// `properties.enableGroupCallEventMessages` — already here, because the capture carried it
/// — is what posts the call line into that thread for all of them. So the shape a group
/// chat's call button needs is the shape a 1:1 already sends; what a group adds is the
/// roster the answer starts reporting, which [`CallSession::others`] already reads.
/// The callbacks a conversation registers for itself, in ONE spelling for the call that
/// starts one and the join that walks into one.
///
/// It is shared because a link missing from one of the two is INVISIBLE: both requests are
/// accepted, the conversation works, and only the action whose answer had nowhere to land
/// silently never happens. That is exactly what shipped — `join_payload` published no
/// `addModalitySuccess`, so a screen share in a MEETING asked for a presenter session, the
/// POST answered `{}` as it always does, and the grant frame could never arrive because the
/// conversation had never named a URL to deliver it to. The section was then rejected for
/// coming from an endpoint that is not the presenter, which sent two rounds of debugging at
/// the SDP (§ 10.8). A CALL published the link all along, which is why the same feature was
/// measured working there in August.
///
/// Every name here is one the captured web client sends (NATIVE-CALLING.md § 2.3).
fn conversation_callback_links(callbacks: &CallbackBase) -> Value {
    json!({
        "conversationEnd": callbacks.link(paths::CONVERSATION_END),
        "conversationUpdate": callbacks.link(paths::CONVERSATION_UPDATE),
        "localParticipantUpdate": callbacks.link(paths::CONVERSATION_LOCAL_PARTICIPANT_UPDATE),
        "addParticipantSuccess": callbacks.link(paths::CONVERSATION_ADD_PARTICIPANT_SUCCESS),
        "addParticipantFailure": callbacks.link(paths::CONVERSATION_ADD_PARTICIPANT_FAILURE),
        // Where the answer to a `contentSharing` ask lands. Without it a screen share can
        // never be granted, and nothing about the refusal says so.
        "addModalitySuccess": callbacks.link(paths::CONVERSATION_ADD_MODALITY_SUCCESS),
        "addModalityFailure": callbacks.link(paths::CONVERSATION_ADD_MODALITY_FAILURE),
        "confirmUnmute": callbacks.link(paths::CONVERSATION_CONFIRM_UNMUTE),
        "receiveMessage": callbacks.link(paths::CONVERSATION_RECEIVE_MESSAGE),
    })
}

pub fn invitation_payload(
    local: &LocalParticipant,
    to: &[String],
    thread_id: Option<&str>,
    offer: &MediaContent,
    callbacks: &CallbackBase,
) -> Value {
    let recipients: Vec<Value> = to.iter().map(|id| json!({ "id": id })).collect();
    json!({
        "conversationRequest": {
            // "Delta" is what the web client sends: send me roster changes, not
            // the whole roster every time.
            "roster": {
                "type": "Delta",
                "rosterUpdate": callbacks.link(paths::CONVERSATION_ROSTER_UPDATE),
            },
            "properties": {
                "enableGroupCallEventMessages": true,
            },
            "links": conversation_callback_links(callbacks),
        },
        "participants": { "from": local.json(), "to": recipients },
        // Everything on the next few lines is here because the JOIN taught it, and a call
        // goes to the same endpoint: the conversation service refuses a request it does
        // not recognise with `400` and an empty body, naming no field, so the only values
        // this app has ever seen accepted are a working client's. A 1:1 has never been
        // rung (NATIVE-CALLING.md § 8) — sending it the shape that IS known to work is
        // the difference between one round of debugging and five.
        "subject": Value::Null,
        "groupContext": Value::Null,
        "capabilities": Value::Null,
        "endpointCapabilities": CAPTURED_ENDPOINT_CAPABILITIES,
        "clientEndpointCapabilities": CAPTURED_CLIENT_CAPABILITIES,
        "endpointMetadata": { "holographicCapabilities": 3 },
        "endpointState": start_endpoint_state(),
        "debugContent": { "causeId": callbacks.cause_id },
        // The thread the call belongs to. Without it the call exists but belongs
        // to no conversation, and nobody in the thread sees that it happened.
        "groupChat": thread_id.map(|t| json!({ "threadId": t, "messageId": null })),
        "callInvitation": {
            "callModalities": [MODALITY_AUDIO],
            "links": {
                "progress": callbacks.link(paths::CALL_PROGRESS),
                "mediaAnswer": callbacks.link(paths::CALL_MEDIA_ANSWER),
                "acceptance": callbacks.link(paths::CALL_ACCEPTANCE),
                "redirection": callbacks.link(paths::CALL_REDIRECTION),
                "end": callbacks.link(paths::CALL_END),
            },
            // Where the media controller may reach us. See [`media_controller_content`].
            "clientContentForMediaController": media_controller_content(callbacks),
            "mediaContent": offer.json(),
        },
    })
}

/// What TAKING a call says, whichever door it is posted through.
///
/// Two doors carry the same statement — "I am this participant, I accept audio, here is my
/// answer, and here is where to reach me for the rest of the call" — so it is built once.
/// `acceptance_payload` names it `callAcceptance` and `attach_payload` names it `attach`; a
/// second copy of the fields would drift from the first the day either door gains one.
fn taking_the_call(
    local: &LocalParticipant,
    answer: &MediaContent,
    callbacks: &CallbackBase,
) -> Value {
    json!({
        "sender": local.json(),
        "acceptedCallModalities": [MODALITY_AUDIO],
        "links": {
            "mediaRenegotiation": callbacks.link(paths::CALL_MEDIA_RENEGOTIATION),
            "transfer": callbacks.link(paths::CALL_TRANSFER),
            "replacement": callbacks.link(paths::CALL_REPLACEMENT),
            "end": callbacks.link(paths::CALL_END),
        },
        // Where the media controller may reach us. See [`media_controller_content`].
        "clientContentForMediaController": media_controller_content(callbacks),
        "mediaContent": answer.json(),
    })
}

/// Build the body that ATTACHES this endpoint to a forked incoming call — which is how a
/// call is taken on this tenant.
///
/// A one-to-one invite here is addressed to the USER (`to.endpointId` is all zeroes) and
/// forked to every endpoint they are signed in on, so it offers `attach` and neither
/// `accept` nor `acceptance`. Measured 2026-09-04: posting the ACCEPTANCE body to that link
/// is refused `400` with the field named —
/// `{"errors":{"Attach":["The Attach field is required."]}}` — so the door is right and only
/// the envelope was wrong. The service names the property `Attach`; it binds case
/// insensitively, and the lowercase spelling is the one every other body here uses.
pub fn attach_payload(
    local: &LocalParticipant,
    answer: &MediaContent,
    callbacks: &CallbackBase,
) -> Value {
    json!({ "attach": taking_the_call(local, answer, callbacks) })
}

/// The capability mask an ATTACH declares: the captured one with bit 16
/// (`brokerEnabledIncoming`) CLEARED — 73447.
///
/// The one computed mask in this module, and a stated exception to the rule above it
/// (docs/calling-roadmap/incoming-and-conversation.md § 4.2, divergence 1). The client sets bit 16
/// only when its broker channel is on, and follows the attach with a broker subscription; this app
/// implements no broker channel, so it must not claim one on the incoming path. The measured
/// legacy attach declared no capabilities at all.
const ATTACH_ENDPOINT_CAPABILITIES: u32 = CAPTURED_ENDPOINT_CAPABILITIES & !16;

/// Build the body that ATTACHES this endpoint to a forked incoming call and, in the same request,
/// JOINS the call's conversation — the client's `WV` (docs/calling-roadmap/incoming-and-
/// conversation.md § 4.1 and § 4.2).
///
/// It carries no SDP, no `sender` and no `acceptedCallModalities`: claiming the leg and joining
/// the conversation is all it does, and the acceptance after it (unchanged, [`acceptance_payload`])
/// is what answers the caller and carries our media. The join is an `additionalActions` entry the
/// service runs against the conversation's controller — the bare `conversationController` the
/// invite named — and its answer, `additionalActionResponses[0].output`, is the conversation
/// object whose doors the call has never had on the measured flow: `leave`, and
/// `updateEndpointState`.
///
/// `require_media` is the client's `requireMediaContent`: true only when the invite carried no
/// offer. At answer time it is always false, because a ring with no offer is not answerable here.
///
/// Two stated divergences from `WV`: bit 16 is cleared (see [`ATTACH_ENDPOINT_CAPABILITIES`]), and
/// the join publishes the same nine conversation callbacks the place and join bodies publish
/// (`WV` names six) — a link missing from one body fails invisibly, which is why they are shared.
pub fn attach_join_payload(
    local: &LocalParticipant,
    controller: &str,
    callbacks: &CallbackBase,
    require_media: bool,
) -> Value {
    json!({
        "attach": {
            "requireMediaContent": require_media,
            "links": { "end": callbacks.link(paths::CALL_END) },
            "locationContent": null,
            "networkContent": null,
            "areaContent": null,
        },
        "capabilities": null,
        "endpointCapabilities": ATTACH_ENDPOINT_CAPABILITIES,
        "additionalActions": [{
            "input": {
                "capabilities": null,
                "endpointCapabilities": ATTACH_ENDPOINT_CAPABILITIES,
                "conversationRequest": {
                    "roster": {
                        "type": "Delta",
                        "rosterUpdate": callbacks.link(paths::CONVERSATION_ROSTER_UPDATE),
                    },
                    "links": conversation_callback_links(callbacks),
                },
                "endpointMetadata": { "holographicCapabilities": 3 },
                "participants": { "from": local.json() },
            },
            "name": "join",
            "url": controller,
            "waitForResponse": true,
        }],
    })
}

/// Build the body that accepts an incoming call: we take it, and we publish the
/// links the service may use for the rest of the call.
pub fn acceptance_payload(
    local: &LocalParticipant,
    answer: &MediaContent,
    callbacks: &CallbackBase,
) -> Value {
    json!({ "callAcceptance": taking_the_call(local, answer, callbacks) })
}

/// Build the body that answers a media offer on its own (a renegotiation, or an
/// acceptance the service asked us to split in two).
/// `modalities` is what the answer DECLARES, and it is the caller's because only the caller
/// knows what the answer really carries: audio alone for the original handshake, and audio
/// plus `ScreenViewer` for an answer that accepts a shared screen. A declaration wider than
/// the SDP is a claim about the user's camera that the SDP does not back.
pub fn media_answer_payload(
    local: &LocalParticipant,
    answer: &MediaContent,
    callbacks: &CallbackBase,
    modalities: &[&str],
) -> Value {
    json!({
        "mediaAnswer": {
            "sender": local.json(),
            "callModalities": modalities,
            "links": {
                "mediaAcknowledgement": callbacks.link(paths::CALL_MEDIA_ACKNOWLEDGEMENT),
            },
            // Where the media controller may reach us — the client's `jH` sends it here too, and
            // this is the body a RECEIVE section is negotiated by. See
            // [`media_controller_content`].
            "clientContentForMediaController": media_controller_content(callbacks),
            "mediaContent": answer.json(),
        }
    })
}

/// Build the body that offers NEW media on a call that is already up.
///
/// This is the outgoing twin of [`media_renegotiation_from_frame`], and it is how a camera or
/// a shared screen goes out: the call was negotiated with audio alone, so adding either means
/// offering a section that does not exist yet. The client's own builder is the same shape, and
/// the service refuses one on a call that is not established —
/// `"media renegotiation can only be performed on an established call"`.
///
/// `modalities` is what the offer DECLARES, and it must match what the SDP carries:
/// `ScreenSharer` for a screen, `Video` for a camera. The service reads the words, so a
/// declaration wider than the blob is a claim about the user's machine that nothing backs.
pub fn media_offer_payload(
    local: &LocalParticipant,
    offer: &MediaContent,
    callbacks: &CallbackBase,
    modalities: &[&str],
) -> Value {
    json!({ "mediaNegotiation": offering_media(local, offer, callbacks, modalities) })
}

/// OFFERING media, whichever door it is posted through.
///
/// Two doors carry the same statement — I am this participant, this is what the offer declares,
/// here is the blob, and here is where to answer or refuse it. `media_offer_payload` names it
/// `mediaNegotiation` and [`start_outgoing_negotiation_payload`] names it
/// `startOutgoingNegotiation`; built once, so the two cannot drift the day either gains a field.
fn offering_media(
    local: &LocalParticipant,
    offer: &MediaContent,
    callbacks: &CallbackBase,
    modalities: &[&str],
) -> Value {
    json!({
        "sender": local.json(),
        "callModalities": modalities,
        "links": {
            // Where the service answers this offer, and where it may refuse it. Both are
            // ours: an offer nobody can answer is an offer that hangs.
            //
            // **TWO LINKS, and `mediaAcknowledgement` is NOT one of them.** Read out of the real
            // client's own builder for this body (`qH` in `calling-pluginless-<hash>.js`, fetched
            // by NATIVE-CALLING.md § 9's recipe): it sends `mediaAnswer` and `rejection` and
            // nothing else here. The acknowledgement belongs to the ANSWER body (`jH`), which is
            // where this app already sends it — an offer is acknowledged by nobody, so publishing
            // a callback for one is a field the door was never told about. That is the class of
            // defect this plane has been bitten by twice: the SDK's `payload` envelope earned a
            // `400 {}` for days (§ 2.3), and the outgoing door's own missing link earned two more
            // — and a body the service merely does not recognise is answered by a section it
            // silently zeroes rather than by a refusal that names anything.
            "mediaAnswer": callbacks.link(paths::CALL_MEDIA_ANSWER),
            "rejection": callbacks.link(paths::CALL_MEDIA_REJECTION),
        },
        "mediaContent": offer.json(),
    })
}

/// Build the body that STARTS a negotiation this endpoint wants, on the door named for it.
///
/// The acceptance names `startOutgoingNegotiation` beside `mediaRenegotiation` and nothing had
/// ever posted to it, while every video section offered on the second came back with a zeroed
/// port — a camera as much as a screen (§ 10.8). Measured 2026-09-04, the first POST to it was
/// refused with the field named:
/// `400 {"errors":{"StartOutgoingNegotiation":["The StartOutgoingNegotiation field is required."]}}`
/// — the same shape the incoming call's `attach` door wanted (§ 8a), so the door is real and
/// only the envelope was wrong.
pub fn start_outgoing_negotiation_payload(
    local: &LocalParticipant,
    offer: &MediaContent,
    callbacks: &CallbackBase,
    modalities: &[&str],
) -> Value {
    let mut body = offering_media(local, offer, callbacks, modalities);
    // This door requires one link the other does not, and the service named it:
    // `400 {"errors":{"StartOutgoingNegotiation.Links.MediaRenegotiation":["The
    // MediaRenegotiation field is required."]}}`. It is where the service renegotiates what this
    // offer started, so it is added HERE rather than to the shared statement — the
    // `mediaNegotiation` door has never asked for it, and a body carrying a field one door does
    // not know is how this plane earns a `400` that names nothing.
    if let Some(links) = body.get_mut("links").and_then(Value::as_object_mut) {
        links.insert(
            "mediaRenegotiation".to_string(),
            Value::String(callbacks.link(paths::CALL_MEDIA_RENEGOTIATION)),
        );
    }
    json!({ "startOutgoingNegotiation": body })
}

/// How long an offer of ours may be in flight before this side stops treating it as pending.
///
/// **35 seconds, and it is the real client's own `mediaAnswerTimeoutSec: 35`** — the timer its
/// `startRenegotiationAsync` starts and its `handleMediaAnswer` stops. The page carries the same
/// number for its own half (`MEDIA_ANSWER_TIMEOUT_MS` in web/src/lib/store.ts), and the two are
/// pinned together by a test: this one decides how long an incoming renegotiation is REFUSED for,
/// so a window that outlived the page's would refuse every later offer for the rest of the call and
/// take the receive path down with it.
pub const OUTGOING_NEGOTIATION_TIMEOUT_MS: i64 = 35_000;

/// Refuse a renegotiation offer that arrived while one of OURS is still in flight — the client's
/// own `getGlareError`, posted to the offer's own `rejection` link.
///
/// **GLARE is a named state on this plane, not an edge case.** The real client's
/// `MEDIA_RENEGOTIATION_FSM_STATE` holds `RENEGOTIATION_GLARE` beside `OUTGOING_RENEGOTIATION`, and
/// its `rejectAndLog` posts exactly this when it cannot handle an incoming offer. This app used to
/// APPLY one, and applying a remote offer in `have-local-offer` makes the browser roll our own
/// offer back — which loses the camera the user just turned on, in silence (measured in a real
/// browser by web/e2e/webrtc-glare.spec.ts).
///
/// The client can afford to apply one and this app cannot: it is REINVITELESS, so every section
/// exists from the first offer and a rollback reverts a `direction` it re-applies, while this app
/// ADDS a section mid-call and a rollback removes the section itself.
///
/// The body is the client's `zH`: a `mediaNegotiationFailure` naming who is refusing and why. It is
/// the same shape the service POSTS to us on the `mediaNegotiationFailure` callback, which is what
/// makes the pair readable in one direction as well as the other.
pub fn media_glare_rejection_payload(local: &LocalParticipant) -> Value {
    json!({
        "mediaNegotiationFailure": {
            "sender": local.json(),
            // The client's own three, verbatim: `CALL_END_CODE.GLARE_ERROR`,
            // `CALL_END_SUB_CODE.MEDIA_GLARE_ERROR`, `CALL_END_PHRASE.RENEGOTIATION_IN_PROGRESS`.
            // A code the service does not recognise is how this plane earns a refusal that names
            // nothing, so nothing here is invented.
            "code": GLARE_ERROR_CODE,
            "subCode": MEDIA_GLARE_ERROR_SUB_CODE,
            "phrase": RENEGOTIATION_IN_PROGRESS_PHRASE,
        }
    })
}

/// `CALL_END_CODE.GLARE_ERROR` — the client's own code for "I cannot take your offer, I have one
/// out". Read out of its `CALL_END_CODE` table; 491 is SIP's `Request Pending`, which is what glare
/// is.
pub const GLARE_ERROR_CODE: i64 = 491;
/// `CALL_END_SUB_CODE.MEDIA_GLARE_ERROR`, read out of the client's own table. It sits in the
/// `31xx` media block beside `MEDIA_RENEGOTIATION_ERROR: 3108`, and it is a value rather than a
/// shape — so it is COPIED and never derived.
pub const MEDIA_GLARE_ERROR_SUB_CODE: i64 = 3118;
/// `CALL_END_PHRASE.RENEGOTIATION_IN_PROGRESS`, and the spelling is the client's own rather than
/// the constant's: `"NegotiationIsInProgress"`. Guessing it from the name gives
/// `RenegotiationInProgress`, which is a different string — this table is the only authority for it.
pub const RENEGOTIATION_IN_PROGRESS_PHRASE: &str = "NegotiationIsInProgress";

/// Build the body that ends our leg of a call.
pub fn hangup_payload(local: &LocalParticipant) -> Value {
    json!({
        // WHO is leaving. The service refuses a body that names nobody with a `400`, and
        // then the user is out of the call HERE while Teams still has them in it — a
        // phantom participant everybody else can see. Measured: our own `callEnd` shape
        // was refused on every `leave` of a joined meeting.
        "participants": { "from": local.json() },
        // Why the CONVERSATION ended for us, and why the CALL leg did. The client sends
        // both, and both are the ordinary ending: nothing failed, the user left.
        "conversationTransactionEnd": {
            "reason": "noError",
            "code": 0,
            "phrase": "ConversationEndedNoModality",
        },
        "callTransactionEnd": {
            "code": 0,
            "phrase": "LocalUserInitiated",
        },
    })
}

/// Build the body that refuses an incoming call.
pub fn rejection_payload(local: &LocalParticipant) -> Value {
    json!({
        "callRejection": {
            "sender": local.json(),
            "code": 603,
            "phrase": "Decline",
        }
    })
}

/// Build the body that mutes or unmutes our own microphone SERVER-side.
///
/// The browser stops sending audio on its own (the track is disabled), and this is
/// the other half: it tells everybody else in the call that we are muted, which is
/// what draws the crossed-out microphone next to our name in their client.
pub fn mute_payload(local: &LocalParticipant, muted: bool) -> Value {
    json!({
        "muteUnmute": {
            "sender": local.json(),
            "muted": muted,
            "scope": "Myself",
        }
    })
}

/// Everything a meeting join needs, read out of the link the calendar already holds.
///
/// A Teams join link carries all of it (`onlineMeeting.joinUrl` from Graph):
///
/// ```text
/// https://teams.microsoft.com/l/meetup-join/{threadId}/{messageId}?context={"Tid":…,"Oid":…}
/// ```
///
/// The thread is the meeting's own conversation, the message id is `0` for a meeting
/// from the calendar and a real id for a channel meeting, and the context names the
/// tenant and the organizer — which is exactly the `meetingInfo` the calling service
/// asks for. So joining needs no new service: the link the user could click is the
/// whole address. `examples/meeting_join_recon.rs` checks this parse against the
/// user's own real meetings, READ-ONLY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingJoin {
    /// The meeting's thread (`19:meeting_…@thread.v2`, or a channel's own thread).
    /// Absent on a SHORT link, which names a meeting code instead of a thread.
    pub thread_id: Option<String>,
    /// The message the meeting hangs off. `"0"` for a calendar meeting.
    pub message_id: String,
    /// The tenant the meeting belongs to, from the link's context.
    pub tenant_id: Option<String>,
    /// The organizer's object id, as the link states it. The service's `meetingInfo`
    /// wants exactly that — a bare oid, NOT an mri (captured from the real client).
    pub organizer_mri: Option<String>,
    /// The meeting code, from a short link (`/meet/{code}`). Teams' newer meetings are
    /// addressed this way, and the service takes it as `meetingData.meetingCode`.
    pub meeting_code: Option<String>,
    /// The passcode that goes with that code (`?p=…`).
    pub passcode: Option<String>,
    /// The link itself, which the service wants back as `meetingData.meetingUrl`.
    pub join_url: String,
}

impl MeetingJoin {
    /// Read a join link, in either shape Teams writes one.
    ///
    /// `None` when it is neither — a link to something else, or a shape this does not
    /// know, which is a thing to add rather than to guess at.
    pub fn from_join_url(url: &str) -> Option<Self> {
        let (base, query) = match url.split_once('?') {
            Some((base, query)) => (base, Some(query)),
            None => (url, None),
        };
        // The SHORT shape, which Teams' newer meetings use: `…/meet/{code}?p={passcode}`.
        // It names no thread at all, so the service is given the code and the passcode
        // instead (`meetingData`) — and this is the shape the user's own meetings have.
        if let Some(after) = base.split("/meet/").nth(1) {
            let code = decode(after.split('/').next().unwrap_or_default());
            if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            return Some(Self {
                thread_id: None,
                message_id: "0".into(),
                tenant_id: None,
                organizer_mri: None,
                meeting_code: Some(code),
                passcode: query
                    .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("p=")))
                    .map(decode)
                    .filter(|p| !p.is_empty()),
                join_url: url.to_string(),
            });
        }
        // The long shape: `…/l/meetup-join/{thread}/{message}?context={…}`.
        let after = base.split("/meetup-join/").nth(1)?;
        let mut segments = after.split('/').filter(|s| !s.is_empty());
        let thread_id = decode(segments.next()?);
        if !thread_id.starts_with("19:") {
            return None;
        }
        let message_id = segments.next().map(decode).unwrap_or_else(|| "0".into());

        // The context is a JSON object, URL-encoded, and its keys are capitalised the
        // way Teams writes them. A link without one still joins: the thread is what
        // addresses the meeting, and `meetingInfo` is what the service prefers.
        let context = query
            .and_then(|q| {
                q.split('&').find_map(|pair| pair.strip_prefix("context=").map(decode))
            })
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
        let field = |name: &str| {
            context
                .as_ref()
                .and_then(|c| c.get(name))
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            thread_id: Some(thread_id),
            message_id,
            tenant_id: field("Tid"),
            // The BARE object id, exactly as the link states it: the captured
            // `meetingInfo.organizerId` is an oid, not an mri.
            organizer_mri: field("Oid"),
            meeting_code: None,
            passcode: None,
            join_url: url.to_string(),
        })
    }

    /// The address of a meeting reached from the CHAT it already has here, rather than
    /// from a calendar link.
    ///
    /// Teams mints one thread per meeting and puts it in the chat list, so the meeting the
    /// user is looking at is addressable without finding a link at all: the thread IS what
    /// a long join link carries in its own first segment, and the `meetingInfo` beside it is
    /// what the service PREFERS rather than what it requires (a long link with no context
    /// joins by its thread — see `a_meeting_link_with_no_context_still_joins_by_its_thread`).
    /// That matters on this tenant, whose own invitations carry the SHORT link shape: the
    /// code lives in the calendar event and nowhere in the conversation, so a chat with no
    /// matching event could otherwise offer nothing.
    ///
    /// `None` for anything but a meeting thread. A plain group chat has no meeting to join —
    /// it is CALLED instead — and a channel meeting hangs off a message id a thread alone
    /// cannot name, so guessing `"0"` there would address the channel and not the meeting
    /// inside it.
    pub fn from_thread_id(thread_id: &str) -> Option<Self> {
        if !thread_id.starts_with("19:meeting_") {
            return None;
        }
        Some(Self {
            thread_id: Some(thread_id.to_string()),
            // The same string the captured request sends for a meeting that hangs off no
            // message, and a calendar meeting's own long link carries.
            message_id: "0".into(),
            tenant_id: None,
            organizer_mri: None,
            meeting_code: None,
            passcode: None,
            // No link was involved, and nothing may invent one: `meetingData.meetingUrl` is
            // only ever the url the user's own invitation carried.
            join_url: String::new(),
        })
    }

    /// True for a meeting that lives in a CHANNEL rather than on the calendar alone.
    /// Its thread is the channel's, and the message id is the post the meeting hangs
    /// off — so both have to travel, or the join addresses the channel instead of the
    /// meeting inside it.
    pub fn is_channel_meeting(&self) -> bool {
        self.thread_id.as_ref().is_some_and(|thread| !thread.starts_with("19:meeting_"))
            && self.message_id != "0"
    }

    /// What the service is told about the meeting when the link named a code rather
    /// than a thread (`meetingData` — `{meetingCode, passcode, meetingUrl}`, the shape
    /// the client's own conversation query reads).
    fn meeting_data(&self) -> Option<Value> {
        let code = self.meeting_code.as_ref()?;
        let mut data = json!({ "meetingCode": code, "meetingUrl": self.join_url });
        if let Some(passcode) = &self.passcode {
            data["passcode"] = json!(passcode);
        }
        Some(data)
    }

    /// The `meetingInfo` object the service wants, or `None` when the link named no
    /// context. Only the two fields the client sends are included; a meeting type or
    /// a reply chain would be added here, not guessed.
    fn meeting_info(&self) -> Option<Value> {
        let tenant = self.tenant_id.as_ref()?;
        let organizer = self.organizer_mri.as_ref()?;
        Some(json!({ "tenantId": tenant, "organizerId": organizer }))
    }
}

/// Percent-decode one URL piece, leaving it alone when it is not encoded.
fn decode(value: &str) -> String {
    urlencoding::decode(value).map(|v| v.into_owned()).unwrap_or_else(|_| value.to_string())
}

/// Build the ONE POST that joins a meeting, with or without a microphone.
///
/// The fields are the captured request of the real web client (2026-08-05, this tenant,
/// this user's own meeting), and `offer` is the one thing the capture did not show,
/// because it was taken on the pre-join screen. The client's own builder settles it: it
/// composes the conversation request and then adds EITHER `stream: {}` (subscribe to the
/// roster and nothing more) OR `callInvitation` with the media — one body, one POST,
/// whichever the user asked for.
///
/// `addModality` is NOT that second request, and reading it as one cost a round: it is
/// how a 1:1 call grows a group modality (`addModalityAsync`, whose body carries no media
/// at all), so the service answers `subCode 5021 — no modality blob in the request`.
///
/// Everything below is the captured shape, field for field:
///
/// * `messageId` is the STRING `"0"` for a meeting on the calendar, not null.
/// * `meetingInfo.organizerId` is the BARE object id, not an mri.
/// * `properties.allowConversationWithoutHost` is true — a scheduled meeting is hostless.
/// * `capabilities` is null, and the two capability masks are the working client's own
///   (see `CAPTURED_ENDPOINT_CAPABILITIES`).
pub fn join_payload(
    local: &LocalParticipant,
    meeting: &MeetingJoin,
    callbacks: &CallbackBase,
    offer: Option<&MediaContent>,
) -> Value {
    let mut payload = json!({
        "conversationRequest": {
            "subject": Value::Null,
            "roster": {
                "type": "Delta",
                "rosterUpdate": callbacks.link(paths::CONVERSATION_ROSTER_UPDATE),
            },
            "properties": {
                "allowConversationWithoutHost": true,
                "enableGroupCallEventMessages": true,
                "enableGroupCallUpgradeMessage": false,
                "enableGroupCallMeetupGeneration": false,
            },
            "links": conversation_callback_links(callbacks),
        },
        "groupContext": Value::Null,
        "participants": { "from": local.json() },
        "capabilities": Value::Null,
        // The capability masks the captured request carries, copied rather than computed.
        //
        // They are DECLARATIONS of client features, and the service refuses a request it
        // does not recognise without saying which field it disliked — so the only values
        // this app has ever seen accepted are the ones a working client sent. The bits
        // this crate can name (`getEndpointCapabilities` in the web client's own bundle)
        // are all ones it can honour: joining a group call, a hostless conference, and a
        // compressed service payload, which `trouter_events` already inflates.
        "endpointCapabilities": CAPTURED_ENDPOINT_CAPABILITIES,
        "clientEndpointCapabilities": CAPTURED_CLIENT_CAPABILITIES,
        "endpointMetadata": { "holographicCapabilities": 3 },
        // The client states how much of itself it shows in a report; the captured value
        // is the only one this app has seen a service accept.
        "endpointState": start_endpoint_state(),
        "debugContent": { "causeId": callbacks.cause_id },
    });
    // How the meeting is addressed, and it is one of two ways: a long link names the
    // thread, a short one names a code the service resolves itself.
    if let Some(thread) = &meeting.thread_id {
        payload["groupChat"] = json!({
            "threadId": thread,
            // A string, and "0" when the meeting hangs off no message. The captured
            // request sends "0" rather than null.
            "messageId": if meeting.message_id.is_empty() { "0" } else { &meeting.message_id },
        });
    }
    if let Some(data) = meeting.meeting_data() {
        payload["meetingData"] = data;
    }
    if let Some(info) = meeting.meeting_info() {
        payload["meetingInfo"] = info;
    }
    // The MEDIA, when this join is the one that opens a microphone. It rides in a
    // `callInvitation` — the same envelope a call's own offer travels in, minus
    // `participants.to`, because a join rings nobody.
    if let Some(offer) = offer {
        payload["callInvitation"] = json!({
            "callModalities": [MODALITY_AUDIO],
            "links": {
                "progress": callbacks.link(paths::CALL_PROGRESS),
                "mediaAnswer": callbacks.link(paths::CALL_MEDIA_ANSWER),
                "acceptance": callbacks.link(paths::CALL_ACCEPTANCE),
                "redirection": callbacks.link(paths::CALL_REDIRECTION),
                "end": callbacks.link(paths::CALL_END),
            },
            // Where the media controller may reach us. See [`media_controller_content`].
            "clientContentForMediaController": media_controller_content(callbacks),
            "mediaContent": offer.json(),
        });
    }
    payload
}

/// The conversation a join landed in: where it lives, and what may be done to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedConversation {
    /// The conversation's own URL — every later action on the MEETING (rather than on
    /// one call leg) is addressed under it.
    pub controller: Option<String>,
    /// Every link the answer named: `leave`, `addModality`, `mute`, `unmute`, `admit`, …
    pub links: Links,
    /// What the service says this conversation is: `"scheduledMeeting"`, hostless or
    /// not, multi-party or not.
    pub state: Value,
    /// The conversation's own doors, replaced rather than merged (see [`ConversationLeg`]).
    /// `links` above is the merged table and is unchanged by it.
    pub conversation: Option<ConversationLeg>,
    /// The whole answer, for the journal while the schema is still young.
    pub raw: Value,
}

/// Join a meeting: ONE POST, and it carries the microphone when there is one.
///
/// `offer` absent joins for the roster alone, which is what a pre-join screen does.
pub async fn join_meeting(
    http: &reqwest::Client,
    session: &Session,
    ic3: &str,
    local: &LocalParticipant,
    meeting: &MeetingJoin,
    callbacks: &CallbackBase,
    correlation_id: &str,
    offer: Option<&MediaContent>,
) -> Result<JoinedConversation> {
    let endpoints = endpoints(session)?;
    let payload = join_payload(local, meeting, callbacks, offer);
    let raw = post_signal(
        http,
        SignalClass::Create,
        &endpoints.conversation_service,
        session,
        ic3,
        correlation_id,
        &payload,
    )
    .await?;
    Ok(JoinedConversation {
        controller: raw.get("conversationController").and_then(Value::as_str).map(String::from),
        links: Links::collect(&raw),
        state: raw.get("state").cloned().unwrap_or(Value::Null),
        conversation: ConversationLeg::from_response(&raw, &callbacks.surl),
        raw,
    })
}


/// Where a join has landed: in the meeting, or in its lobby waiting to be let in.
///
/// The lobby is not a failure and not a connection. Teams reports it as a state of its
/// own (`ConnectedForRosterOnly`), the user is told nobody has admitted them yet, and
/// the same call continues when somebody does — so it must be visible rather than
/// hidden behind "connecting…".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyState {
    /// In the meeting.
    Admitted,
    /// Waiting for somebody in the meeting to admit us.
    Waiting,
}

/// Read the lobby state out of a frame, or `None` when it says nothing about it.
///
/// Two spellings, because the service states it two ways: the call's own status, and a
/// participant's state in the roster.
pub fn lobby_state_in_frame(frame: &Value) -> Option<LobbyState> {
    fn state_of(value: &str) -> Option<LobbyState> {
        match value {
            "ConnectedForRosterOnly" | "Lobby" | "InLobby" => Some(LobbyState::Waiting),
            "Connected" => Some(LobbyState::Admitted),
            _ => None,
        }
    }
    fn walk(value: &Value) -> Option<LobbyState> {
        match value {
            Value::Object(map) => {
                for key in ["callState", "state", "participantState", "status"] {
                    if let Some(found) = map.get(key).and_then(Value::as_str).and_then(state_of) {
                        return Some(found);
                    }
                }
                map.values().find_map(walk)
            }
            Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(frame)
}

/// One stream a participant publishes into the meeting.
///
/// **`source_id` is the whole point of this type**: it is the media source id a
/// subscription is addressed by, and the roster is the only place it exists
/// (NATIVE-CALLING.md § 10.2). Measured values are small integers, per meeting, and they
/// move between joins — so one is never cached across calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterStream {
    /// The ENDPOINT this stream is published from — the roster's own key for the device.
    ///
    /// It is what tells this machine's own publishing from everybody else's, and an mri cannot:
    /// one person joined from a laptop and a phone has two endpoints under one mri, so excluding
    /// by person hides a screen their other device is really sharing. Measured 2026-08-06: the
    /// user shared from real Teams on the same account, the section was negotiated and the tile
    /// drawn, and nothing was ever subscribed to because the stream read as ours.
    pub endpoint_id: String,
    /// `main-audio` / `main-video` / `applicationsharing-video` / `data`. The label is the
    /// wire name, and the only thing that tells a shared screen from a camera.
    pub label: String,
    /// What the frame calls it, which FOLLOWS the label rather than the m-line kind: a
    /// shared screen is `applicationsharing-video`, not `video`.
    pub kind: String,
    /// The media source id — what a source request names.
    pub source_id: i64,
    /// That endpoint's own direction, which is what says who is doing what: a sharer's
    /// section is `sendonly`, and a camera nobody turned on is `recvonly`.
    pub direction: String,
    pub server_muted: bool,
}

impl RosterStream {
    /// Whether this stream is a SHARED SCREEN being sent. The client's own test, verbatim
    /// (`N2` in its bundle) — and measured against a real share.
    pub fn is_shared_screen(&self) -> bool {
        self.label == labels::SHARING && self.direction == "sendonly"
    }

    /// Whether this stream is a CAMERA being sent.
    pub fn is_camera(&self) -> bool {
        self.label == labels::VIDEO && self.direction != "recvonly" && self.direction != "inactive"
    }

    pub fn json(&self) -> Value {
        json!({
            "label": self.label,
            "kind": self.kind,
            "source_id": self.source_id,
            "direction": self.direction,
            "server_muted": self.server_muted,
            // Decided HERE so the page never has to know the label vocabulary twice.
            "shared_screen": self.is_shared_screen(),
            "camera": self.is_camera(),
        })
    }
}

/// One person in a meeting, as a roster frame names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterMember {
    pub mri: String,
    pub display_name: String,
    /// True while they are only in the lobby, so a count of who can hear us is honest.
    pub in_lobby: bool,
    /// False when the roster says this person is no longer in the meeting. A delta frame
    /// announces a departure by turning their state, not by omitting them, so a reader
    /// that ignored this would keep naming somebody who left.
    pub present: bool,
    /// Everything they publish, across every endpoint they are joined from. One person on
    /// a laptop and a phone has several, which is why this is flattened rather than kept
    /// per endpoint: a subscription names a source id and nothing else.
    pub streams: Vec<RosterStream>,
}

/// A roster frame's contents: who it names, and whether that is the WHOLE meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterUpdate {
    pub members: Vec<RosterMember>,
    /// True when the frame carries only the participants that CHANGED (`type: "Delta"`),
    /// which is what this app asks for in every join and call payload
    /// (`roster: {type: "Delta"}`). Measured: consecutive frames from one meeting carried
    /// one participant, then two, then one — never the roster whole. So a delta must be
    /// MERGED, and replacing the list with it makes the meeting flicker between the people
    /// in it.
    pub delta: bool,
}

/// Read the roster out of a `rosterUpdate` frame, or `None` when the frame is not one.
///
/// Everybody the frame names, us included: the caller knows its own mri and drops it,
/// and doing that here would make an empty roster and "only me" the same answer.
///
/// **The shape here is measured, and it is not the one the web client's own types
/// describe** (NATIVE-CALLING.md § 10.2). The earlier version of this function read
/// `/rosterUpdate/participants` as an ARRAY, which no real frame has ever been: it returned
/// `None` every time, so a joined meeting named nobody and the call bar said "In the
/// meeting" where it should have said who was in it. Three differences, and every one of
/// them alone was enough to lose the whole roster:
///
/// * the frame BODY *is* the roster — `{type, sequenceNumber, participantCounts,
///   participants}` — because the callback url it arrived on is what names it;
/// * `participants` is an OBJECT keyed by mri, not an array of people carrying an `id`;
/// * the display name is under `details`, and `state` is `"active"` / `"inactive"` rather
///   than the `"Connected"` / `"Lobby"` spellings a call leg uses.
///
/// Every older spelling is still tried first, because they cost one pointer lookup each and
/// this tenant is one tenant.
pub fn roster_in_frame(frame: &Value) -> Option<RosterUpdate> {
    let roster = ["/_decoded/rosterUpdate", "/rosterUpdate"]
        .iter()
        .find_map(|p| frame.pointer(p))
        // The body itself, which is the shape this tenant really sends. It is recognised by
        // holding a `participants` map rather than by its `type`, so a frame that names one
        // is never mistaken for a roster.
        .or_else(|| frame.get("participants").map(|_| frame))?;
    let participants = roster.get("participants")?;
    // Either shape: the object this tenant sends, keyed by mri, or the array the client's
    // own types describe.
    let people: Vec<(Option<&str>, &Value)> = match participants {
        Value::Object(map) => map.iter().map(|(mri, person)| (Some(mri.as_str()), person)).collect(),
        Value::Array(items) => items.iter().map(|person| (None, person)).collect(),
        _ => return None,
    };
    let members: Vec<RosterMember> = people
        .into_iter()
        .filter_map(|(key, person)| {
            let mri = key
                .or_else(|| person.get("id").and_then(Value::as_str))
                .filter(|id| id.starts_with('8') || id.starts_with('4'))?;
            let state = person.get("state").and_then(Value::as_str).unwrap_or_default();
            Some(RosterMember {
                mri: mri.to_string(),
                display_name: display_name_in_roster(person),
                in_lobby: matches!(state, "Lobby" | "InLobby" | "ConnectedForRosterOnly"),
                // An unstated state is presence: the array shape never carried one, and a
                // person the roster names is in the meeting until it says otherwise.
                present: !matches!(state, "inactive" | "Inactive" | "Disconnected"),
                streams: streams_of(person),
            })
        })
        .collect();
    Some(RosterUpdate {
        members,
        delta: roster.get("type").and_then(Value::as_str) == Some("Delta"),
    })
}

/// Everything one participant publishes, from every endpoint they are joined from.
///
/// The nesting is measured: `endpoints` is an OBJECT keyed by endpoint id, and the streams
/// sit under that endpoint's `call`. `endpointDetails[]` — which the web client's own types
/// describe — is its normalized form and never travels, so both are read: one pointer each,
/// and the one that finds nothing costs nothing.
fn streams_of(person: &Value) -> Vec<RosterStream> {
    let endpoints: Vec<(String, &Value)> = match person.get("endpoints") {
        // Keyed by endpoint id, which is the key itself — and the only thing that says which
        // device a stream comes from.
        Some(Value::Object(map)) => map.iter().map(|(id, e)| (id.clone(), e)).collect(),
        // The client's own shape, in case another tenant sends it. No key, so no endpoint: an
        // empty id can never match ours, which keeps such a stream visible rather than hidden.
        Some(Value::Array(items)) => items.iter().map(|e| (String::new(), e)).collect(),
        _ => return Vec::new(),
    };
    endpoints
        .into_iter()
        .flat_map(|(endpoint_id, endpoint)| {
            // `endpointDetails` is itself an array of endpoints in that other shape, so a
            // single level of either is handled without a second walk.
            let under_call = endpoint.pointer("/call/mediaStreams").and_then(Value::as_array);
            let bare = endpoint.get("mediaStreams").and_then(Value::as_array);
            under_call
                .or(bare)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(move |stream| (endpoint_id.clone(), stream))
                .collect::<Vec<_>>()
        })
        .filter_map(|(endpoint_id, stream)| {
            // A stream with no source id cannot be subscribed to, so it is not one this app
            // has any use for.
            let source_id = stream.get("sourceId").and_then(Value::as_i64)?;
            let label = stream.get("label").and_then(Value::as_str).unwrap_or_default();
            Some(RosterStream {
                label: label.to_string(),
                kind: stream
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or(label)
                    .to_string(),
                source_id,
                direction: stream
                    .get("direction")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                server_muted: stream.get("serverMuted").and_then(Value::as_bool).unwrap_or(false),
                endpoint_id,
            })
        })
        .collect()
}

/// A roster participant's name, from wherever that frame keeps it.
///
/// `details.displayName` is what this tenant sends; the bare `displayName` is the client's
/// own normalized form and what the older test invented.
fn display_name_in_roster(person: &Value) -> String {
    ["/details/displayName", "/displayName", "/details/name"]
        .iter()
        .find_map(|p| person.pointer(p))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Fold a roster frame into the list a call holds, and return whether anything moved.
///
/// A delta names only what changed, so it is merged by mri and a departure removes its
/// person; a full frame replaces the list. Either way the order is the order people were
/// first seen, because a roster that re-sorts itself under the reader moves the name they
/// were reading.
pub fn apply_roster_update(current: &mut Vec<RosterMember>, update: RosterUpdate) -> bool {
    let before = current.clone();
    if !update.delta {
        current.clear();
    }
    for member in update.members {
        let at = current.iter().position(|held| held.mri == member.mri);
        match (at, member.present) {
            (Some(at), false) => {
                current.remove(at);
            }
            (Some(at), true) => current[at] = member,
            (None, true) => current.push(member),
            // Somebody who left and was never here needs no room made for them.
            (None, false) => {}
        }
    }
    *current != before
}

/// A call the service accepted: the links it answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedCall {
    pub links: Links,
    /// The conversation the service created for this call, when it named one.
    pub conversation_url: Option<String>,
    /// That conversation's own doors, replaced rather than merged (see [`ConversationLeg`]).
    pub conversation: Option<ConversationLeg>,
    /// The whole response, for the journal while the schema is still young.
    pub raw: Value,
}

/// What a signaling request is FOR — and so how long its answer may take before the transport
/// gives up on it (docs/calling-roadmap/plane-reliability.md § 4.6).
///
/// Nothing bounded a signaling request before this: the backend's client carries no timeout, so a
/// request the service never answered held its caller for ever — and the frame handler awaits its
/// acknowledgements in line, so one unanswered acknowledgement stopped every later frame of the
/// call behind it. Each class is the real client's own bound for that kind of operation, or above
/// it, so this app never gives up on an answer the client would still be waiting for.
///
/// The bound aborts the HTTP request and NOTHING ELSE. The credentials are read before the request
/// starts and outside it: `session()` and the token cache can hold the broker's interactive turn,
/// and a future dropped there would leave an abandoned window on the broker's display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalClass {
    /// A request whose ANSWER carries the links of a leg the service may already have made — the
    /// invitation, a join, an attach, an acceptance, a sharing session (`addModality`). Those links
    /// are the only address that leg will ever have, so its caller runs in a task of its own and a
    /// page that goes away never cancels it. 65 s: the client's `addModalityTimeoutSec`, its
    /// longest single-request operation.
    Create,
    /// A media answer, our own renegotiation offer, or a subscription. 35 s, the client's
    /// `mediaAnswerTimeoutSec`: a lost answer costs one renegotiation, never a leg nobody holds.
    Answer,
    /// Ends, refuses, acknowledges or publishes — a hangup, a reject, a mute, a keep-alive, a
    /// stop-sharing, a glare refusal, an acknowledgement, the relay token, the unregister. 10 s,
    /// the client's `fetchTimeoutMs`: each is idempotent or ending, so a lost answer costs nothing
    /// the ending itself does not settle.
    End,
    /// The hangup and the unregister a STOPPING backend sends, inside the stop budget the
    /// supervisors grant it (§ 4.8). 4 s.
    Shutdown,
}

impl SignalClass {
    /// How long the transport waits for this class's answer, connection included.
    pub const fn timeout(self) -> std::time::Duration {
        std::time::Duration::from_secs(match self {
            SignalClass::Create => 65,
            SignalClass::Answer => 35,
            SignalClass::End => 10,
            SignalClass::Shutdown => 4,
        })
    }
}

/// The service REFUSED a signaling request outright: a 4xx, which says the request was read and
/// turned down — never a lost answer, which says nothing about whether it was carried out.
///
/// It travels in the error chain so a caller can tell the two apart
/// (`e.downcast_ref::<SignalRefused>()`). A step that falls back on a refusal must never fall back
/// on a timeout or a dropped connection: the leg it would fall back FROM may exist, and the only
/// address it has is the answer that never arrived. A 5xx is not one either — the service failed,
/// which says no more than silence does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalRefused {
    pub status: u16,
    /// The sentence the error reads as: the service's own words and headers, unchanged, because
    /// they name the real cause ("this user has no calling licence").
    message: String,
}

impl std::fmt::Display for SignalRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SignalRefused {}

/// POST one signaling frame and return the parsed response (or `Value::Null` for an empty 200,
/// which is the normal answer to an acknowledgement).
///
/// This and its two siblings are THE ONE way this app reaches the calling service
/// (docs/calling-roadmap/README.md § 3, collision 7): every request carries the same headers
/// ([`signal_headers`]), is read the same way ([`read_signal_reply`]) and is bounded by its
/// [`SignalClass`]. A workstream that needs another verb uses these rather than adding a transport
/// or a timeout of its own.
pub async fn post_signal(
    http: &reqwest::Client,
    class: SignalClass,
    url: &str,
    session: &Session,
    ic3: &str,
    correlation_id: &str,
    payload: &Value,
) -> Result<Value> {
    signal_within(
        http,
        reqwest::Method::POST,
        url,
        class.timeout(),
        session,
        ic3,
        correlation_id,
        payload,
    )
    .await
}

/// PUT one signaling frame — [`post_signal`] with the other verb, for the conversation's state
/// doors (docs/calling-roadmap/meeting-participation.md § 5).
pub async fn put_signal(
    http: &reqwest::Client,
    class: SignalClass,
    url: &str,
    session: &Session,
    ic3: &str,
    correlation_id: &str,
    payload: &Value,
) -> Result<Value> {
    signal_within(
        http,
        reqwest::Method::PUT,
        url,
        class.timeout(),
        session,
        ic3,
        correlation_id,
        payload,
    )
    .await
}

/// DELETE with a signaling body — [`post_signal`] with the other verb. The client's own DELETE
/// carries a body (`sendDeleteRequest({url, payload})`), so this one does too
/// (docs/calling-roadmap/incoming-and-conversation.md § 4.5).
pub async fn delete_signal(
    http: &reqwest::Client,
    class: SignalClass,
    url: &str,
    session: &Session,
    ic3: &str,
    correlation_id: &str,
    payload: &Value,
) -> Result<Value> {
    signal_within(
        http,
        reqwest::Method::DELETE,
        url,
        class.timeout(),
        session,
        ic3,
        correlation_id,
        payload,
    )
    .await
}

/// Send one signaling request under an explicit bound. The three public verbs are this with their
/// class's bound; the tests drive it with a short one against a stub that never answers.
#[allow(clippy::too_many_arguments)]
async fn signal_within(
    http: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    bound: std::time::Duration,
    session: &Session,
    ic3: &str,
    correlation_id: &str,
    payload: &Value,
) -> Result<Value> {
    // What we SENT, when the capture switch is on: a payload the service refuses is
    // diagnosed by comparing it with the real client's, and it cannot be compared if it
    // was never written down. Redacted of nothing, because it is the user's own machine
    // and the SDP is theirs — see `record_call_frame` for where this must not be pointed.
    if std::env::var("TEAMS_LITE_CALL_DEBUG").as_deref() == Ok("1") {
        eprintln!(
            "[calling] {method} {}\n{}",
            redact_url(url),
            serde_json::to_string_pretty(payload).unwrap_or_default()
        );
    }
    let response = signal_headers(http.request(method.clone(), url), &session.region, ic3, correlation_id)
        .json(payload)
        .timeout(bound)
        .send()
        .await
        .map_err(|e| transport_failure(&method, url, bound, e))?;
    read_signal_reply(response, &method, url, bound).await
}

/// The headers every signaling request carries, whatever its verb.
///
/// Every header below was copied off a request this service ACCEPTED (captured from
/// the real client, 2026-08-05). Two of them are absences, and they are the point:
///
///   * NO `X-Skypetoken`. The captured request authenticates with the ic3 bearer
///     ALONE. This app sent both, and a calling request carrying two credentials is a
///     shape the real client never produces — the likeliest reading of a `400` that
///     names nothing.
///   * NO `api-version`. The conversation service is not versioned that way; the
///     header belongs to the relay-token GET, which is a different service.
fn signal_headers(
    builder: reqwest::RequestBuilder,
    region: &str,
    ic3: &str,
    correlation_id: &str,
) -> reqwest::RequestBuilder {
    builder
        .header("content-type", "application/json")
        .header("accept", "*/*")
        .header("authorization", format!("Bearer {ic3}"))
        // Who this client is, in the format the CALLING service takes — which is not the
        // Skype-era one: `SkypeSpaces/{version}/{platform}/TsCallingVersion=…`.
        .header("X-Microsoft-Skype-Client", CLIENT_IDENTITY)
        .header("X-Microsoft-Skype-Chain-ID", correlation_id)
        // One id per REQUEST, beside the chain id that spans the call.
        .header("X-Microsoft-Skype-Message-ID", uuid::Uuid::new_v4().to_string())
        .header("X-MS-Migration", "True")
        // Where this user lives. The service routes on all three, and the captured
        // request carries them on every call.
        .header("ms-teams-region", region)
        .header("ms-teams-partition", teams_partition(region))
        .header("ms-teams-ring", "general")
}

/// Why a signaling request never got an answer: the bound ran out, or the connection did.
///
/// Said as a lost answer, never as a refusal — nothing here knows whether the service carried the
/// request out, and a caller must not read silence as a no.
///
/// reqwest's own error carries the WHOLE address, query included, and prints it ("… for url
/// (<url>)"), so it is stripped before it is wrapped: a signaling link can carry a token in its
/// query, and this sentence ends up in a journal that is read for weeks. The redacted address in
/// the context is the only one left.
fn transport_failure(
    method: &reqwest::Method,
    url: &str,
    bound: std::time::Duration,
    error: reqwest::Error,
) -> anyhow::Error {
    let error = error.without_url();
    if error.is_timeout() {
        anyhow::Error::new(error).context(format!(
            "calling: no answer to a signaling {method} from {} within {} s",
            redact_url(url),
            bound.as_secs_f32()
        ))
    } else {
        anyhow::Error::new(error)
            .context(format!("calling: signaling {method} to {} failed", redact_url(url)))
    }
}

/// Read a signaling answer: its JSON on success, and the service's own words on a refusal.
async fn read_signal_reply(
    response: reqwest::Response,
    method: &reqwest::Method,
    url: &str,
    bound: std::time::Duration,
) -> Result<Value> {
    let status = response.status();
    // The service explains a refusal in HEADERS as often as in the body: a validation
    // failure answers `{}` and names the field in `x-microsoft-skype-*`. A 400 with an
    // empty body and no headers is an error nobody can act on, and this app got one.
    let reasons: Vec<String> = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            name.starts_with("x-microsoft-skype") || name.starts_with("x-ms-") || name == "ms-cv"
        })
        .filter_map(|(name, value)| value.to_str().ok().map(|v| format!("{name}: {v}")))
        .collect();
    let text = match response.text().await {
        Ok(text) => text,
        // A refusal is reported by its status whatever became of its words.
        Err(_) if !status.is_success() => String::new(),
        // A SUCCESS whose body never arrived whole is still a success: the service DID what was
        // asked, and only its words were lost. It reads as an empty answer, as it always has —
        // and that is the safer of the two for a creating request too. Reported as a failure, a
        // placed call would be ended here while the callee's phone rang, and the frames that can
        // still name its links (an acceptance, a conversation update) would then find no call to
        // land on. Kept, the call can still learn them. Said in the journal, since it is rare
        // and it is exactly what a "why did this call have no hangup link" question needs.
        Err(e) => {
            eprintln!(
                "[calling] {method} {} answered {status}, but its body was lost: {} (bounded at \
                 {} s)",
                redact_url(url),
                e.without_url(),
                bound.as_secs_f32()
            );
            String::new()
        }
    };
    if !status.is_success() {
        // A refusal says so in the journal, exactly as a refused write does. It used to
        // reach the page and nowhere else, so the only record of a call this machine
        // could not place was the sentence the user read out by hand. The url is
        // redacted and the credentials are never named.
        eprintln!(
            "[calling] refused: {status} from {} — {} [{}]",
            redact_url(url),
            text.chars().take(200).collect::<String>(),
            reasons.join("; ")
        );
        // Keep the service's own words: its phrases name the real cause ("this user
        // has no calling licence", "conversation does not exist") and a generic
        // message here would hide them.
        let message = format!(
            "calling: {status} from {} — {} [{}]",
            redact_url(url),
            text.chars().take(400).collect::<String>(),
            reasons.join("; ")
        );
        if status.is_client_error() {
            return Err(SignalRefused { status: status.as_u16(), message }.into());
        }
        return Err(anyhow!(message));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// Place a call: one POST to the conversation service.
#[allow(clippy::too_many_arguments)]
pub async fn place_call(
    http: &reqwest::Client,
    session: &Session,
    ic3: &str,
    local: &LocalParticipant,
    to: &[String],
    thread_id: Option<&str>,
    offer: &MediaContent,
    callbacks: &CallbackBase,
    correlation_id: &str,
) -> Result<PlacedCall> {
    if to.is_empty() {
        anyhow::bail!("calling: a call needs somebody to ring");
    }
    let endpoints = endpoints(session)?;
    let payload = invitation_payload(local, to, thread_id, offer, callbacks);
    let raw = post_signal(
        http,
        SignalClass::Create,
        &endpoints.conversation_service,
        session,
        ic3,
        correlation_id,
        &payload,
    )
    .await?;
    Ok(PlacedCall {
        links: Links::collect(&raw),
        conversation_url: ["/conversationUrl/Location", "/conversation/url", "/Location"]
            .iter()
            .find_map(|p| raw.pointer(p).and_then(Value::as_str))
            .map(String::from),
        conversation: ConversationLeg::from_response(&raw, &callbacks.surl),
        raw,
    })
}

/// An ICE server in the shape `RTCPeerConnection` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServer {
    pub fn json(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("urls".into(), json!(self.urls));
        if let Some(u) = &self.username {
            object.insert("username".into(), json!(u));
        }
        if let Some(c) = &self.credential {
            object.insert("credential".into(), json!(c));
        }
        Value::Object(object)
    }
}

/// The STUN server named by `calling_udpTransportUrl` (`udp://host:port`).
///
/// STUN needs no credentials, and it is what lets the browser learn its own public
/// address — enough for a call whose far side publishes reachable candidates of its
/// own, which is the ordinary Teams case (the media path terminates on a Microsoft
/// media server). A RELAYED path additionally needs the TURN credentials in
/// [`ice_servers_from_relay_config`].
pub fn stun_from_udp_transport(udp_transport: &str) -> Option<IceServer> {
    let rest = udp_transport.strip_prefix("udp://")?;
    let host_port = rest.trim_end_matches('/');
    if host_port.is_empty() || !host_port.contains(':') {
        return None;
    }
    Some(IceServer {
        urls: vec![format!("stun:{host_port}")],
        username: None,
        credential: None,
    })
}

/// Turn the service's own relay description into ICE servers, exactly as the web
/// client does: `turn:{host}:{port}?transport=udp`, the TCP form, and the TLS form
/// on the fqdn — each carrying the username and password from the relay token.
///
/// `relay` is the `Relay.Turn` object the service sent (`addresses`, `fqdns`,
/// `udpPort`, `tcpPort`, `tlsPort`); `credentials` is one entry of the token
/// response (`username`, `password`). Both come from the service, so a change in
/// either is a change we follow rather than one we have to predict.
pub fn ice_servers_from_relay_config(relay: &Value, credentials: &Value) -> Vec<IceServer> {
    let address = relay
        .get("addresses")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str);
    let fqdn = relay
        .get("fqdns")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str);
    let host = fqdn.or(address);
    let Some(host) = host else { return Vec::new() };

    let username = credentials.get("username").and_then(Value::as_str).map(String::from);
    let credential = credentials
        .get("password")
        .or_else(|| credentials.get("credential"))
        .and_then(Value::as_str)
        .map(String::from);

    let port = |name: &str| relay.get(name).and_then(Value::as_u64);
    let mut urls = Vec::new();
    if let Some(p) = port("udpPort") {
        urls.push(format!("turn:{host}:{p}?transport=udp"));
    }
    if let Some(p) = port("tcpPort") {
        urls.push(format!("turn:{host}:{p}?transport=tcp"));
    }
    // TLS is addressed by name only: a certificate is issued to the fqdn, never to
    // the bare address.
    if let (Some(p), Some(fqdn)) = (port("tlsPort"), fqdn) {
        urls.push(format!("turns:{fqdn}:{p}"));
    }
    if urls.is_empty() {
        return Vec::new();
    }
    vec![IceServer { urls, username, credential }]
}

/// Fetch the relay (TURN) credentials from the URL the service itself named
/// (`relayConfig.Service.tokenUrl`).
///
/// One GET with the skypetoken, exactly as the web client's legacy token path does.
/// The answer is `{tokens:[{realm, username, password}], expires}` — the credentials
/// stay in the backend, and only the built ICE servers ever reach the browser.
pub async fn fetch_relay_credentials(
    http: &reqwest::Client,
    token_url: &str,
    session: &Session,
) -> Result<Value> {
    let response = http
        .get(token_url)
        .header("X-Skypetoken", &session.skypetoken)
        .header("api-version", "2")
        // Bounded like every other request this module makes: the relay token is fetched while a
        // call is being prepared, and a GET nobody answered held `call_prepare` with it.
        .timeout(SignalClass::End.timeout())
        .send()
        .await
        // Never the URL: a relay token URL has carried a token in its query, and reqwest's own
        // error prints the whole address.
        .map_err(reqwest::Error::without_url)
        .context("calling: relay token request failed")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("calling: relay token -> {status}"));
    }
    serde_json::from_str(&text).context("calling: relay token is not JSON")
}

/// Read the relay configuration a frame carried (`relayConfig`, wherever the
/// service nested it), so a call can build its ICE servers from the service's own
/// description rather than from anything hard-coded here.
pub fn relay_config_in_frame(frame: &Value) -> Option<Value> {
    fn walk(value: &Value) -> Option<Value> {
        match value {
            Value::Object(map) => {
                if let Some(config) = map.get("relayConfig") {
                    return Some(config.clone());
                }
                map.values().find_map(walk)
            }
            Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(frame)
}

/// Build every ICE server a call should offer the browser: the directory's STUN
/// server, plus the service's TURN relay when we hold its credentials.
pub fn ice_servers(
    endpoints: &Endpoints,
    relay: Option<&Value>,
    credentials: Option<&Value>,
) -> Vec<IceServer> {
    let mut servers = Vec::new();
    if let Some(udp) = &endpoints.udp_transport
        && let Some(stun) = stun_from_udp_transport(udp)
    {
        servers.push(stun);
    }
    if let (Some(relay), Some(credentials)) = (relay, credentials) {
        servers.extend(ice_servers_from_relay_config(relay, credentials));
    }
    servers
}

/// The first credential entry of a relay-token response, whatever realm it names.
/// A response carries one entry per realm and a browser can only be given one pair.
pub fn first_relay_credential(token_response: &Value) -> Option<Value> {
    if let Some(first) =
        token_response.get("tokens").and_then(Value::as_array).and_then(|t| t.first())
    {
        return Some(first.clone());
    }
    // A single-realm response is the object itself.
    token_response
        .get("username")
        .is_some()
        .then(|| token_response.clone())
}

/// A URL with its query string dropped, for an error message. A signaling link
/// carries routing ids in its path and sometimes a token in its query, and an error
/// string ends up in a journal that is read for weeks.
fn redact_url(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn local() -> LocalParticipant {
        LocalParticipant {
            id: "8:orgid:me".into(),
            display_name: "Me".into(),
            endpoint_id: "endpoint-1".into(),
            participant_id: "participant-1".into(),
        }
    }

    fn callbacks() -> CallbackBase {
        CallbackBase {
            surl: "https://trouter.example/v4/f/abc/".into(),
            session_id: "session-1".into(),
            cause_id: "0a1b2c3d".into(),
        }
    }

    #[test]
    fn a_callback_link_is_built_the_way_the_web_client_builds_it() {
        let link = callbacks().link(paths::CALL_MEDIA_ANSWER);
        assert_eq!(
            link,
            "https://trouter.example/v4/f/abc/callAgent/session-1/0a1b2c3d/call/mediaAnswer/"
        );
    }

    /// The surl the trouter hands back has sometimes carried no trailing slash, and
    /// a link with a doubled or a missing slash is a link the service cannot reach.
    #[test]
    fn a_surl_without_a_trailing_slash_still_makes_one_link() {
        let base = CallbackBase {
            surl: "https://trouter.example/v4/f/abc".into(),
            session_id: "s".into(),
            cause_id: "c".into(),
        };
        assert_eq!(base.link("/call/end/"), "https://trouter.example/v4/f/abc/callAgent/s/c/call/end/");
    }

    #[test]
    fn links_are_collected_from_every_depth_and_the_deepest_wins() {
        let frame = json!({
            "conversation": {
                "links": { "conversationEnd": "https://x/conv/end", "hangup": "https://x/outer" },
                "call": { "links": { "hangup": "https://x/inner", "mediaAnswer": "https://x/ma" } }
            }
        });
        let links = Links::collect(&frame);
        assert_eq!(links.media_answer(), Some("https://x/ma"));
        // The nested (per-call) hangup wins over the per-conversation one.
        assert_eq!(links.hangup(), Some("https://x/inner"));
    }

    /// A link table is only useful if a non-URL value can never enter it: a
    /// `links` object also carries flags, and a `true` posted as a URL is a panic
    /// waiting for a live call.
    #[test]
    fn a_links_object_only_takes_urls() {
        let links = Links::collect(&json!({ "links": { "hangup": true, "accept": "https://x/a" } }));
        assert_eq!(links.hangup(), None);
        assert_eq!(links.accept(), Some("https://x/a"));
    }

    /// The link table of a REAL one-to-one invite, measured on this tenant 2026-09-04.
    ///
    /// Three things about it decided the accept path, and each is asserted here rather than
    /// described in a comment somewhere: there is no `accept` and no `acceptance`, the way
    /// in is `attach` (the path says `/forked/`, because the invite is addressed to the user
    /// and forked to every endpoint), and `mediaAnswer` is the shorthand `cc://ma`, which is
    /// not something this app can POST to.
    ///
    /// Before this, `accept()` found nothing, the accept fell through to that shorthand, and
    /// answering an incoming call failed on the URL scheme with a message about neither the
    /// call nor the link. A regression puts an unanswerable ring back.
    #[test]
    fn a_forked_one_to_one_invite_is_accepted_by_attaching_to_it() {
        let base = "https://api.flightproxy.teams.microsoft.com/api/v2/ep/cc-frce-02-prod-aks\
                    .cc.skype.com/cc/v1/forked/08dea4a3-a935-4684-a03f-d129c76b695a/27/i1/1351";
        let links = Links::collect(&json!({
            "callNotification": { "links": {
                "attach": format!("{base}/attach?i=10-128-136-201"),
                "progress": format!("{base}/progress?i=10-128-136-201"),
                "reject": format!("{base}/reject?i=10-128-136-201"),
                "mediaAnswer": "cc://ma",
                "udpTransport": "udp://52.123.144.224:3478/",
            }}
        }));

        // `attach` is its OWN door, not a spelling of `accept`: the two want different
        // envelopes, and posting the acceptance body to this link is refused `400` with
        // `The Attach field is required.`
        assert_eq!(links.accept(), None);
        assert_eq!(links.attach(), Some(format!("{base}/attach?i=10-128-136-201").as_str()));
        assert_eq!(links.reject(), Some(format!("{base}/reject?i=10-128-136-201").as_str()));
        // `collect_links` admits only a URL, so the shorthand and the transport never enter
        // the table at all — which is why the accept had nowhere to go before `attach` was
        // a spelling it knew, and why it failed with "no link to answer on" rather than on a
        // URL scheme.
        assert_eq!(links.media_answer(), None);
        assert_eq!(links.names(), vec!["attach", "progress", "reject"]);
    }

    /// The two doors that TAKE a call say the same thing under different names, and the
    /// service reads the name. `attach` was refused `400` for carrying the acceptance
    /// envelope, so a body that lost its own key would put that failure straight back.
    #[test]
    fn attaching_and_accepting_say_the_same_thing_under_their_own_names() {
        let (local, callbacks) = (local(), callbacks());
        let answer = MediaContent::sdp("v=0");

        let attach = attach_payload(&local, &answer, &callbacks);
        let acceptance = acceptance_payload(&local, &answer, &callbacks);

        // Each names its own envelope, and neither carries the other's.
        assert!(attach.get("attach").is_some(), "attach body: {attach}");
        assert!(attach.get("callAcceptance").is_none(), "attach body: {attach}");
        assert!(acceptance.get("callAcceptance").is_some(), "acceptance: {acceptance}");
        assert!(acceptance.get("attach").is_none(), "acceptance: {acceptance}");
        // And the statement inside is the same one, so the two cannot drift apart.
        assert_eq!(attach["attach"], acceptance["callAcceptance"]);
        // The answer really travels: an attach with no SDP takes a call and says nothing.
        assert_eq!(attach["attach"]["mediaContent"]["blob"], "v=0");
    }

    #[test]
    fn an_incoming_invite_is_read_out_of_a_wrapped_frame() {
        let frame = json!({
            "callId": "call-9",
            "_decoded": {
                "callNotification": {
                    "from": { "id": "8:orgid:her", "displayName": "Her" },
                    "to": { "participantId": "leg-7" },
                    "callModalities": ["audio"],
                    "groupChat": { "threadId": "19:thread@thread.v2" },
                    "mediaContent": { "blob": "v=0\r\n", "contentType": "application/sdp-ngc-1.0" },
                    "links": {
                        "accept": "https://x/accept",
                        "mediaAnswer": "https://x/media-answer",
                        "reject": "https://x/reject"
                    }
                }
            }
        });
        let call = incoming_call_from_frame(&frame).expect("an invite");
        assert_eq!(call.caller_mri, "8:orgid:her");
        assert_eq!(call.caller_name, "Her");
        assert_eq!(call.call_id, "call-9");
        assert_eq!(call.thread_id.as_deref(), Some("19:thread@thread.v2"));
        assert_eq!(call.participant_id.as_deref(), Some("leg-7"));
        assert!(call.has_audio());
        assert_eq!(call.offer.as_ref().map(|o| o.blob.as_str()), Some("v=0\r\n"));
        assert_eq!(call.links.accept(), Some("https://x/accept"));
        assert_eq!(call.links.media_answer(), Some("https://x/media-answer"));
    }

    /// The invite arrives un-wrapped when it is small enough, and it must read the
    /// same either way.
    #[test]
    fn an_unwrapped_invite_reads_the_same() {
        let frame = json!({
            "callNotification": {
                "from": { "id": "8:orgid:her" },
                "callModalities": ["audio", "video"],
            }
        });
        let call = incoming_call_from_frame(&frame).expect("an invite");
        assert_eq!(call.caller_mri, "8:orgid:her");
        assert!(call.has_audio());
        assert_eq!(call.offer, None);
    }

    /// Every other frame on the calling socket must read as "not an invite", or the
    /// app rings for a hangup.
    #[test]
    fn a_frame_that_is_not_an_invite_is_not_read_as_one() {
        for frame in [
            json!({ "_decoded": { "callEnd": { "code": 0 } } }),
            json!({ "_decoded": { "rosterUpdate": {} } }),
            json!({ "evt": "telemetry" }),
            // An invite with no caller names nobody, so it is not one.
            json!({ "callNotification": { "callModalities": ["audio"] } }),
        ] {
            assert_eq!(incoming_call_from_frame(&frame), None, "frame: {frame}");
        }
    }

    #[test]
    fn an_ending_is_read_from_either_name() {
        let roster = "https://surl/callAgent/s/c/conversation/rosterUpdate/";
        let end = call_ended_from_frame(
            roster,
            &json!({
                "_decoded": { "callEnd": {
                    "code": 410, "subCode": 5013, "phrase": "Gone", "callId": "c1"
                } }
            }),
        )
        .expect("an ending");
        assert_eq!(end.code, 410);
        assert_eq!(end.sub_code, 5013);
        assert_eq!(end.phrase, "Gone");
        assert_eq!(end.call_id, "c1");
        assert_eq!(end.accepted_elsewhere_by, None);
        assert!(call_ended_from_frame(roster, &json!({ "conversationEnd": {} })).is_some());
        assert!(call_ended_from_frame(roster, &json!({ "callNotification": {} })).is_none());
    }

    /// The client reads `e.body.callEnd || e.body`, so an ending posted to one of our end
    /// callbacks may carry no `callEnd` key at all. Missed, it left the call drawn as live.
    /// Anywhere else a body with no ending key is not an ending.
    #[test]
    fn a_bare_body_on_our_end_paths_is_an_ending_and_nowhere_else() {
        let base = CallbackBase {
            surl: "https://surl/".into(),
            session_id: "s".into(),
            cause_id: "c".into(),
        };
        let bare = json!({ "code": 487, "subCode": 10003, "phrase": "Request terminated" });
        for path in [paths::CALL_END, paths::CONVERSATION_END] {
            let url = base.link(path);
            // Unwrapped: the root IS the body.
            let end = call_ended_from_frame(&url, &bare).unwrap_or_else(|| panic!("a bare ending on {path}"));
            assert_eq!((end.code, end.sub_code), (487, 10003), "{path}");
            assert_eq!(end.phrase, "Request terminated");
            // Wrapped: the body is what `_decoded` holds, never the envelope around it.
            let wrapped = json!({ "evt": 1, "cp": "…", "_decoded": bare.clone() });
            let end = call_ended_from_frame(&url, &wrapped).expect("a wrapped bare ending");
            assert_eq!(end.code, 487, "{path}");
        }
        for path in [paths::CONVERSATION_UPDATE, paths::CALL_ACCEPTANCE, paths::CALL_TRANSFER] {
            assert_eq!(call_ended_from_frame(&base.link(path), &bare), None, "{path}");
        }
    }

    /// `callControllerTransactionEnd` is the ending when it is there — the client replaces its
    /// reading with it — and a field it does not state is still read off the outer object.
    /// `acceptedElsewhereBy` is read at either level.
    #[test]
    fn the_call_controllers_own_ending_overrides_the_outer_one() {
        let url = "https://surl/callAgent/s/c/call/end/";
        let end = call_ended_from_frame(
            url,
            &json!({ "callEnd": {
                "code": 0, "phrase": "Outer", "callId": "outer-id",
                "callControllerTransactionEnd": {
                    "code": 450, "subCode": 10500,
                    "acceptedElsewhereBy": { "id": "8:orgid:me", "displayName": "Me" }
                }
            } }),
        )
        .expect("an ending");
        assert_eq!((end.code, end.sub_code), (450, 10500));
        // The override names no phrase, so the outer one stands.
        assert_eq!(end.phrase, "Outer");
        assert_eq!(end.call_id, "outer-id");
        assert_eq!(end.accepted_elsewhere_by, Some(("8:orgid:me".into(), "Me".into())));

        let outer = call_ended_from_frame(
            url,
            &json!({ "_decoded": { "callEnd": {
                "code": 450,
                "acceptedElsewhereBy": { "id": "8:orgid:other", "displayName": "" }
            } } }),
        )
        .expect("an ending");
        assert_eq!(outer.accepted_elsewhere_by, Some(("8:orgid:other".into(), String::new())));
        // An `acceptedElsewhereBy` naming nobody names nobody.
        let empty = call_ended_from_frame(
            url,
            &json!({ "callEnd": { "code": 450, "acceptedElsewhereBy": {} } }),
        )
        .expect("an ending");
        assert_eq!(empty.accepted_elsewhere_by, None);
    }

    /// The four names a RING can end with, and only while it is still being offered here.
    #[test]
    fn a_ring_that_ended_elsewhere_is_named_and_a_live_call_is_not() {
        let ended = |code: i64, by: Option<(&str, &str)>| CallEnded {
            call_id: "c".into(),
            code,
            sub_code: 0,
            phrase: "CallEndReasonHangup".into(),
            accepted_elsewhere_by: by.map(|(id, name)| (id.to_string(), name.to_string())),
        };
        assert_eq!(end_reason_for(&ended(450, None), true), Some(END_REASON_ANSWERED_ELSEWHERE));
        assert_eq!(end_reason_for(&ended(451, None), true), Some(END_REASON_FORWARDED));
        assert_eq!(end_reason_for(&ended(452, None), true), Some(END_REASON_VOICEMAIL));
        assert_eq!(end_reason_for(&ended(487, None), true), Some(END_REASON_CALLER_CANCELLED));
        // Naming who took the call is a statement only an answer elsewhere makes.
        assert_eq!(
            end_reason_for(&ended(0, Some(("8:orgid:x", "X"))), true),
            Some(END_REASON_ANSWERED_ELSEWHERE)
        );
        // Any other code is the service's own phrase.
        assert_eq!(end_reason_for(&ended(0, None), true), None);
        assert_eq!(end_reason_for(&ended(603, None), true), None);
        // And a call that is not being offered here is never "answered elsewhere".
        for code in [450, 451, 452, 487] {
            assert_eq!(end_reason_for(&ended(code, None), false), None, "{code}");
        }
        assert_eq!(end_reason_for(&ended(450, Some(("8:orgid:x", "X"))), false), None);
    }

    #[test]
    fn one_person_is_one_person_however_the_mri_is_spelled() {
        assert!(same_person("8:orgid:ABC-1", "8:orgid:abc-1"));
        assert!(same_person("8:orgid:abc-1", "abc-1"));
        assert!(same_person(" abc-1 ", "8:ORGID:abc-1"));
        assert!(!same_person("8:orgid:abc-1", "8:orgid:abc-2"));
        assert!(!same_person("", ""));
        assert!(!same_person("8:orgid:", "8:orgid:"));
    }

    /// The conversation half of an invite, read wrapped and unwrapped, and stated as a shape
    /// that names no value — the subject and the controller URL above all.
    #[test]
    fn an_invites_conversation_is_read_and_journaled_as_a_shape() {
        let invite = json!({
            "evt": 109,
            "_decoded": {
                "callNotification": { "from": { "id": "8:orgid:caller", "displayName": "C" } },
                "conversationInvitation": {
                    "conversationController": "https://cc.example/conv/secret",
                    "isMultiParty": true,
                    "isHostless": false,
                    "subject": "  Quarterly planning ",
                },
                "meetingInfo": { "organizerId": "x" },
            }
        });
        let call = incoming_call_from_frame(&invite).expect("an invite");
        let c = &call.conversation;
        assert_eq!(c.evt, Some(109));
        assert_eq!(c.controller.as_deref(), Some("https://cc.example/conv/secret"));
        assert_eq!((c.multi_party, c.hostless), (Some(true), Some(false)));
        assert_eq!(c.subject.as_deref(), Some("Quarterly planning"));
        assert!(c.meeting);
        let shape = c.shape();
        assert!(shape.contains("evt 109"), "{shape}");
        assert!(shape.contains("\"isMultiParty\""), "{shape}");
        assert!(!shape.contains("Quarterly"), "the subject is user content: {shape}");
        assert!(!shape.contains("cc.example"), "the controller is a URL: {shape}");

        // Unwrapped, with the meeting named by its thread and `evt` sent as text.
        let plain = json!({
            "evt": "107",
            "callNotification": {
                "from": { "id": "8:orgid:caller" },
                "groupChat": { "threadId": "19:meeting_abc@thread.v2" },
            }
        });
        let c = incoming_call_from_frame(&plain).expect("an invite").conversation;
        assert_eq!(c.evt, Some(107));
        assert!(c.meeting);
        assert!(c.invitation_keys.is_empty());
        assert_eq!(c.multi_party, None);

        // An invite that says nothing about its conversation is the one-to-one it always was.
        let bare = json!({ "callNotification": { "from": { "id": "8:orgid:caller" } } });
        assert_eq!(
            incoming_call_from_frame(&bare).expect("an invite").conversation,
            InviteConversation::default()
        );
    }

    #[test]
    fn an_acceptance_says_how_it_wants_to_be_kept_alive() {
        let ack = json!({ "callAcceptanceAcknowledgement": {
            "callKeepAliveInterval": 45,
            "links": { "callLeg": "https://cc/leg" }
        } });
        let offer = keep_alive_offer(&ack).expect("an offer");
        assert_eq!(offer.source, "callAcceptanceAcknowledgement");
        assert_eq!(offer.interval_s, Some(45));
        assert!(offer.call_leg);
        assert!(!offer.shape().contains("https"), "{}", offer.shape());

        let frame = json!({ "_decoded": { "callAcceptance": {
            "callKeepAliveInterval": "60", "links": { "acknowledgement": "https://cc/ack" }
        } } });
        let offer = keep_alive_offer(&frame).expect("an offer");
        assert_eq!((offer.source, offer.interval_s, offer.call_leg), ("callAcceptance", Some(60), false));

        let silent = keep_alive_offer(&json!({ "callAcceptance": {} })).expect("an offer");
        assert_eq!(silent.interval_s, None);
        assert!(silent.shape().contains("interval none"));
        assert_eq!(keep_alive_offer(&json!({ "rosterUpdate": {} })), None);
    }

    #[test]
    fn a_conversation_says_which_doors_it_names_and_no_url() {
        let update = json!({ "_decoded": { "conversationUpdate": {
            "links": {
                "leave": "https://cc/leave",
                "updateEndpointState": "https://cc/ues",
                "subscribeToBroker": "https://cc/b",
            }
        } } });
        let doors = conversation_doors(&update).expect("doors");
        assert_eq!(doors.source, "conversationUpdate");
        let shape = doors.shape();
        assert!(shape.contains("leave yes"), "{shape}");
        assert!(shape.contains("updateEndpointState yes"), "{shape}");
        assert!(shape.contains("notificationLinks no"), "{shape}");
        assert!(shape.contains("unmute no"), "{shape}");
        assert!(shape.contains("broker-shaped [\"subscribeToBroker\"]"), "{shape}");
        assert!(!shape.contains("https"), "{shape}");

        let attach = json!({ "callInvitation": { "links": { "acceptance": "a" } },
            "additionalActionResponses": [ { "output": { "links": { "unmute": "u" } } } ] });
        let doors = conversation_doors(&attach).expect("doors");
        assert_eq!(doors.source, "additionalActionResponses output");
        assert!(doors.shape().contains("unmute yes"));

        let place = json!({ "links": { "leave": "l" }, "state": "x" });
        assert_eq!(conversation_doors(&place).expect("doors").source, "conversation");
        assert_eq!(conversation_doors(&json!({ "callInvitation": {} })), None);
    }

    /// A conversation's doors are its OWN top-level `links`: a `leave` nested under the call leg
    /// or a sharing session is a different door with the same name, and a link on this call's
    /// own callback surl is one of ours echoed back. The sequence is the client's reading of it —
    /// a number, a numeric string, or -1 for none or a negative one.
    #[test]
    fn a_conversation_leg_is_read_off_its_own_top_level_doors() {
        let surl = callbacks().surl;
        let answer = json!({
            "conversationController": "https://cc/conv",
            "sequenceNumber": 7,
            "state": { "isMultiParty": true },
            "links": {
                "leave": "https://cc/conv/leave",
                "updateEndpointState": "https://cc/conv/ues",
                // Ours, echoed back: never a door.
                "conversationUpdate": callbacks().link(paths::CONVERSATION_UPDATE),
                // A flag, not a URL: never a door either.
                "notificationLinks": { "x": 1 },
            },
            "callInvitation": { "links": { "leave": "https://cc/leg/leave" } },
        });
        let leg = ConversationLeg::from_response(&answer, &surl).expect("a conversation");
        assert_eq!(leg.leave(), Some("https://cc/conv/leave"), "the call leg's leave leaked in");
        assert_eq!(leg.update_endpoint_state(), Some("https://cc/conv/ues"));
        assert_eq!(leg.links.names(), vec!["leave", "updateEndpointState"]);
        assert_eq!(leg.sequence_number, 7);
        assert_eq!(leg.controller.as_deref(), Some("https://cc/conv"));
        assert_eq!(leg.state, json!({ "isMultiParty": true }));

        for (sequence, read) in [(json!("12"), 12), (json!(-3), -1), (json!("soon"), -1), (Value::Null, -1)] {
            let body = json!({ "sequenceNumber": sequence, "links": {} });
            assert_eq!(ConversationLeg::from_response(&body, &surl).unwrap().sequence_number, read);
        }

        // Where it is looked for is where the journal looks: one rule, two readers.
        let wrapped = json!({ "_decoded": { "conversationUpdate": { "links": { "unmute": "https://cc/u" } } } });
        assert_eq!(ConversationLeg::from_response(&wrapped, &surl).unwrap().unmute(), Some("https://cc/u"));
        let attach = json!({ "additionalActionResponses": [ { "output": { "links": { "leave": "https://cc/o" } } } ] });
        assert_eq!(ConversationLeg::from_response(&attach, &surl).unwrap().leave(), Some("https://cc/o"));
        // No top-level `links` object is no conversation, whatever is nested deeper.
        assert_eq!(ConversationLeg::from_response(&json!({ "callAcceptance": { "links": { "leave": "https://x" } } }), &surl), None);
        assert_eq!(ConversationLeg::from_response(&json!({ "links": "https://x" }), &surl), None);
    }

    /// The client REPLACES a conversation's doors on every object it applies, and ignores one
    /// whose sequence is below the last applied — which is how a withdrawn `unmute` stops being
    /// offered, and how an update that arrives late cannot bring it back.
    #[test]
    fn a_conversation_leg_is_replaced_whole_under_the_sequence_gate() {
        let surl = callbacks().surl;
        let leg = |sequence: Value, links: Value| {
            ConversationLeg::from_response(&json!({ "sequenceNumber": sequence, "links": links }), &surl).unwrap()
        };
        let mut known = leg(json!(5), json!({ "unmute": "https://cc/unmute", "updateEndpointState": "https://cc/ues" }));

        // Newer and without `unmute`: the door is withdrawn, not kept from the merge.
        assert!(known.apply(leg(json!(6), json!({ "updateEndpointState": "https://cc/ues2" }))));
        assert_eq!(known.unmute(), None, "a withdrawn door survived the update");
        assert_eq!(known.update_endpoint_state(), Some("https://cc/ues2"));

        // Older: ignored whole.
        assert!(!known.apply(leg(json!(4), json!({ "unmute": "https://cc/old" }))));
        assert_eq!((known.unmute(), known.sequence_number), (None, 6));
        // Unnumbered, once a numbered one was applied: ignored, as the client reads it.
        assert!(!known.apply(leg(Value::Null, json!({ "unmute": "https://cc/old" }))));
        // Equal: applied.
        assert!(known.apply(leg(json!(6), json!({ "leave": "https://cc/leave" }))));
        assert_eq!(known.links.names(), vec!["leave"]);

        // Two unnumbered objects apply in the order they arrive.
        let mut unnumbered = leg(Value::Null, json!({ "leave": "https://cc/a" }));
        assert!(unnumbered.apply(leg(Value::Null, json!({ "leave": "https://cc/b" }))));
        assert_eq!(unnumbered.leave(), Some("https://cc/b"));
    }

    /// The mute is published as the client's own `updateEndpointState`: `{from, endpointState}`
    /// with nothing else, the endpoint state being the last one published with its `state`
    /// replaced and the next sequence — 1 on the first publish, because the start body never
    /// sets it.
    #[test]
    fn the_mute_is_published_as_the_clients_own_endpoint_state() {
        let first = next_endpoint_state(&serde_json::Map::new(), true, 1);
        assert_eq!(
            Value::Object(first.clone()),
            json!({ "state": { "isMuted": true }, "endpointStateSequenceNumber": 1 })
        );
        let body = endpoint_state_payload(&local(), &first);
        assert_eq!(
            body,
            json!({
                "from": {
                    "id": "8:orgid:me", "displayName": "Me", "endpointId": "endpoint-1",
                    "participantId": "participant-1", "languageId": "en-us",
                },
                "endpointState": { "state": { "isMuted": true }, "endpointStateSequenceNumber": 1 },
            })
        );

        // The spread is SHALLOW: every other key the last publish held travels again, `state` is
        // replaced whole, and the sequence is the new one.
        let mut held = first.clone();
        held.insert("endpointProperties".into(), json!({ "preheatProperties": 0 }));
        held.insert("state".into(), json!({ "isMuted": true, "other": 1 }));
        let second = next_endpoint_state(&held, false, 2);
        assert_eq!(
            Value::Object(second),
            json!({
                "endpointProperties": { "preheatProperties": 0 },
                "state": { "isMuted": false },
                "endpointStateSequenceNumber": 2,
            })
        );
    }

    /// A call this app STARTS publishes its mute on top of the endpoint state its start body
    /// declared, as the client's `startOrJoinCall` seeds `latestEndpointState` — so the first
    /// publish re-sends the `endpointProperties`, at sequence 1. The place and join bodies send
    /// that very object, spelled once.
    #[test]
    fn a_started_calls_first_mute_carries_what_its_start_body_declared() {
        let seed = start_endpoint_state();
        let first = next_endpoint_state(seed.as_object().unwrap(), true, 1);
        assert_eq!(
            Value::Object(first),
            json!({
                "endpointStateSequenceNumber": 1,
                "endpointProperties": {
                    "additionalEndpointProperties": { "infoShownInReportMode": "FullInformation" }
                },
                "state": { "isMuted": true },
            })
        );
        assert!(!endpoint_state_is_muted(seed.as_object().unwrap()), "the seed reads as unmuted");
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let offer = MediaContent::sdp("v=0\r\n");
        assert_eq!(join_payload(&local(), &meeting, &callbacks(), Some(&offer))["endpointState"], seed);
        assert_eq!(
            invitation_payload(&local(), &["8:orgid:her".into()], None, &offer, &callbacks())["endpointState"],
            seed
        );
    }

    /// The client reads `state.isMuted` loosely: an endpoint state that says nothing about it
    /// is NOT muted, which is what makes a first Unmute "nothing new to send".
    #[test]
    fn an_endpoint_state_that_says_nothing_reads_as_not_muted() {
        let read = |v: Value| endpoint_state_is_muted(v.as_object().unwrap());
        assert!(!read(json!({})));
        assert!(!read(json!({ "state": {} })));
        assert!(!read(json!({ "state": { "isMuted": "yes" } })));
        assert!(!read(json!({ "state": { "isMuted": false } })));
        assert!(read(json!({ "state": { "isMuted": true } })));
    }

    /// The protocol is this app's EXISTING flow unless the user said otherwise: absent, unreadable, or a
    /// word this build does not know reads as the default, field by field, and a key a newer build
    /// stored is ignored. The RPC is stricter, because a typo that changed nothing would leave the
    /// user believing a protocol is in force that is not.
    #[test]
    fn the_calling_protocol_reads_as_measured_unless_told_otherwise() {
        for stored in [None, Some(""), Some("not json"), Some("[]"), Some(r#"{"mute":"loud"}"#), Some(r#"{"mute":1}"#)] {
            assert_eq!(CallProtocol::from_stored(stored), CallProtocol::default(), "{stored:?}");
        }
        assert_eq!(CallProtocol::default().mute, MuteProtocol::Legacy);
        assert_eq!(CallProtocol::default().incoming, IncomingProtocol::Answer);
        // A word this build does not know — M4b's `ring`, a newer build's key — reads as the
        // default for that field alone, and the field beside it keeps its own.
        let stored = CallProtocol::from_stored(Some(
            r#"{"mute":"endpoint-state","incoming":"ring","keepalive":"client"}"#,
        ));
        assert_eq!(stored.mute, MuteProtocol::EndpointState);
        assert_eq!(stored.incoming, IncomingProtocol::Answer);
        assert_eq!(stored.json(), json!({ "mute": "endpoint-state", "incoming": "answer" }));
        assert_eq!(CallProtocol::from_stored(Some(&stored.json().to_string())), stored);
        let joining = CallProtocol::from_stored(Some(r#"{"incoming":"join-at-answer"}"#));
        assert_eq!(joining.incoming, IncomingProtocol::JoinAtAnswer);
        assert_eq!(joining.mute, MuteProtocol::Legacy);
        for word in MuteProtocol::WORDS {
            assert_eq!(MuteProtocol::from_word(word).unwrap().word(), word);
        }
        for word in IncomingProtocol::WORDS {
            assert_eq!(IncomingProtocol::from_word(word).unwrap().word(), word);
        }

        let base = CallProtocol::default();
        let set = |params: Value| base.with_changes(&params);
        assert_eq!(set(json!({})).unwrap(), base);
        assert_eq!(set(json!({ "write_token": "t", "mute": "endpoint-state" })).unwrap().mute, MuteProtocol::EndpointState);
        let refused = format!("{:#}", set(json!({ "mute": "loud" })).unwrap_err());
        assert!(refused.contains("`mute` must be one of"), "{refused}");
        assert_eq!(
            set(json!({ "incoming": "join-at-answer" })).unwrap().incoming,
            IncomingProtocol::JoinAtAnswer
        );
        // `ring` is M4b's word, and a choice that changes nothing is refused rather than kept.
        let refused = format!("{:#}", set(json!({ "incoming": "ring" })).unwrap_err());
        assert!(refused.contains("`incoming` must be one of"), "{refused}");
        let refused = format!("{:#}", set(json!({ "keepalive": "client" })).unwrap_err());
        assert!(refused.contains("does not know `keepalive`"), "{refused}");
        assert_eq!(format!("{:#}", set(json!({ "mute": true })).unwrap_err()).contains("must be one of"), true);
    }

    /// The setting is one per BACKEND, keyed by its port, so the released build may try a
    /// protocol while the staged one keeps the existing one on one shared store.
    #[test]
    fn the_calling_protocol_is_kept_per_port() {
        assert_eq!(call_protocol_setting_key(19422), "calling_protocol:19422");
        assert_ne!(call_protocol_setting_key(19420), call_protocol_setting_key(19422));
    }

    #[test]
    fn a_frame_nothing_acts_on_is_named_by_its_keys() {
        assert_eq!(
            body_key_names(&json!({ "evt": 1, "_decoded": { "transfer": {}, "links": {} } })),
            vec!["links".to_string(), "transfer".to_string()]
        );
        assert_eq!(body_key_names(&json!({ "replacement": {} })), vec!["replacement".to_string()]);
        assert!(body_key_names(&json!("text")).is_empty());
    }

    /// The frame that names the CAUSE of a call that ends two seconds in, in the shape the
    /// tenant really sends it (`web/scripts/call-live.ts` measured this one).
    #[test]
    fn an_invitation_that_reached_nobody_is_read_and_told_apart() {
        let url = "https://pub.trouter.example/v4/f/x/callAgent/a/b/conversation/\
                   addParticipantFailure/";
        let failed = invite_failed(
            url,
            &json!({
                "participantInfos": [{
                    "code": 480,
                    "subCode": 10037,
                    "reason": "clientError",
                    "phrase": "No callee endpoints were found."
                }],
                "participants": { "8:orgid:x": { "code": 580, "subCode": 5205 } }
            }),
        )
        .expect("a failed invitation");
        assert!(failed.no_endpoints);
        assert_eq!(failed.phrase, "No callee endpoints were found.");

        // A device that rang and refused is NOT this: somebody was reachable, and the
        // ending's own phrase is the honest thing to report.
        let refused = invite_failed(
            url,
            &json!({ "participantInfos": [{ "code": 486, "subCode": 10004, "phrase": "Busy here." }] }),
        )
        .expect("a failed invitation");
        assert!(!refused.no_endpoints);
        assert_eq!(refused.phrase, "Busy here.");

        // And it is recognised by its own callback path, never by a body shape another
        // frame could share.
        assert!(invite_failed("…/conversation/rosterUpdate/", &json!({ "phrase": "x" })).is_none());
    }

    /// The name of that ending is a CONTRACT with the page: the backend states it and
    /// `web/src/lib/call.ts` turns it into the sentence the user reads. Two spellings of one
    /// name would leave the user with "The call ended." and no way to notice why.
    #[test]
    fn the_page_knows_the_name_of_an_unreachable_ending() {
        let call_ts = include_str!("../web/src/lib/call.ts");
        assert!(
            call_ts.contains(END_REASON_UNREACHABLE),
            "web/src/lib/call.ts must name {END_REASON_UNREACHABLE}, or an unreachable \
             colleague is reported as \"The call ended.\""
        );
    }

    /// The same contract for a ringing call let go without a decline: unnamed on the page it
    /// falls to the default, and the user is told "The call ended." about a call they ended.
    #[test]
    fn the_page_knows_the_name_of_a_ring_let_go() {
        let call_ts = include_str!("../web/src/lib/call.ts");
        assert!(
            call_ts.contains(&format!("\"{END_REASON_LEFT_RINGING}\"")),
            "web/src/lib/call.ts must name {END_REASON_LEFT_RINGING}, or a ring the user let go \
             is reported as \"The call ended.\""
        );
    }

    /// The same contract for a call a stopping backend left: unnamed on the page it falls to "The
    /// call ended.", and the user is not told that teams-lite going away is what took them out.
    #[test]
    fn the_page_knows_the_name_of_a_call_left_by_a_stopping_backend() {
        let call_ts = include_str!("../web/src/lib/call.ts");
        assert!(
            call_ts.contains(&format!("\"{END_REASON_BACKEND_STOPPING}\"")),
            "web/src/lib/call.ts must name {END_REASON_BACKEND_STOPPING}, or a call teams-lite left \
             as it stopped is reported as \"The call ended.\""
        );
    }

    /// The page's type, the mock's refusal list and the Settings row's options name exactly the
    /// backend's `MuteProtocol` words, in its order: a word the page does not know is a protocol
    /// it cannot show, and a word the mock accepts that the backend refuses is a test that passes
    /// over a refusal the user would meet.
    #[test]
    fn the_page_and_the_mock_know_the_mute_protocol_words() {
        // The page's TYPE, and only the type: the words are quoted in doc comments too, so a
        // scan of the whole file would pass with a word dropped from the one place that decides
        // what the page accepts.
        let members = |source: &str, declaration: &str, end: char| -> Vec<String> {
            let start = source.find(declaration).unwrap_or_else(|| panic!("{declaration}"));
            let body = &source[start + declaration.len()..];
            let body = &body[..body.find(end).expect("the declaration ends")];
            body.split(['|', ','])
                .map(|m| m.trim().trim_matches('"').to_string())
                .filter(|m| !m.is_empty() && !m.starts_with('['))
                .collect()
        };
        let words: Vec<String> = MuteProtocol::WORDS.iter().map(|w| w.to_string()).collect();
        let page = include_str!("../web/src/lib/call.ts");
        assert_eq!(members(page, "export type MuteProtocol =", ';'), words, "web/src/lib/call.ts");
        let mock = include_str!("../web/mock/server.ts");
        let mock_words = members(mock, "const MOCK_MUTE_PROTOCOLS = [", ']');
        assert_eq!(mock_words, words, "web/mock/server.ts must refuse exactly what the backend refuses");
        // And the row offers every word, in the backend's order.
        let row = include_str!("../web/src/lib/calling-protocol.ts");
        let offered: Vec<usize> =
            words.iter().map(|w| row.find(&format!("value: \"{w}\"")).expect(w)).collect();
        assert!(offered.windows(2).all(|pair| pair[0] < pair[1]), "the row's order is not the backend's");

        // And the INCOMING row, the same three ways.
        let words: Vec<String> = IncomingProtocol::WORDS.iter().map(|w| w.to_string()).collect();
        assert_eq!(members(page, "export type IncomingProtocol =", ';'), words, "web/src/lib/call.ts");
        let mock_words = members(mock, "const MOCK_INCOMING_PROTOCOLS = [", ']');
        assert_eq!(mock_words, words, "web/mock/server.ts must refuse exactly what the backend refuses");
        let start = row.find("INCOMING_PROTOCOL_CHOICES").expect("the incoming row's choices");
        let offered: Vec<usize> = words
            .iter()
            .map(|w| start + row[start..].find(&format!("value: \"{w}\"")).expect(w))
            .collect();
        assert!(offered.windows(2).all(|pair| pair[0] < pair[1]), "the incoming row's order is not the backend's");
    }

    /// The attach that JOINS the conversation is the client's `WV` in what matters and nothing
    /// more: no SDP, no sender, no accepted modalities — claiming the leg and joining the
    /// conversation is all it does — the join aimed at the controller the invite named, the
    /// captured mask with bit 16 cleared, and no request envelope. Its exact bytes are the golden
    /// pin; this names the rules the pin holds.
    #[test]
    fn the_attach_that_joins_carries_no_media_and_names_the_controller() {
        let body = attach_join_payload(&local(), "https://conv/controller/", &callbacks(), false);
        assert_eq!(body["attach"]["requireMediaContent"], false);
        assert_eq!(
            attach_join_payload(&local(), "https://conv/controller/", &callbacks(), true)["attach"]
                ["requireMediaContent"],
            true
        );
        let text = body.to_string();
        for absent in ["mediaContent", "sender", "acceptedCallModalities", "\"payload\"", "blob"] {
            assert!(!text.contains(absent), "the attach carries {absent}: {text}");
        }
        assert_eq!(ATTACH_ENDPOINT_CAPABILITIES, 73447, "the captured mask with bit 16 cleared");
        assert_eq!(ATTACH_ENDPOINT_CAPABILITIES, CAPTURED_ENDPOINT_CAPABILITIES & !16);
        assert_eq!(body["endpointCapabilities"], 73447);
        let join = &body["additionalActions"][0];
        assert_eq!(join["name"], "join");
        assert_eq!(join["url"], "https://conv/controller/");
        assert_eq!(join["waitForResponse"], true);
        assert_eq!(join["input"]["endpointCapabilities"], 73447);
        assert_eq!(join["input"]["participants"]["from"]["participantId"], "participant-1");
        assert_eq!(
            join["input"]["conversationRequest"]["links"],
            conversation_callback_links(&callbacks()),
            "the join publishes the same conversation callbacks the place and join bodies do"
        );
        assert_eq!(
            body["attach"]["links"]["end"],
            callbacks().link(paths::CALL_END),
            "a ring's own ending lands on our call end callback"
        );
    }

    /// The same contract for every ending a RING can have and for one nobody answered: each
    /// name is a sentence on the page, and a name the page does not know falls to "The call
    /// ended." — about a call the user missed, or took on their phone.
    #[test]
    fn the_page_knows_the_names_of_a_ring_that_ended_elsewhere() {
        let call_ts = include_str!("../web/src/lib/call.ts");
        for name in [
            END_REASON_ANSWERED_ELSEWHERE,
            END_REASON_FORWARDED,
            END_REASON_VOICEMAIL,
            END_REASON_CALLER_CANCELLED,
            END_REASON_UNANSWERED,
        ] {
            assert!(
                call_ts.contains(&format!("\"{name}\"")),
                "web/src/lib/call.ts must name {name}, or the ring it names is reported as \
                 \"The call ended.\""
            );
        }
        // And the mock names them the same way, or the specs review a page the backend never
        // talks to.
        let mock = include_str!("../web/mock/server.ts");
        for name in [
            END_REASON_ANSWERED_ELSEWHERE,
            END_REASON_FORWARDED,
            END_REASON_VOICEMAIL,
            END_REASON_CALLER_CANCELLED,
        ] {
            assert!(mock.contains(&format!("\"{name}\"")), "web/mock/server.ts must name {name}");
        }
    }

    #[test]
    fn an_answer_sdp_is_read_out_of_a_media_answer_frame() {
        let answer = media_answer_from_frame(&json!({
            "_decoded": { "mediaAnswer": {
                "mediaContent": { "blob": "v=0 answer", "contentType": "application/sdp-ngc-1.0" }
            } }
        }))
        .expect("an answer");
        assert_eq!(answer.blob, "v=0 answer");
        assert!(media_answer_from_frame(&json!({ "callEnd": {} })).is_none());
    }

    /// What an answer GRANTED, stated for a journal — and never anything a journal must
    /// not hold.
    ///
    /// The measurement that asked for it: a screen share was offered live, the service
    /// answered by rejecting the section, and this machine's own log said only that an
    /// offer had gone out. Which section came back refused was unanswerable afterwards.
    #[test]
    fn an_answers_sections_are_stated_as_accepted_or_rejected() {
        let answer = [
            "v=0",
            "o=- 0 0 IN IP4 127.0.0.1",
            "a=fingerprint:sha-256 AA:BB:CC",
            "m=audio 3478 RTP/SAVP 111",
            "a=mid:0",
            "a=label:main-audio",
            "a=candidate:1 1 udp 2130706431 10.1.2.3 51234 typ host",
            "m=video 0 RTP/SAVP 107",
            "a=mid:5",
            "a=label:applicationsharing-video",
            "",
        ]
        .join("\r\n");
        let sections = media_sections(&answer);
        assert_eq!(
            sections,
            vec![
                "audio mid=0 label=main-audio accepted",
                "video mid=5 label=applicationsharing-video REJECTED",
            ]
        );

        // The rule that keeps this loggable at all: the shape travels and the content does
        // not. A candidate is an address of the user's, a fingerprint is a key, and a port
        // is a relay's — so a zero port is stated as the fact it is and nothing else.
        let printed = sections.join(" | ");
        for secret in ["10.1.2.3", "51234", "AA:BB:CC", "3478", "candidate"] {
            assert!(!printed.contains(secret), "{secret} must never reach a log line");
        }

        assert!(media_sections("").is_empty());
    }

    /// The content-sharing session a screen share needs, field for field against the
    /// client's own `j2` — and never carrying a link that would overwrite the call's.
    #[test]
    fn a_content_sharing_modality_asks_to_present_and_names_its_own_callbacks() {
        let payload = content_sharing_payload(&local(), &callbacks(), "session-guid");
        assert_eq!(payload.pointer("/contentSharing/identifier").unwrap(), "session-guid");
        // The client's builder passes `i || null` / `t || null`, and this app has neither: a
        // screen has no subject, and the state is the service's to decide.
        assert_eq!(payload.pointer("/contentSharing/subject").unwrap(), &Value::Null);
        assert_eq!(payload.pointer("/contentSharing/sessionState").unwrap(), &Value::Null);
        // The literal the client sends on a start.
        assert_eq!(payload.pointer("/contentSharing/sequenceNumber").unwrap(), 1);
        // It reaches nobody: a session is asked of the SERVICE, not offered to a person.
        assert_eq!(payload.pointer("/participants/to").unwrap(), &json!([]));
        // Two callbacks for the session's own changes, and two for the modality itself.
        for pointer in [
            "/contentSharing/links/sessionUpdate",
            "/contentSharing/links/sessionEnd",
            "/links/addModalitySuccess",
            "/links/addModalityFailure",
        ] {
            let link = payload.pointer(pointer).and_then(Value::as_str).expect(pointer);
            assert!(link.starts_with("https://"), "{pointer} must be a callback of ours");
        }
        // And no envelope. A wrapped body is refused `400` with `{}` and names nothing —
        // the failure § Joining a meeting cost days to.
        assert!(payload.get("payload").is_none());
    }

    /// A CALL and a JOIN register the SAME conversation callbacks.
    ///
    /// A link missing from one of the two is invisible: both requests are accepted, the
    /// conversation works, and only the action whose answer had nowhere to land silently never
    /// happens. That shipped — the join published no `addModalitySuccess`, so a screen share in
    /// a MEETING could never be granted (the POST answers `{}` and the grant arrives on that
    /// callback), the section was rejected for not coming from the presenter, and two rounds of
    /// debugging went at the SDP instead. A call published it all along, which is why the same
    /// feature measured fine there.
    ///
    /// So the set is compared rather than spelled twice, and `addModalitySuccess` is named on
    /// its own because it is the one this cost.
    #[test]
    fn a_call_and_a_join_register_the_same_conversation_callbacks() {
        let (local, callbacks) = (local(), callbacks());
        let offer = MediaContent::sdp("v=0");
        let call = invitation_payload(&local, &["8:orgid:them".into()], Some("19:t"), &offer, &callbacks);
        let join = join_payload(
            &local,
            &MeetingJoin::from_join_url("https://teams.microsoft.com/meet/12345678?p=abc")
                .expect("a short link"),
            &callbacks,
            Some(&offer),
        );

        let of = |p: &Value| p["conversationRequest"]["links"].clone();
        assert_eq!(of(&call), of(&join), "a call and a join must register the same callbacks");

        for name in ["addModalitySuccess", "addModalityFailure"] {
            assert!(
                of(&join).get(name).and_then(Value::as_str).is_some_and(|u| !u.is_empty()),
                "a join that does not publish {name} can never be granted a screen share"
            );
        }
    }

    /// OFFERING media on the door named for it, and the two envelopes the service wants.
    ///
    /// Measured 2026-09-04, in two refusals that each named their own field:
    /// `StartOutgoingNegotiation` (the envelope), then
    /// `StartOutgoingNegotiation.Links.MediaRenegotiation` (a link the other door never asks
    /// for). With both, the POST is accepted and the capture is retained — where every offer on
    /// `mediaRenegotiation` had its video section zeroed, a camera as much as a screen.
    ///
    /// The statement inside is shared, so the two doors cannot drift; the extra link is added to
    /// the outgoing one ALONE, because a body carrying a field the other door does not know is
    /// how this plane earns a `400` that names nothing.
    #[test]
    fn offering_media_names_its_own_envelope_and_the_link_that_door_requires() {
        let (local, callbacks) = (local(), callbacks());
        let offer = MediaContent::sdp("v=0");
        let mods = [MODALITY_AUDIO, "Video"];

        let outgoing = start_outgoing_negotiation_payload(&local, &offer, &callbacks, &mods);
        let plain = media_offer_payload(&local, &offer, &callbacks, &mods);

        // Each names its own envelope and neither carries the other's.
        assert!(outgoing.get("startOutgoingNegotiation").is_some(), "{outgoing}");
        assert!(outgoing.get("mediaNegotiation").is_none(), "{outgoing}");
        assert!(plain.get("mediaNegotiation").is_some(), "{plain}");
        assert!(plain.get("startOutgoingNegotiation").is_none(), "{plain}");

        // The link this door requires, and which the other must not grow by accident.
        let links = &outgoing["startOutgoingNegotiation"]["links"];
        assert!(
            links.get("mediaRenegotiation").and_then(Value::as_str).is_some_and(|u| !u.is_empty()),
            "the service refuses this door without it: {outgoing}"
        );
        assert!(plain["mediaNegotiation"]["links"].get("mediaRenegotiation").is_none(), "{plain}");

        // THE REAL CLIENT'S OWN LINK SET for this body, and the one it must never grow again.
        // `qH` in `calling-pluginless-<hash>.js` publishes `mediaAnswer` and `rejection` here and
        // nothing else; the ACKNOWLEDGEMENT belongs to the answer body (`jH`), because an offer is
        // acknowledged by nobody. This app sent one for months, and a field the door was never told
        // about is answered by a silently zeroed section rather than by anything that names itself.
        for door in [&plain["mediaNegotiation"], &outgoing["startOutgoingNegotiation"]] {
            let links = door["links"].as_object().expect("a links object");
            assert!(links.contains_key("mediaAnswer"), "{door}");
            assert!(links.contains_key("rejection"), "{door}");
            assert!(
                !links.contains_key("mediaAcknowledgement"),
                "an OFFER publishes no acknowledgement callback — the client's own `qH` does not, \
                 and the ANSWER body is where this app already sends one: {door}"
            );
        }
        // And the ANSWER really does carry it, so the pair above is a placement rule rather than a
        // deletion: `jH` names `mediaAcknowledgement` and only that.
        let answered = media_answer_payload(&local, &offer, &callbacks, &mods);
        assert!(
            answered["mediaAnswer"]["links"].get("mediaAcknowledgement").is_some(),
            "{answered}"
        );

        // And the statement itself is the same one, so the two cannot drift.
        for field in ["sender", "callModalities", "mediaContent"] {
            assert_eq!(
                outgoing["startOutgoingNegotiation"][field], plain["mediaNegotiation"][field],
                "{field} differs between the two doors"
            );
        }
        assert_eq!(outgoing["startOutgoingNegotiation"]["mediaContent"]["blob"], "v=0");
    }

    /// The session's links are read APART from the call's, because it carries a `leave` of its
    /// own and `Links::collect` takes the deepest of a name. Merged in, giving a SHARE up
    /// would have hung the call up instead.
    #[test]
    fn a_sharing_sessions_leave_never_becomes_the_calls_own() {
        let answer = json!({
            "contentSharing": {
                "sessionId": "cs-1",
                "links": { "leave": "https://x/content-sharing-leave", "takeControl": "https://x/tc" }
            },
            // What the call already holds, in the same answer.
            "links": { "leave": "https://x/hang-up" }
        });
        let session = ContentSharing::from_answer("corr-1", &answer);
        assert_eq!(session.correlation_id, "corr-1");
        assert_eq!(session.session_id.as_deref(), Some("cs-1"));
        assert_eq!(session.leave.as_deref(), Some("https://x/content-sharing-leave"));
        // The one that must never be confused for it.
        assert_ne!(session.leave.as_deref(), Some("https://x/hang-up"));

        // An answer that names no way out is read as one: a session this app could not give
        // back is reported rather than remembered.
        let bare = ContentSharing::from_answer("corr-2", &json!({ "contentSharing": {} }));
        assert!(bare.leave.is_none());
    }

    /// The GRANT arrives on a frame, and its shape is not the one a pointer would guess.
    ///
    /// Measured 2026-08-06: the `addModality` POST answers `{}`, and the service POSTs
    /// `addModalitySuccess` with the session's six links. A first reading looked for `/links`
    /// at the top and found nothing, so the app offered its section with no presenter and the
    /// service rejected it — which is why this reads at every depth instead.
    #[test]
    fn the_grant_is_read_out_of_its_frame_at_whatever_depth_it_arrives() {
        // Named after its type, the way `mediaAnswer` is.
        let named = json!({
            "addModalitySuccess": {
                "contentSharingSessionId": "cs-42",
                "links": { "leave": "https://x/cs-leave", "takeControl": "https://x/tc" }
            }
        });
        let session = ContentSharing::from_frame("corr", &named).expect("the grant");
        assert_eq!(session.leave.as_deref(), Some("https://x/cs-leave"));
        assert_eq!(session.session_id.as_deref(), Some("cs-42"));

        // And at the top, which is the shape `contentSharingEnd` really has.
        let flat = json!({ "links": { "leave": "https://x/flat" } });
        assert_eq!(
            ContentSharing::from_frame("corr", &flat).and_then(|s| s.leave).as_deref(),
            Some("https://x/flat")
        );

        // A frame that names no way out is not the grant: this app never holds a session it
        // could not give back.
        assert!(ContentSharing::from_frame("corr", &json!({ "links": {} })).is_none());
        assert!(ContentSharing::from_frame("corr", &json!({})).is_none());
    }

    /// A content type we cannot hand a browser must be refused rather than passed
    /// on: `setRemoteDescription` on a non-SDP blob is an exception in the page.
    #[test]
    fn a_media_content_that_is_not_sdp_is_refused() {
        assert_eq!(
            MediaContent::parse(&json!({ "blob": "x", "contentType": "application/x-ngc" })),
            None
        );
        assert_eq!(MediaContent::parse(&json!({ "blob": "  " })), None);
        // The older label the service still uses is accepted.
        assert!(MediaContent::parse(
            &json!({ "blob": "v=0", "contentType": "application/sdp-ngc-0.5" })
        )
        .is_some());
    }

    #[test]
    fn an_invitation_names_the_person_the_thread_and_our_own_links() {
        let payload = invitation_payload(
            &local(),
            &["8:orgid:her".to_string()],
            Some("19:thread@thread.v2"),
            &MediaContent::sdp("v=0 offer"),
            &callbacks(),
        );
        let invitation = payload.pointer("/callInvitation").expect("an invitation");
        assert_eq!(invitation["callModalities"], json!(["audio"]));
        assert_eq!(invitation["mediaContent"]["blob"], "v=0 offer");
        assert_eq!(invitation["mediaContent"]["contentType"], SDP_CONTENT_TYPE);
        assert!(invitation["links"]["mediaAnswer"]
            .as_str()
            .unwrap()
            .ends_with("/call/mediaAnswer/"));
        assert_eq!(payload.pointer("/participants/to/0/id").unwrap(), "8:orgid:her");
        // A call carries the same capability masks a JOIN is known to be accepted with:
        // same endpoint, same silent refusal, and the 1:1 path has never been rung.
        assert_eq!(payload.pointer("/endpointCapabilities").unwrap(), 73463);
        assert_eq!(payload.pointer("/clientEndpointCapabilities").unwrap(), 63928042);
        assert_eq!(payload.pointer("/capabilities").unwrap(), &Value::Null);
        assert!(payload.pointer("/endpointState").is_some());
        assert!(payload.pointer("/endpointMetadata").is_some());
        assert_eq!(payload.pointer("/participants/from/id").unwrap(), "8:orgid:me");
        assert_eq!(
            payload.pointer("/groupChat/threadId").unwrap(),
            "19:thread@thread.v2"
        );
    }

    /// A GROUP call is the same POST with more people in it: every mri is rung, they share
    /// one thread, and the group-call event messages that put the call line in that thread
    /// are on. Nothing about the body is per-person.
    #[test]
    fn an_invitation_rings_every_person_it_names() {
        let payload = invitation_payload(
            &local(),
            &["8:orgid:her".to_string(), "8:orgid:him".to_string(), "8:orgid:them".to_string()],
            Some("19:group@thread.v2"),
            &MediaContent::sdp("v=0 offer"),
            &callbacks(),
        );
        let to = payload.pointer("/participants/to").unwrap().as_array().unwrap();
        assert_eq!(to.len(), 3);
        assert_eq!(to[2]["id"], "8:orgid:them");
        assert_eq!(payload.pointer("/groupChat/threadId").unwrap(), "19:group@thread.v2");
        // What posts the call line into the thread everybody in it reads.
        assert_eq!(
            payload.pointer("/conversationRequest/properties/enableGroupCallEventMessages").unwrap(),
            &json!(true)
        );
        // And the roster subscription, which is the only way "who is in this call" is ever
        // answered once there is more than one person to answer it for.
        assert!(payload
            .pointer("/conversationRequest/roster/rosterUpdate")
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("/conversation/rosterUpdate/"));
    }

    /// Audio only, everywhere, in every direction. A video m-line would be
    /// negotiated for a camera this app never opens.
    #[test]
    fn every_payload_offers_audio_and_only_audio() {
        let offer = MediaContent::sdp("v=0");
        let invitation =
            invitation_payload(&local(), &["8:orgid:her".into()], None, &offer, &callbacks());
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let joined_audio = join_payload(&local(), &meeting, &callbacks(), Some(&offer));
        assert_eq!(
            joined_audio.pointer("/callInvitation/callModalities").unwrap(),
            &json!(["audio"])
        );
        let acceptance = acceptance_payload(&local(), &offer, &callbacks());
        let answer = media_answer_payload(&local(), &offer, &callbacks(), &[MODALITY_AUDIO]);
        assert_eq!(invitation.pointer("/callInvitation/callModalities").unwrap(), &json!(["audio"]));
        assert_eq!(
            acceptance.pointer("/callAcceptance/acceptedCallModalities").unwrap(),
            &json!(["audio"])
        );
        assert_eq!(answer.pointer("/mediaAnswer/callModalities").unwrap(), &json!(["audio"]));
        let whole = format!("{invitation}{acceptance}{answer}");
        assert!(!whole.contains("video"), "no payload may ever offer video: {whole}");
    }

    #[test]
    fn a_hangup_and_a_rejection_name_who_sent_them() {
        let hangup = hangup_payload(&local());
        assert_eq!(hangup.pointer("/participants/from/id").unwrap(), "8:orgid:me");
        // Both endings, the way the client's own `leaveConversation` sends them. A body
        // that names nobody is refused with a 400, and then Teams still has the user in
        // the meeting while this app does not.
        assert_eq!(hangup.pointer("/conversationTransactionEnd/reason").unwrap(), "noError");
        assert!(hangup.pointer("/callTransactionEnd/code").is_some());
        assert_eq!(
            rejection_payload(&local()).pointer("/callRejection/sender/id").unwrap(),
            "8:orgid:me"
        );
        assert_eq!(mute_payload(&local(), true).pointer("/muteUnmute/muted").unwrap(), true);
    }

    // ---- joining a meeting -------------------------------------------------

    /// The link the calendar already holds carries everything a join needs. This is
    /// the real shape Graph hands back for a meeting on the calendar.
    #[test]
    fn a_calendar_meeting_link_yields_its_thread_and_its_meeting_info() {
        let url = "https://teams.microsoft.com/l/meetup-join/\
                   19%3ameeting_NTk4YzY0MTQt%40thread.v2/0\
                   ?context=%7b%22Tid%22%3a%22af1bbf3d-1111-2222-3333-444455556666%22%2c\
                   %22Oid%22%3a%2299887766-5544-3322-1100-aabbccddeeff%22%7d";
        let join = MeetingJoin::from_join_url(url).expect("a join link");
        assert_eq!(join.thread_id.as_deref(), Some("19:meeting_NTk4YzY0MTQt@thread.v2"));
        // A calendar meeting hangs off no message.
        assert_eq!(join.message_id, "0");
        assert!(!join.is_channel_meeting());
        assert_eq!(join.tenant_id.as_deref(), Some("af1bbf3d-1111-2222-3333-444455556666"));
        // A BARE object id, which is what the captured `meetingInfo.organizerId` is.
        assert_eq!(
            join.organizer_mri.as_deref(),
            Some("99887766-5544-3322-1100-aabbccddeeff")
        );
    }

    /// A meeting inside a CHANNEL is the other shape: the channel's own thread, and the
    /// post the meeting hangs off. Both have to survive the parse, or the join
    /// addresses the channel instead of the meeting in it.
    #[test]
    fn a_channel_meeting_link_keeps_the_message_it_hangs_off() {
        let url = "https://teams.microsoft.com/l/meetup-join/\
                   19%3aabc123%40thread.tacv2/1719400000000?context=%7b%22Tid%22%3a%22t%22%7d";
        let join = MeetingJoin::from_join_url(url).expect("a join link");
        assert_eq!(join.thread_id.as_deref(), Some("19:abc123@thread.tacv2"));
        assert_eq!(join.message_id, "1719400000000");
        assert!(join.is_channel_meeting());
        // A context with no organizer names no `meetingInfo` rather than half of one.
        assert_eq!(join.meeting_info(), None);
    }

    /// A link that is not a meeting join must read as one thing: not a meeting. The
    /// UI decides whether to offer a Join button from exactly this answer.
    #[test]
    fn a_link_that_is_not_a_meeting_join_is_refused() {
        for url in [
            "",
            "https://teams.microsoft.com/l/channel/19%3aabc%40thread.tacv2/General",
            "https://zoom.us/j/123456",
            // The path is right but the thread is not a thread.
            "https://teams.microsoft.com/l/meetup-join/not-a-thread/0",
            "https://teams.microsoft.com/l/meetup-join/",
        ] {
            assert_eq!(MeetingJoin::from_join_url(url), None, "url: {url}");
        }
    }

    /// A join link with no context at all still joins: the thread is what addresses the
    /// meeting, and `meetingInfo` is the service's preference rather than its key.
    #[test]
    fn a_join_link_without_a_context_still_names_the_meeting() {
        let join =
            MeetingJoin::from_join_url("https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0")
                .expect("a join link");
        assert_eq!(join.thread_id.as_deref(), Some("19:meeting_x@thread.v2"));
        assert_eq!(join.tenant_id, None);
        assert_eq!(join.meeting_info(), None);
    }

    /// A meeting reached from its own CHAT: the thread is the whole address, and the body
    /// is the one a context-free long link builds — the same `groupChat` and the same
    /// `"0"`, with nothing invented beside it.
    #[test]
    fn a_meeting_thread_is_a_join_address_on_its_own() {
        let join = MeetingJoin::from_thread_id("19:meeting_abc@thread.v2").expect("a meeting");
        assert_eq!(join.thread_id.as_deref(), Some("19:meeting_abc@thread.v2"));
        assert_eq!(join.message_id, "0");
        assert!(!join.is_channel_meeting());
        // No link was involved, so neither of the two things a link carries is claimed.
        assert_eq!(join.meeting_info(), None);
        assert_eq!(join.meeting_data(), None);
        assert_eq!(join.join_url, "");

        let payload = join_payload(&local(), &join, &callbacks(), None);
        assert_eq!(
            payload.pointer("/groupChat/threadId").unwrap(),
            "19:meeting_abc@thread.v2"
        );
        assert_eq!(payload.pointer("/groupChat/messageId").unwrap(), "0");
        assert!(payload.pointer("/meetingData").is_none());
        assert!(payload.pointer("/meetingInfo").is_none());
        // A join rings nobody, whichever way it was addressed.
        assert!(payload.pointer("/participants/to").is_none());
    }

    /// Only a MEETING thread is a join address. A plain group chat is called instead, and
    /// a channel's thread would address the channel rather than the meeting inside it —
    /// so both read as "nothing to join" rather than as a join to the wrong place.
    #[test]
    fn a_thread_that_is_not_a_meeting_is_no_join_address() {
        for thread in [
            "",
            // An ordinary group chat: 32 hex digits, no `meeting_`.
            "19:21d2695ae8ff4e25ace9c662e5c326cb@thread.v2",
            // A channel.
            "19:abc@thread.tacv2",
            // A one-to-one chat.
            "19:oid1_oid2@unq.gbl.spaces",
            "48:notes",
        ] {
            assert_eq!(MeetingJoin::from_thread_id(thread), None, "thread: {thread}");
        }
    }

    /// The shape the user's OWN meetings have: a meeting code and a passcode, and no
    /// thread at all. The service resolves the thread from the code, so the link is
    /// handed back to it as `meetingData` instead of `groupChat`.
    #[test]
    fn a_short_meeting_link_yields_its_code_and_passcode() {
        let url = "https://teams.microsoft.com/meet/35017215452446?p=4QyEW2wHMvAevXsCVU";
        let join = MeetingJoin::from_join_url(url).expect("a join link");
        assert_eq!(join.thread_id, None);
        assert_eq!(join.meeting_code.as_deref(), Some("35017215452446"));
        assert_eq!(join.passcode.as_deref(), Some("4QyEW2wHMvAevXsCVU"));
        assert_eq!(join.join_url, url);
        assert!(!join.is_channel_meeting());

        let payload = join_payload(&local(), &join, &callbacks(), None);
        // No thread to name, so none is invented.
        assert!(payload.pointer("/groupChat").is_none());
        assert_eq!(payload.pointer("/meetingData/meetingCode").unwrap(), "35017215452446");
        assert_eq!(payload.pointer("/meetingData/passcode").unwrap(), "4QyEW2wHMvAevXsCVU");
        assert_eq!(payload.pointer("/meetingData/meetingUrl").unwrap(), url);
    }

    /// A short link with no passcode still names the meeting, and a code that is not one
    /// is refused rather than sent.
    #[test]
    fn a_short_link_is_checked_before_it_is_trusted() {
        let join = MeetingJoin::from_join_url("https://teams.microsoft.com/meet/12345")
            .expect("a join link");
        assert_eq!(join.meeting_code.as_deref(), Some("12345"));
        assert_eq!(join.passcode, None);
        assert!(join.meeting_data().unwrap().get("passcode").is_none());
        for url in [
            "https://teams.microsoft.com/meet/",
            "https://teams.microsoft.com/meet/not a code",
        ] {
            assert_eq!(MeetingJoin::from_join_url(url), None, "url: {url}");
        }
    }

    /// `conversationType` is NULL for an ordinary join. The client only ever names one
    /// for an emergency call, a cast, a huddle or a consult-and-add — and an invented
    /// value earned a `400 {}` from the conversation service with no explanation.
    #[test]
    fn a_join_names_no_conversation_type_and_carries_the_captured_fields() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let payload = join_payload(&local(), &meeting, &callbacks(), None);
        // The captured request names NO conversationType at all, so neither does this.
        assert!(payload.pointer("/conversationRequest/conversationType").is_none());
        // And the fields it does always carry, each of which a strict service may demand.
        assert_eq!(payload.pointer("/capabilities").unwrap(), &Value::Null);
        assert!(payload.pointer("/endpointCapabilities").is_some());
        assert!(payload.pointer("/endpointState").is_some());
        assert!(payload.pointer("/endpointMetadata").is_some());
    }

    #[test]
    fn a_join_rings_nobody_and_names_the_meeting() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0\
             ?context=%7b%22Tid%22%3a%22tenant%22%2c%22Oid%22%3a%22organizer%22%7d",
        )
        .expect("a join link");
        let payload = join_payload(&local(), &meeting, &callbacks(), None);

        // A join has no `to`: the meeting is already there, and nobody is rung.
        assert!(payload.pointer("/participants/to").is_none());
        assert_eq!(payload.pointer("/participants/from/id").unwrap(), "8:orgid:me");
        assert_eq!(
            payload.pointer("/groupChat/threadId").unwrap(),
            "19:meeting_x@thread.v2"
        );
        // The STRING "0", exactly as the captured request sends it — not null.
        assert_eq!(payload.pointer("/groupChat/messageId").unwrap(), "0");
        assert_eq!(payload.pointer("/meetingInfo/tenantId").unwrap(), "tenant");
        assert_eq!(payload.pointer("/meetingInfo/organizerId").unwrap(), "organizer");
        // And NO media: the join joins the conversation, and audio is a second request
        // (see `add_audio_payload`). A `callInvitation` here is a 400 with no body.
        assert!(payload.pointer("/callInvitation").is_none());
        assert!(!payload.to_string().contains("mediaContent"));
        // The roster is asked to be kept up to date, because that is what "who" means.
        assert!(payload
            .pointer("/conversationRequest/roster/rosterUpdate")
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("/conversation/rosterUpdate/"));
    }

    /// The credentials a calling request carries, and the ones it must NOT. The captured
    /// request authenticates with the ic3 bearer ALONE — sending a skypetoken beside it is
    /// a shape the real client never produces, and it cost three rounds of a `400` that
    /// named nothing.
    #[test]
    fn a_calling_request_carries_one_credential_and_no_api_version() {
        let source = include_str!("calling.rs");
        // The ONE header builder every verb goes through. The relay-token GET below it
        // legitimately sends a skypetoken and an api-version — that is a different service, and
        // the web client's own legacy token fetcher sends exactly those two.
        let start = source.find("fn signal_headers(").expect("signal_headers");
        let end = source[start..].find("\n/// ").map(|at| start + at).unwrap_or(source.len());
        let headers = &source[start..end];
        assert!(
            !headers.contains(".header(\"X-Skypetoken\""),
            "the calling service takes the ic3 bearer alone; a second credential is refused"
        );
        assert!(
            !headers.contains(".header(\"api-version\""),
            "the conversation service is not versioned by that header"
        );
        assert!(headers.contains("ms-teams-region"), "the service routes on the region");
        assert!(headers.contains("ms-teams-ring"), "and on the ring");
        // And nothing BUT that builder sets a header on a signaling request: the transport
        // function adds the body and the bound, never a header of its own.
        let within = source.find("async fn signal_within(").expect("signal_within");
        let within = &source[within..within + source[within..].find("\n}\n").expect("its end")];
        assert!(!within.contains(".header("), "a header set outside signal_headers:\n{within}");
    }

    /// EVERY VERB CARRIES THE SAME HEADERS, built and read back rather than scanned: a PUT or a
    /// DELETE that grew a header of its own — or lost one — is a request shape the service has
    /// never accepted from this app, and the refusal would name nothing. The set is the one the
    /// verified POST has always carried.
    #[test]
    fn every_signaling_verb_carries_the_one_header_set() {
        let http = reqwest::Client::new();
        let names = |method: reqwest::Method| -> Vec<String> {
            let request = signal_headers(
                http.request(method, "https://conv.example/conversation/"),
                "emea",
                "ic3-token",
                "call-1",
            )
            .json(&json!({ "a": 1 }))
            .build()
            .expect("a request");
            let mut names: Vec<String> =
                request.headers().keys().map(|name| name.as_str().to_string()).collect();
            names.sort_unstable();
            names
        };
        let post = names(reqwest::Method::POST);
        assert_eq!(
            post,
            [
                "accept",
                "authorization",
                "content-type",
                "ms-teams-partition",
                "ms-teams-region",
                "ms-teams-ring",
                "x-microsoft-skype-chain-id",
                "x-microsoft-skype-client",
                "x-microsoft-skype-message-id",
                "x-ms-migration",
            ]
        );
        assert_eq!(names(reqwest::Method::PUT), post);
        assert_eq!(names(reqwest::Method::DELETE), post);
        // The values that are not per-request, for the POST the verified calls go out on.
        let request = signal_headers(
            http.post("https://conv.example/conversation/"),
            "emea",
            "ic3-token",
            "call-1",
        )
        .build()
        .expect("a request");
        let value = |name: &str| request.headers()[name].to_str().unwrap().to_string();
        assert_eq!(value("authorization"), "Bearer ic3-token");
        assert_eq!(value("x-microsoft-skype-chain-id"), "call-1");
        assert_eq!(value("x-microsoft-skype-client"), CLIENT_IDENTITY);
        assert_eq!(value("ms-teams-region"), "emea");
        assert_eq!(value("ms-teams-partition"), teams_partition("emea"));
        assert_eq!(value("ms-teams-ring"), "general");
        assert_eq!(value("x-ms-migration"), "True");
        assert_eq!(value("accept"), "*/*");
    }

    /// A stand-in calling service on loopback, on a thread of its own: it reads one request and
    /// answers `reply` after `delay` — or NEVER answers when `reply` is `None`, holding the
    /// connection open, which is exactly the hang a transport bound exists for.
    fn stub_service(reply: Option<String>, delay: std::time::Duration) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else { return };
            let mut request = [0u8; 16 * 1024];
            let _ = socket.read(&mut request);
            std::thread::sleep(delay);
            match reply {
                Some(reply) => {
                    let _ = socket.write_all(reply.as_bytes());
                }
                None => std::thread::sleep(std::time::Duration::from_secs(120)),
            }
        });
        format!("http://{address}/conversation/")
    }

    fn stub_session() -> Session {
        Session {
            skypetoken: "skype".into(),
            region: "emea".into(),
            gtms: json!({}),
            self_name: String::new(),
            self_mri: String::new(),
        }
    }

    fn http_reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
             x-microsoft-skype-diagnostics: golden-reason\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// A REQUEST NOBODY ANSWERS IS BOUNDED. Before this the backend's client carried no timeout,
    /// so a signaling request the service never answered held its caller for ever — and the
    /// frame handler awaits its acknowledgements in line, so one hung acknowledgement stopped
    /// every later frame of the call. Driven for real against a socket that accepts and says
    /// nothing, and said as a LOST answer, never as a refusal.
    #[tokio::test]
    async fn a_signaling_request_nobody_answers_is_bounded_and_said_as_lost() {
        let url = stub_service(None, std::time::Duration::ZERO);
        let began = std::time::Instant::now();
        let error = signal_within(
            &reqwest::Client::new(),
            reqwest::Method::POST,
            &url,
            std::time::Duration::from_millis(300),
            &stub_session(),
            "ic3",
            "call-1",
            &json!({ "a": 1 }),
        )
        .await
        .expect_err("a request with no answer must end");
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "{:?}", began.elapsed());
        let said = format!("{error:#}");
        assert!(said.contains("no answer to a signaling POST"), "{said}");
        assert!(said.contains("within 0.3 s"), "{said}");
        assert!(error.downcast_ref::<SignalRefused>().is_none(), "silence is not a refusal");
    }

    /// A LOST ANSWER NEVER PRINTS ITS ADDRESS'S QUERY. reqwest's own error carries the whole URL
    /// and prints it, and a signaling link can carry a token in its query — so the sentence a
    /// timeout reads as, chain and all, holds the redacted address and nothing else.
    #[tokio::test]
    async fn a_lost_answer_never_prints_the_query_of_its_address() {
        let url = format!("{}?i=secret-token", stub_service(None, std::time::Duration::ZERO));
        let error = signal_within(
            &reqwest::Client::new(),
            reqwest::Method::POST,
            &url,
            std::time::Duration::from_millis(200),
            &stub_session(),
            "ic3",
            "call-1",
            &json!({}),
        )
        .await
        .expect_err("a request with no answer must end");
        let said = format!("{error:#}");
        assert!(said.contains("no answer to a signaling POST"), "{said}");
        assert!(!said.contains("secret-token"), "the query leaked into the error: {said}");
        // And the same for a connection that is REFUSED rather than silent.
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
            let address = listener.local_addr().expect("its address");
            drop(listener);
            format!("http://{address}/conversation/?i=secret-token")
        };
        let error = signal_within(
            &reqwest::Client::new(),
            reqwest::Method::POST,
            &closed,
            std::time::Duration::from_secs(2),
            &stub_session(),
            "ic3",
            "call-1",
            &json!({}),
        )
        .await
        .expect_err("nothing listens there");
        let said = format!("{error:#}");
        assert!(said.contains("calling: signaling POST to http://127.0.0.1:"), "{said}");
        assert!(!said.contains("secret-token"), "the query leaked into the error: {said}");
    }

    /// A SUCCESS WHOSE BODY WAS CUT OFF IS STILL A SUCCESS. The service did what was asked and only
    /// its words were lost, so it reads as the empty answer it always has — never as a failure,
    /// which would end a placed call here while the callee's phone rang and leave the frames that
    /// can still name its links nowhere to land.
    #[tokio::test]
    async fn a_success_whose_body_was_cut_off_reads_as_an_empty_answer() {
        // Declares 200 bytes, sends a dozen, and closes.
        let cut = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 200\r\n\
                   connection: close\r\n\r\n{\"links\":{\"hang"
            .to_string();
        let answered = signal_within(
            &reqwest::Client::new(),
            reqwest::Method::POST,
            &stub_service(Some(cut), std::time::Duration::ZERO),
            std::time::Duration::from_secs(5),
            &stub_session(),
            "ic3",
            "call-1",
            &json!({}),
        )
        .await
        .expect("a 2xx is a success whatever became of its body");
        assert_eq!(answered, Value::Null);
    }

    /// A 4xx is a REFUSAL, typed so a caller can fall back on it; a 5xx and a lost answer are
    /// not, because neither says the request was turned down. The sentence a refusal reads as is
    /// the one it always was — the service's own words and headers — since the page and the
    /// journal both read it.
    #[tokio::test]
    async fn a_refusal_is_typed_and_reads_as_it_always_did() {
        let http = reqwest::Client::new();
        let send = |reply: String| {
            let http = http.clone();
            async move {
                signal_within(
                    &http,
                    reqwest::Method::POST,
                    &stub_service(Some(reply), std::time::Duration::ZERO),
                    std::time::Duration::from_secs(5),
                    &stub_session(),
                    "ic3",
                    "call-1",
                    &json!({}),
                )
                .await
            }
        };
        let refused = send(http_reply("403 Forbidden", "{\"subCode\":4005}")).await.unwrap_err();
        let typed = refused.downcast_ref::<SignalRefused>().expect("a 4xx is a refusal");
        assert_eq!(typed.status, 403);
        let said = refused.to_string();
        assert!(said.starts_with("calling: 403 Forbidden from http://127.0.0.1:"), "{said}");
        assert!(said.contains("— {\"subCode\":4005} [x-microsoft-skype-diagnostics: golden-reason]"), "{said}");

        let failed = send(http_reply("503 Service Unavailable", "")).await.unwrap_err();
        assert!(failed.downcast_ref::<SignalRefused>().is_none(), "a 5xx says no more than silence");
        assert!(failed.to_string().starts_with("calling: 503 Service Unavailable from"), "{failed}");

        let answered = send(http_reply("200 OK", "{\"links\":{\"hangup\":\"https://h/\"}}")).await;
        assert_eq!(answered.unwrap(), json!({ "links": { "hangup": "https://h/" } }));
        assert_eq!(send(http_reply("200 OK", "")).await.unwrap(), Value::Null);
    }

    /// The classes are the real client's own bounds or above them, and their ORDER is the point:
    /// a creating request is given the longest, because its answer is the only address of a leg
    /// the service may already have made.
    #[test]
    fn the_signaling_classes_are_the_clients_own_bounds() {
        let secs = |class: SignalClass| class.timeout().as_secs();
        // `addModalityTimeoutSec:65`, `mediaAnswerTimeoutSec:35`, `fetchTimeoutMs:1e4`.
        assert_eq!(secs(SignalClass::Create), 65);
        assert_eq!(secs(SignalClass::Answer), 35);
        assert_eq!(secs(SignalClass::End), 10);
        assert_eq!(secs(SignalClass::Shutdown), 4);
        assert!(secs(SignalClass::Create) > secs(SignalClass::Answer));
        assert!(secs(SignalClass::Answer) > secs(SignalClass::End));
        assert!(secs(SignalClass::End) > secs(SignalClass::Shutdown));
    }

    /// The journal names the address a call publishes its links on, because a refusal
    /// that says nothing makes that address the first thing to compare. It names the
    /// HOST and the flavour and never the allocation id, which routes frames to us.
    #[test]
    fn a_logged_surl_keeps_its_host_and_drops_its_id() {
        let surl = "https://pub-ent-plce-03-f.trouter.teams.microsoft.com:3443/v4/f/SBjmODent0m1dNwsgcTbeQ/";
        let shape = surl_shape(surl);
        assert_eq!(
            shape,
            "https://pub-ent-plce-03-f.trouter.teams.microsoft.com:3443/v4/f/…/"
        );
        assert!(!shape.contains("SBjmODent0m1dNwsgcTbeQ"), "the id is a key: {shape}");
        // An allocate endpoint carries no id at all, and stays whole.
        assert_eq!(
            surl_shape("https://go-eu.trouter.teams.microsoft.com/v4/a"),
            "https://go-eu.trouter.teams.microsoft.com/v4/a"
        );
        // Anything that is not a trouter address is printed as it is.
        assert_eq!(surl_shape("https://example.test/x"), "https://example.test/x");
    }

    /// Every way this module reaches the calling service is one the automation guard refuses
    /// from a cargo example (`.claude/hooks/guard-live-automation.sh`, rule 1d-quater). The guard
    /// names the senders and matches the bodies by their `*_payload` shape, on the other side of a
    /// process boundary — so a sender added here and forgotten there is an example that can ring
    /// somebody or hang a call up with nothing to stop it, and only this test can notice.
    ///
    /// It pins ONE direction, like `every_gated_method_is_named_in_the_automation_guard`: a name
    /// the guard keeps for a sender that does not exist yet blocks nothing real.
    #[test]
    fn every_calling_sender_is_named_in_the_example_guard() {
        let hook = include_str!("../.claude/hooks/guard-live-automation.sh");
        let alternation = |name: &str| -> Vec<String> {
            let start = format!("{name}='(");
            let line = hook
                .lines()
                .find_map(|line| line.strip_prefix(start.as_str()))
                .unwrap_or_else(|| panic!("the guard defines no {name}"));
            let inner = line.strip_suffix(")'").expect("one alternation on one line");
            inner.split('|').map(str::to_string).collect()
        };
        let reach = alternation("CALLING_REACH");
        let ring = alternation("CALLING_RING");
        // The bodies by shape, so a builder written tomorrow is covered with no edit to the hook.
        assert!(reach.iter().any(|alt| alt == "[A-Za-z0-9_]*_payload"), "{reach:?}");
        // The calls that ring a person are refused whatever an example pins.
        for name in ["place_call", "invitation_payload"] {
            assert!(ring.iter().any(|alt| alt == name), "{name} is not in CALLING_RING: {ring:?}");
        }

        // Every function of this module that SENDS: each `pub async fn` reaches the network, and
        // each one but the relay credentials reaches the calling service. The relay read fetches
        // TURN credentials and publishes nothing about the user, so it is named as a read.
        const READS: &[&str] = &["fetch_relay_credentials"];
        let source = include_str!("calling.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        let senders: Vec<&str> = code
            .lines()
            .filter_map(|line| line.strip_prefix("pub async fn "))
            .map(|rest| rest.split(['(', '<']).next().unwrap_or_default())
            .collect();
        assert!(senders.contains(&"post_signal"), "the scan found no senders: {senders:?}");
        for sender in senders {
            if READS.contains(&sender) {
                continue;
            }
            assert!(
                reach.iter().any(|alt| alt == sender),
                "calling::{sender} reaches the calling service, but the example guard does not \
                 name it in CALLING_REACH — an example could call it with nothing to stop it"
            );
        }
        // And every body builder really has the shape the guard matches it by: a public function
        // that answers a bare `Value` is a body, so it ends in `_payload` — or it is a FRAGMENT
        // that only ever travels inside one, which reaches nobody on its own.
        // `start_endpoint_state` is the `endpointState` the place and join bodies carry, and the
        // seed a call they start publishes its mute over — it never travels on its own.
        const FRAGMENTS: &[&str] = &["media_controller_content", "start_endpoint_state"];
        let mut bodies = 0;
        for at in code.match_indices("\npub fn ").map(|(at, _)| at + "\npub fn ".len()) {
            let signature = code[at..].split('{').next().unwrap_or_default();
            if !signature.trim_end().ends_with(") -> Value") {
                continue;
            }
            let name = signature.split(['(', '<']).next().unwrap_or_default();
            bodies += 1;
            assert!(
                name.ends_with("_payload") || FRAGMENTS.contains(&name),
                "calling::{name} builds a body but is not named `*_payload`, so the example guard \
                 does not see it — rename it, or name it as a fragment here if it never travels alone"
            );
        }
        assert!(bodies >= 10, "the scan found {bodies} body builders, which is not this module");
    }

    /// NO body carries a `payload` envelope, and this is the test that keeps it that way.
    ///
    /// The web client's own builders DO produce `{payload: {…}}`, so a reader of that
    /// bundle copies the envelope in good faith — this app did, in every one of its eight
    /// bodies. But the envelope belongs to the SDK's request OBJECT, not to the protocol:
    /// its transport serialises `s.payload`, so what reaches the wire is the contents. A
    /// wrapped body is refused with `400` and an empty response, which cost several
    /// rounds of guessing to find.
    #[test]
    fn no_body_carries_the_sdk_request_envelope() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let offer = MediaContent::sdp("v=0\r\n");
        let bodies = [
            (
                "invitation",
                invitation_payload(&local(), &["8:orgid:her".into()], None, &offer, &callbacks()),
            ),
            ("join", join_payload(&local(), &meeting, &callbacks(), None)),
            ("join with audio", join_payload(&local(), &meeting, &callbacks(), Some(&offer))),
            ("acceptance", acceptance_payload(&local(), &offer, &callbacks())),
            ("media answer", media_answer_payload(&local(), &offer, &callbacks(), &[MODALITY_AUDIO])),
            ("hangup", hangup_payload(&local())),
            ("rejection", rejection_payload(&local())),
            ("mute", mute_payload(&local(), true)),
            ("endpoint state", endpoint_state_payload(&local(), &next_endpoint_state(&serde_json::Map::new(), true, 1))),
        ];
        for (what, body) in bodies {
            assert!(
                body.get("payload").is_none(),
                "the {what} body wraps itself in the SDK's envelope"
            );
            // And it is a real body, not an empty object that trivially passes.
            assert!(
                body.as_object().is_some_and(|o| !o.is_empty()),
                "the {what} body is empty"
            );
        }
    }

    /// A `callAcceptance` carries the answer, and for a meeting join it is the ONLY frame
    /// that does — the service accepts and answers in one. Reading only `mediaAnswer` left
    /// the page holding an offer nothing replied to.
    #[test]
    fn an_acceptance_carries_the_answer_too() {
        let frame = json!({
            "callAcceptance": {
                "acceptedBy": { "id": "8:orgid:her" },
                "mediaContent": { "blob": "v=0 answer", "contentType": SDP_CONTENT_TYPE },
            }
        });
        let answer = media_answer_from_frame(&frame).expect("the answer in the acceptance");
        assert_eq!(answer.blob, "v=0 answer");
        // And the older shape still reads, wrapped or not.
        let wrapped = json!({ "_decoded": { "mediaAnswer": {
            "mediaContent": { "blob": "v=0 plain", "contentType": SDP_CONTENT_TYPE } } } });
        assert_eq!(media_answer_from_frame(&wrapped).unwrap().blob, "v=0 plain");
    }

    /// The service ends a call it accepted when nobody acknowledges the acceptance —
    /// `Call Controller timed out while waiting for acknowledgement`, 30 s after a join
    /// that looked perfect. So the link is read from the acceptance's OWN links, and the
    /// body publishes the seven the real client publishes.
    #[test]
    fn an_acceptance_is_acknowledged_on_its_own_link() {
        let frame = json!({
            "callAcceptance": {
                "links": {
                    "acknowledgement": "https://cc/ack",
                    "mediaRenegotiation": "https://cc/reneg",
                }
            }
        });
        assert_eq!(acceptance_acknowledgement_link(&frame), Some("https://cc/ack"));
        // A frame that names none is not an error: nothing is owed to a service that
        // asked for nothing.
        assert_eq!(acceptance_acknowledgement_link(&json!({ "callAcceptance": {} })), None);

        let body = acceptance_acknowledgement_payload(&callbacks());
        let links = body.pointer("/callAcceptanceAcknowledgement/links").expect("the links");
        for name in [
            "mediaRenegotiation",
            "transfer",
            "replacement",
            "balanceUpdate",
            "retargetCompletion",
            "controlVideoStreaming",
            "updateMediaDescriptions",
        ] {
            let link = links.get(name).and_then(Value::as_str).unwrap_or_default();
            assert!(link.contains("callAgent"), "{name} is not one of our callbacks: {link}");
        }
        // It carries no media and claims nothing about the answer.
        assert!(!body.to_string().contains("mediaContent"));
        assert!(body.get("payload").is_none(), "no SDK envelope on the wire");
    }

    #[test]
    fn a_region_names_its_partition() {
        // The one observation this is built from: region `fr` lives in partition `fr01`.
        assert_eq!(teams_partition("fr"), "fr01");
    }

    /// The capability masks are COPIED from a request the service accepted, not computed.
    /// A reader who replaces them with a clean 0 gets a `400` with an empty body and no
    /// way to tell why — which is exactly the round this cost.
    #[test]
    fn the_join_repeats_the_capability_masks_a_working_client_sent() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let payload = join_payload(&local(), &meeting, &callbacks(), None);
        assert_eq!(payload.pointer("/endpointCapabilities").unwrap(), 73463);
        assert_eq!(payload.pointer("/clientEndpointCapabilities").unwrap(), 63928042);
        assert_eq!(
            payload.pointer("/endpointMetadata/holographicCapabilities").unwrap(),
            3
        );
    }

    /// The microphone travels WITH the join, in one POST, and it rings nobody.
    ///
    /// Reading `addModality` as the second half of a join is what earned
    /// `subCode 5021 — no modality blob in the request`: that link grows a GROUP modality
    /// on a 1:1 call and its body carries no media at all. The client's own builder puts
    /// the offer in a `callInvitation` beside the conversation request, and posts once.
    #[test]
    fn a_join_carries_the_offer_and_rings_nobody() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0",
        )
        .unwrap();
        let offer = MediaContent::sdp("v=0 offer");
        let payload = join_payload(&local(), &meeting, &callbacks(), Some(&offer));
        let invitation = payload.pointer("/callInvitation").expect("an invitation");
        assert_eq!(invitation["callModalities"], json!(["audio"]));
        assert_eq!(invitation["mediaContent"]["blob"], "v=0 offer");
        assert_eq!(invitation["mediaContent"]["contentType"], SDP_CONTENT_TYPE);
        // The conversation request travels in the SAME body, which is the whole point.
        assert!(payload.pointer("/conversationRequest/roster").is_some());
        assert!(payload.pointer("/groupChat/threadId").is_some());
        // Nobody to ring: a meeting's roster already exists.
        assert!(payload.pointer("/participants/to").is_none());
        assert!(!payload.to_string().contains("video"));
        // And a join for the roster alone carries no media at all — that is the shape
        // the pre-join capture pins.
        let roster_only = join_payload(&local(), &meeting, &callbacks(), None);
        assert!(roster_only.pointer("/callInvitation").is_none());
    }

    #[test]
    fn a_channel_meeting_join_carries_the_message_id() {
        let meeting = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3aabc%40thread.tacv2/1719400000000",
        )
        .expect("a join link");
        let payload = join_payload(&local(), &meeting, &callbacks(), None);
        assert_eq!(payload.pointer("/groupChat/messageId").unwrap(), "1719400000000");
    }

    /// The lobby is a state of its own, not a failure and not a connection: the user is
    /// waiting to be let in, and the same call continues when somebody admits them.
    #[test]
    fn the_lobby_is_read_from_either_spelling() {
        assert_eq!(
            lobby_state_in_frame(&json!({ "callState": "ConnectedForRosterOnly" })),
            Some(LobbyState::Waiting)
        );
        assert_eq!(
            lobby_state_in_frame(&json!({ "_decoded": { "rosterUpdate": { "participants": [
                { "id": "8:orgid:me", "state": "Lobby" }
            ] } } })),
            Some(LobbyState::Waiting)
        );
        assert_eq!(
            lobby_state_in_frame(&json!({ "callState": "Connected" })),
            Some(LobbyState::Admitted)
        );
        // A frame that says nothing about it must not be read as either.
        assert_eq!(lobby_state_in_frame(&json!({ "callEnd": { "code": 0 } })), None);
    }

    /// The shape the TENANT sends, measured (NATIVE-CALLING.md § 10.2): the body is the
    /// roster, `participants` is keyed by mri, the name is under `details`, and the state is
    /// `active` / `inactive`.
    ///
    /// This test used to assert an invented shape — an array of people under a
    /// `rosterUpdate` key, each carrying `id` and `displayName` — and it passed for weeks
    /// while `roster_in_frame` returned `None` on every real frame. A fixture nobody
    /// measured is a test of the fixture.
    #[test]
    fn a_roster_frame_is_read_in_the_shape_the_tenant_really_sends() {
        let roster = roster_in_frame(&json!({
            "type": "Delta",
            "sequenceNumber": 26,
            "participantCounts": { "active": 2 },
            "participants": {
                "8:orgid:her": {
                    "details": { "displayName": "Her" },
                    "state": "active",
                    "endpoints": { "guid-1": { "call": { "mediaStreams": [
                        { "type": "audio", "label": "main-audio", "sourceId": 2677,
                          "direction": "sendrecv" }
                    ] } } },
                },
                "8:orgid:him": { "details": { "displayName": "Him" }, "state": "inactive" },
                // A resource that is not a person keeps out of the list.
                "19:meeting_x@thread.v2": { "details": { "displayName": "the meeting" } },
            }
        }))
        .expect("a roster");
        assert!(roster.delta, "the frame said Delta, and that decides whether it merges");
        assert_eq!(roster.members.len(), 2);
        assert_eq!(roster.members[0].display_name, "Her");
        assert!(roster.members[0].present);
        assert!(!roster.members[1].present, "an inactive person has left the meeting");
        // Not a roster frame at all.
        assert_eq!(roster_in_frame(&json!({ "callEnd": {} })), None);
    }

    /// A participant's published streams, in the nesting the tenant really sends: an
    /// `endpoints` OBJECT keyed by endpoint id, and the streams under that endpoint's
    /// `call`. The values are one real frame's, taken while the user shared their screen.
    #[test]
    fn a_roster_carries_the_source_ids_a_subscription_is_addressed_by() {
        let roster = roster_in_frame(&json!({
            "type": "Delta",
            "participants": { "8:orgid:her": {
                "details": { "displayName": "Her" },
                "state": "active",
                "endpoints": {
                    "guid-phone": { "call": { "mediaStreams": [
                        { "type": "audio", "label": "main-audio", "sourceId": 2677,
                          "direction": "sendrecv", "serverMuted": false }
                    ] } },
                    "guid-laptop": { "call": { "mediaStreams": [
                        { "type": "audio", "label": "main-audio", "sourceId": 2462,
                          "direction": "sendrecv", "serverMuted": false },
                        // Their camera is OFF: the section exists and points the other way.
                        { "type": "video", "label": "main-video", "sourceId": 2463,
                          "direction": "recvonly" },
                        // And their screen is on. `type` follows the LABEL here, not the
                        // m-line kind — it is not "video".
                        { "type": "applicationsharing-video",
                          "label": "applicationsharing-video", "sourceId": 2473,
                          "direction": "sendonly", "ordinal": 1 },
                        { "type": "data", "label": "data", "sourceId": 2474,
                          "direction": "sendrecv" },
                    ] } },
                },
            } }
        }))
        .expect("a roster");
        let her = &roster.members[0];
        // Every endpoint's streams, flattened: a subscription names a source id and nothing
        // else, so which endpoint published it is not information this app keeps. The order
        // is by endpoint id (the frame keys them, and a JSON object is read in key order),
        // which is stable frame to frame without being meaningful — hence the sort.
        let mut ids: Vec<i64> = her.streams.iter().map(|s| s.source_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![2462, 2463, 2473, 2474, 2677]);
        let sharing: Vec<i64> =
            her.streams.iter().filter(|s| s.is_shared_screen()).map(|s| s.source_id).collect();
        assert_eq!(sharing, vec![2473], "the shared screen is the sendonly sharing label");
        assert!(
            !her.streams.iter().any(RosterStream::is_camera),
            "a camera nobody turned on is recvonly, and must not be offered as a tile"
        );
        // A stream with no source id cannot be asked for, so it is not kept.
        let none = roster_in_frame(&json!({
            "participants": { "8:orgid:him": { "endpoints": { "g": { "call": {
                "mediaStreams": [{ "type": "video", "label": "main-video" }]
            } } } } }
        }))
        .expect("a roster");
        assert!(none.members[0].streams.is_empty());
    }

    /// The renegotiation the service makes ON ITS OWN, which is how a shared screen
    /// arrives (NATIVE-CALLING.md § 10.3a). It is told from an answer by the SDP's own
    /// `a=setup:actpass`, and reading it off a LINK instead is what made a shared screen
    /// invisible — see the test below, which is the defect this one's discriminator was
    /// changed for.
    #[test]
    fn a_media_renegotiation_is_read_as_an_offer_and_never_as_an_answer() {
        let frame = json!({
            "mediaNegotiation": {
                "callModalities": ["audio", "ScreenViewer"],
                "mediaContent": { "blob": "v=0\r\nm=video 3481 RTP/SAVP 107\r\na=setup:actpass\r\n",
                                  "contentType": SDP_CONTENT_TYPE },
                "links": { "mediaAnswer": "https://x/answer", "rejection": "https://x/no" },
            },
            "debugContent": {},
        });
        let offered = media_renegotiation_from_frame(&frame).expect("a renegotiation");
        assert_eq!(offered.answer_link, "https://x/answer");
        assert_eq!(offered.reject_link.as_deref(), Some("https://x/no"));
        assert!(offered.offer.blob.contains("m=video"));
        assert_eq!(offered.modalities, vec!["audio", "ScreenViewer"]);
        // An ANSWER to something we offered is not one, on the plain path too.
        let answer = json!({
            "mediaAnswer": { "mediaContent": { "blob": "v=0\r\na=setup:active\r\n",
                                               "contentType": SDP_CONTENT_TYPE } }
        });
        assert_eq!(media_renegotiation_from_frame(&answer), None);
        assert!(media_answer_from_frame(&answer).is_some());
    }

    /// **An ANSWER that carries an answer LINK is still an answer, and this is why no camera
    /// has ever been seen.**
    ///
    /// `startOutgoingNegotiation` is REQUIRED to publish a `mediaRenegotiation` callback of its
    /// own — the service refused the POST by name until it did (§ 8b) — and it then answers our
    /// camera offer on it, naming where the next step goes. Classified by the presence of a
    /// `links.mediaAnswer`, that answer read as a fresh OFFER: the page applied it as one, and in
    /// `have-local-offer` Chrome implicitly ROLLED BACK the offer the camera was in. The section
    /// was never negotiated, no `mediaAnswer` frame was ever counted (the measured run held
    /// `{"/call/mediaRenegotiation/":2}` and no `mediaAnswer` at all), and nothing reported it.
    #[test]
    fn an_answer_is_not_a_renegotiation_however_many_links_it_carries() {
        // The shape the service really posts to our own `mediaRenegotiation` callback: a
        // `mediaNegotiation` body, an answer link beside it, and an ANSWER in the blob.
        let frame = json!({
            "mediaNegotiation": {
                "callModalities": ["audio", "Video"],
                "mediaContent": {
                    "blob": "v=0\r\nm=audio 3478 RTP/SAVP 111\r\na=setup:passive\r\n\
                             m=video 3481 RTP/SAVP 107\r\na=setup:passive\r\n",
                    "contentType": SDP_CONTENT_TYPE,
                },
                "links": { "mediaAnswer": "https://x/answer", "rejection": "https://x/no" },
            },
        });
        assert_eq!(
            media_renegotiation_from_frame(&frame),
            None,
            "an answer read as an offer is applied to the connection as one, and the browser \
             rolls our own pending offer back under it — which loses the camera in silence"
        );
        let answer = media_answer_from_frame(&frame).expect("the answer, on the same body");
        assert!(answer.blob.contains("m=video"));
    }

    /// EVERY body that carries an SDP tells the media controller where to reach us.
    ///
    /// The real client sends `clientContentForMediaController` on its call invitation, its join,
    /// its acceptance and every media answer (`getClientUrls`, gated on `isWebRtcCall`), and this
    /// app sent it on none of them — which is the leading candidate for a subscription the service
    /// ACCEPTS and then never sends a byte on. Four bodies is four chances to forget one, so the
    /// rule is checked over all four at once.
    #[test]
    fn every_body_that_carries_an_sdp_names_where_the_media_controller_reaches_us() {
        let (local, callbacks) = (local(), callbacks());
        let media = MediaContent::sdp("v=0");
        let meeting = MeetingJoin {
            thread_id: Some("19:meeting_x@thread.v2".into()),
            message_id: "0".into(),
            tenant_id: None,
            organizer_mri: None,
            meeting_code: None,
            passcode: None,
            join_url: "https://teams.microsoft.com/l/meetup-join/x/0".into(),
        };
        for (name, body) in [
            ("invitation", invitation_payload(&local, &["8:orgid:her".into()], None, &media, &callbacks)),
            ("join", join_payload(&local, &meeting, &callbacks, Some(&media))),
            ("acceptance", acceptance_payload(&local, &media, &callbacks)),
            ("attach", attach_payload(&local, &media, &callbacks)),
            ("media answer", media_answer_payload(&local, &media, &callbacks, &[MODALITY_AUDIO])),
        ] {
            // Wherever the body puts it — inside `callInvitation`, inside `callAcceptance`, at the
            // root of an `attach` — the statement is one object and it is found by NAME.
            let mut found = None;
            walk_for_key(&body, "clientContentForMediaController", &mut found);
            let content = found
                .unwrap_or_else(|| panic!("{name} names no clientContentForMediaController: {body}"));
            for link in ["controlVideoStreaming", "csrcInfo"] {
                assert!(
                    content.get(link).and_then(Value::as_str).is_some_and(|u| u.contains(CALL_AGENT)),
                    "{name} does not publish {link} as a callback of ours: {content}"
                );
            }
            // `dominantSpeakerInfo` is the client's third and is behind a config flag; nothing here
            // reads a dominant speaker, and a link nothing posts to goes stale.
            assert!(content.get("dominantSpeakerInfo").is_none(), "{name}: {content}");
        }
    }

    /// The first value of a key at any depth. The bodies nest it differently and the rule is about
    /// the statement rather than about where one body happens to put it.
    fn walk_for_key(value: &Value, key: &str, out: &mut Option<Value>) {
        if out.is_some() {
            return;
        }
        match value {
            Value::Object(map) => {
                if let Some(found) = map.get(key) {
                    *out = Some(found.clone());
                    return;
                }
                for child in map.values() {
                    walk_for_key(child, key, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk_for_key(item, key, out);
                }
            }
            _ => {}
        }
    }

    /// A renegotiation refused for GLARE says so in the client's own words.
    ///
    /// The three values are COPIED out of its `CALL_END_CODE` / `CALL_END_SUB_CODE` /
    /// `CALL_END_PHRASE` tables and not one of them is derivable from its name — guessing the
    /// sub-code from the neighbouring media errors gives the wrong number, and guessing the phrase
    /// from `RENEGOTIATION_IN_PROGRESS` gives `RenegotiationInProgress` where the table says
    /// `NegotiationIsInProgress`. A code the service does not recognise is how this plane earns a
    /// refusal that names nothing, so the values are pinned rather than trusted to a reader.
    #[test]
    fn a_glare_refusal_is_stated_in_the_clients_own_code_and_phrase() {
        let body = media_glare_rejection_payload(&local());
        let failure = &body["mediaNegotiationFailure"];
        assert_eq!(failure["code"], json!(491));
        assert_eq!(failure["subCode"], json!(3118));
        assert_eq!(failure["phrase"], json!("NegotiationIsInProgress"));
        // WHO is refusing. The service refuses a body that names nobody, which is what every other
        // payload here already carries a sender for.
        assert_eq!(failure["sender"], local().json());
        // And it is the same envelope the service POSTS to us on its own failure callback, which is
        // what makes the pair readable in both directions.
        assert!(body.get("mediaNegotiation").is_none(), "{body}");
    }

    /// The discriminator itself, and the trap it is written to avoid.
    #[test]
    fn an_offer_is_told_from_an_answer_by_its_own_setup_role() {
        assert!(sdp_is_offer("v=0\r\na=setup:actpass\r\n"));
        assert!(!sdp_is_offer("v=0\r\na=setup:active\r\n"));
        assert!(!sdp_is_offer("v=0\r\na=setup:passive\r\n"));
        // Read off the ABSENCE of `actpass`, never the presence of a role — the lesson
        // `web/src/lib/ms-sdp.ts`'s own `isAnswer` carries: an OFFER whose rejected section
        // states a role of its own is still an offer, and scanning for `active` called it an
        // answer.
        assert!(sdp_is_offer(
            "v=0\r\nm=audio 3478 RTP/SAVP 111\r\na=setup:actpass\r\n\
             m=video 0 RTP/SAVP 107\r\na=setup:active\r\n"
        ));
        // A blob that states no role at all is not an offer: nothing may be applied as one on
        // the strength of a line that is missing, and every real description carries one.
        assert!(!sdp_is_offer("v=0\r\nm=audio 3478 RTP/SAVP 111\r\n"));
    }

    /// Which of our own callbacks a frame came in on, without the session key in front of it.
    #[test]
    fn a_callback_is_named_by_its_own_tail_and_never_by_the_surl() {
        assert_eq!(
            callback_path("https://go-eu.trouter.teams.microsoft.com/v3/c/x/callAgent/s/c1/call/mediaAnswer/"),
            "call/mediaAnswer"
        );
        assert_eq!(callback_path("https://x/callAgent/s/c1/conversation/rosterUpdate/"), "conversation/rosterUpdate");
        // A query is the service's own, and it carries an ADDRESS on the forked leg (§ 8a) — so it
        // is cut before the path is read rather than printed as part of the door.
        assert_eq!(callback_path("https://x/callAgent/s/c1/call/mediaAnswer/?i=1.2.3.4"), "call/mediaAnswer");
        assert_eq!(callback_path("nothing"), "?");
    }

    /// The session a frame is addressed to is the segment our own link builder put after
    /// `callAgent/` — read off a link this crate built, so the two cannot drift apart.
    #[test]
    fn a_frame_names_the_signaling_session_its_callback_was_built_for() {
        let base = CallbackBase {
            surl: "https://trouter.example/v4/f/abc/".into(),
            session_id: "8b0f3c2e-session".into(),
            cause_id: "0a1b2c3d".into(),
        };
        assert_eq!(session_of(&base.link(paths::CALL_END)), Some("8b0f3c2e-session"));
        assert_eq!(session_of(&base.link(paths::CONVERSATION_END)), Some("8b0f3c2e-session"));
        // A query the service appends is never read as part of the address.
        assert_eq!(
            session_of("https://x/v4/f/a/callAgent/s1/c1/call/mediaAnswer/?i=1.2.3.4"),
            Some("s1")
        );
        assert_eq!(session_of("https://x/v4/f/a/?q=/callAgent/s1/c1/"), None);
        // An address with no session segment — the registration's own, say — names none.
        assert_eq!(session_of("https://trouter.example/v4/f/abc/"), None);
        // A segment that is present and empty names our agent and no session of ours.
        assert_eq!(session_of("https://x/callAgent//c1/call/end/"), Some(""));
    }

    /// A frame acts on the live call only when it is addressed to that call's own session.
    #[test]
    fn a_frame_is_routed_to_the_live_call_only_by_its_own_session() {
        let live = "https://tr/v4/f/a/callAgent/live/c/conversation/conversationEnd/";
        let other = "https://tr/v4/f/a/callAgent/other/c/conversation/conversationEnd/";
        let bare = "https://tr/v4/f/a/";
        assert_eq!(frame_route(live, Some("live")), FrameRoute::Ours);
        assert_eq!(frame_route(other, Some("live")), FrameRoute::Elsewhere { session: "other" });
        // With no call held, every addressed frame belongs elsewhere — a call that ended frees
        // its slot, and its late frames must not reach the next one.
        assert_eq!(frame_route(live, None), FrameRoute::Elsewhere { session: "live" });
        // An address that names no session keeps today's handling, live call or not.
        assert_eq!(frame_route(bare, Some("live")), FrameRoute::Unaddressed);
        assert_eq!(frame_route(bare, None), FrameRoute::Unaddressed);
        // An empty segment can never be the live call's.
        assert_eq!(
            frame_route("https://tr/v4/f/a/callAgent//c/call/end/", Some("live")),
            FrameRoute::Elsewhere { session: "" }
        );
    }

    /// An unaddressed frame is journaled by the part after the surl, and the surl — whose
    /// allocation id routes frames to this connection — never reaches the journal.
    #[test]
    fn an_unaddressed_frame_is_named_without_the_surl() {
        let surl = Some("https://tr/v4/f/secretid/");
        assert_eq!(unaddressed_frame_shape("https://tr/v4/f/secretid/", surl), "the connection's own address");
        assert_eq!(
            unaddressed_frame_shape("https://tr/v4/f/secretid/SkypeSpacesWeb/?i=1.2.3.4", surl),
            "SkypeSpacesWeb"
        );
        // A prefix of the id is not the surl: the boundary is a segment, never a character.
        let longer = unaddressed_frame_shape("https://tr/v4/f/secretidtail/x", surl);
        assert!(!longer.contains("tail"), "printed part of another allocation id: {longer}");
        // Off this connection's surl, or with no surl known, nothing of the address is printed.
        for shape in [
            unaddressed_frame_shape("https://elsewhere/v4/f/key/x", surl),
            unaddressed_frame_shape("https://tr/v4/f/secretid/x", None),
        ] {
            assert!(!shape.contains("key") && !shape.contains("secretid"), "{shape}");
        }
    }

    /// Both spellings of a source request. The newer one addresses the section by mid and
    /// carries its parameter as a JSON STRING inside the JSON, which is the service's own
    /// shape (`JSON.stringify` in the client's message generator) and not a mistake here.
    #[test]
    fn a_source_request_is_built_in_both_spellings() {
        let request = SourceRequest {
            mid: "3".into(),
            source_id: 2473,
            stream_msid: "stream-id".into(),
            fmt_params: DEFAULT_VIDEO_FMTP.into(),
        };
        let modern = source_request_payload(&request, 7, true);
        assert_eq!(modern.pointer("/applyChannelParameters/multiChannelParameter/mids"), Some(&json!(["3"])));
        let parameter = modern
            .pointer("/applyChannelParameters/multiChannelParameter/mediaParameter")
            .and_then(Value::as_str)
            .expect("a string, not an object");
        let inner: Value = serde_json::from_str(parameter).expect("valid JSON inside the string");
        assert_eq!(inner.pointer("/controlVideoStreaming/sequenceNumber"), Some(&json!(7)));
        assert_eq!(inner.pointer("/controlVideoStreaming/controlInfo/sourceId"), Some(&json!(2473)));
        assert_eq!(
            inner.pointer("/controlVideoStreaming/controlInfo/streamMsid"),
            Some(&json!("stream-id"))
        );

        let legacy = source_request_payload(&request, 7, false);
        // The older shape's controlInfo is an ARRAY, and it names the control itself.
        assert_eq!(
            legacy.pointer("/controlVideoStreaming/controlInfo/0/control"),
            Some(&json!("start"))
        );
        assert_eq!(
            legacy.pointer("/controlVideoStreaming/controlInfo/0/sourceId"),
            Some(&json!(2473))
        );
        // The fmtp is mandatory: the client's own builder throws without one, so neither
        // spelling may go out empty.
        for payload in [modern.to_string(), legacy.to_string()] {
            assert!(payload.contains("profile-level-id"), "a source request needs an fmtp");
        }
    }

    /// The older spelling still reads, because it costs one pointer lookup and this tenant
    /// is one tenant.
    #[test]
    fn a_wrapped_roster_of_an_array_still_reads() {
        let roster = roster_in_frame(&json!({
            "_decoded": { "rosterUpdate": { "participants": [
                { "id": "8:orgid:her", "displayName": "Her", "state": "Connected" },
                { "id": "8:orgid:him", "displayName": "Him", "state": "Lobby" },
            ] } }
        }))
        .expect("a roster");
        assert!(!roster.delta, "no `type` means the frame is the whole roster");
        assert_eq!(roster.members.len(), 2);
        assert_eq!(roster.members[0].display_name, "Her");
        assert!(!roster.members[0].in_lobby);
        assert!(roster.members[1].in_lobby);
        assert!(roster.members[1].present, "the lobby is not an absence");
    }

    /// A delta is FOLDED in. Every one of these three moves was seen in one 40-second
    /// meeting, and replacing the list on each frame showed it emptying between them.
    #[test]
    fn a_delta_roster_merges_and_a_departure_removes_its_person() {
        let mut held = Vec::new();
        let arrives = |mri: &str, state: &str| RosterUpdate {
            delta: true,
            members: vec![RosterMember {
                mri: mri.to_string(),
                display_name: mri.to_string(),
                in_lobby: false,
                present: state == "active",
                streams: Vec::new(),
            }],
        };
        assert!(apply_roster_update(&mut held, arrives("8:orgid:her", "active")));
        assert!(apply_roster_update(&mut held, arrives("8:orgid:him", "active")));
        assert_eq!(held.len(), 2, "a delta adds to the list rather than replacing it");
        // The same frame twice changes nothing, so no state is emitted for it.
        assert!(!apply_roster_update(&mut held, arrives("8:orgid:him", "active")));
        assert!(apply_roster_update(&mut held, arrives("8:orgid:him", "inactive")));
        assert_eq!(held.len(), 1, "somebody who left is dropped, not kept as a name");
        assert_eq!(held[0].mri, "8:orgid:her");
        // A FULL frame replaces the list, which is the other half of the same function.
        let full = RosterUpdate {
            delta: false,
            members: vec![RosterMember {
                mri: "8:orgid:third".into(),
                display_name: "Third".into(),
                in_lobby: false,
                present: true,
                streams: Vec::new(),
            }],
        };
        assert!(apply_roster_update(&mut held, full));
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].mri, "8:orgid:third");
    }

    #[test]
    fn the_directory_udp_transport_becomes_one_stun_server() {
        let stun = stun_from_udp_transport("udp://api-emea.flightproxy.teams.microsoft.com:3478")
            .expect("a stun server");
        assert_eq!(stun.urls, vec!["stun:api-emea.flightproxy.teams.microsoft.com:3478"]);
        assert!(stun.username.is_none());
        assert_eq!(stun_from_udp_transport("https://x:3478"), None);
        assert_eq!(stun_from_udp_transport("udp://no-port"), None);
    }

    #[test]
    fn a_relay_config_becomes_the_ice_servers_a_browser_takes() {
        let relay = json!({
            "addresses": ["52.113.1.2"],
            "fqdns": ["relay.example.net"],
            "udpPort": 3478,
            "tcpPort": 443,
            "tlsPort": 443
        });
        let credentials = json!({ "username": "user", "password": "secret", "realm": "\"r\"" });
        let servers = ice_servers_from_relay_config(&relay, &credentials);
        assert_eq!(servers.len(), 1);
        let server = &servers[0];
        assert_eq!(
            server.urls,
            vec![
                "turn:relay.example.net:3478?transport=udp",
                "turn:relay.example.net:443?transport=tcp",
                "turns:relay.example.net:443",
            ]
        );
        assert_eq!(server.username.as_deref(), Some("user"));
        assert_eq!(server.credential.as_deref(), Some("secret"));
        // The browser shape carries exactly the three keys it knows.
        let rendered = server.json();
        assert!(rendered.get("urls").is_some());
        assert_eq!(rendered.get("credential").unwrap(), "secret");
    }

    #[test]
    fn a_relay_config_with_no_host_yields_nothing_rather_than_a_broken_url() {
        assert!(ice_servers_from_relay_config(&json!({ "udpPort": 3478 }), &json!({})).is_empty());
        assert!(ice_servers_from_relay_config(&json!({ "fqdns": ["r"] }), &json!({})).is_empty());
    }

    #[test]
    fn the_ice_server_list_holds_stun_without_credentials_and_turn_with_them() {
        let endpoints = Endpoints {
            conversation_service: "https://conv".into(),
            trouter: "https://trouter/v3/c".into(),
            registrar: "https://registrar".into(),
            udp_transport: Some("udp://media.example:3478".into()),
        };
        let stun_only = ice_servers(&endpoints, None, None);
        assert_eq!(stun_only.len(), 1);
        assert!(stun_only[0].urls[0].starts_with("stun:"));

        let both = ice_servers(
            &endpoints,
            Some(&json!({ "fqdns": ["relay.example"], "udpPort": 3478 })),
            Some(&json!({ "username": "u", "password": "p" })),
        );
        assert_eq!(both.len(), 2);
        assert!(both[1].urls[0].starts_with("turn:"));
    }

    #[test]
    fn a_relay_token_response_yields_its_first_credential_either_shape() {
        let listed = json!({ "expires": 1, "tokens": [{ "realm": "a", "username": "u1", "password": "p1" }] });
        assert_eq!(first_relay_credential(&listed).unwrap()["username"], "u1");
        let single = json!({ "realm": "a", "username": "u2", "password": "p2" });
        assert_eq!(first_relay_credential(&single).unwrap()["username"], "u2");
        assert_eq!(first_relay_credential(&json!({ "expires": 1 })), None);
    }

    #[test]
    fn a_relay_config_is_found_wherever_the_service_nested_it() {
        let frame = json!({ "a": { "b": { "relayConfig": { "Relay": { "Turn": {} } } } } });
        assert!(relay_config_in_frame(&frame).is_some());
        assert!(relay_config_in_frame(&json!({ "a": 1 })).is_none());
    }

    /// An error string is read out of a journal for weeks, and a signaling link can
    /// carry a token in its query.
    #[test]
    fn an_error_never_repeats_a_link_query() {
        assert_eq!(redact_url("https://x/call/end/?access_token=secret"), "https://x/call/end/");
        assert_eq!(redact_url("https://x/call/end/"), "https://x/call/end/");
    }

    /// Where the golden call bodies live, one file per body.
    const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/testdata/call-bodies");

    /// Every body this module builds, in its DEFAULT shape, from fixed inputs — each named by the
    /// file it is pinned to.
    ///
    /// The inputs are invented and stable: nobody's ids, and nothing that moves between runs. A
    /// builder that started reading a clock or minting a uuid would fail here at once, because a
    /// body that differs between two presses is one nobody can compare with a capture.
    fn golden_bodies() -> Vec<(&'static str, Value)> {
        let local = LocalParticipant {
            id: "8:orgid:00000000-0000-4000-8000-000000000001".into(),
            display_name: "Ada Lovelace".into(),
            endpoint_id: "11111111-1111-4111-8111-111111111111".into(),
            participant_id: "22222222-2222-4222-8222-222222222222".into(),
        };
        let callbacks = CallbackBase {
            surl: "https://pub-ent-golden.trouter.teams.microsoft.com:3443/v4/f/GOLDEN/".into(),
            session_id: "33333333-3333-4333-8333-333333333333".into(),
            cause_id: "0a1b2c3d".into(),
        };
        let sdp = MediaContent::sdp(
            "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 9 RTP/SAVP 111\r\na=mid:0\r\n",
        );
        let callee = vec!["8:orgid:00000000-0000-4000-8000-0000000000aa".to_string()];
        let long_link = MeetingJoin::from_join_url(
            "https://teams.microsoft.com/l/meetup-join/19%3ameeting_GOLDEN%40thread.v2/0?context=\
             %7b%22Tid%22%3a%2244444444-4444-4444-8444-444444444444%22%2c%22Oid%22%3a%2255555555-5555-\
             4555-8555-555555555555%22%7d",
        )
        .expect("a long join link");
        let short_link =
            MeetingJoin::from_join_url("https://teams.microsoft.com/meet/123456789012?p=GoldenPass")
                .expect("a short join link");
        let thread = MeetingJoin::from_thread_id("19:meeting_GOLDEN@thread.v2").expect("a thread");
        let subscription = SourceRequest {
            mid: "3".into(),
            source_id: 7,
            stream_msid: "golden-stream".into(),
            fmt_params: DEFAULT_VIDEO_FMTP.into(),
        };
        let seed = match start_endpoint_state() {
            Value::Object(map) => map,
            other => panic!("the start endpoint state is an object: {other}"),
        };
        vec![
            (
                "invitation",
                invitation_payload(&local, &callee, Some("19:golden@unq.gbl.spaces"), &sdp, &callbacks),
            ),
            ("join-long-link", join_payload(&local, &long_link, &callbacks, Some(&sdp))),
            ("join-short-link", join_payload(&local, &short_link, &callbacks, Some(&sdp))),
            ("join-thread-roster-only", join_payload(&local, &thread, &callbacks, None)),
            ("attach", attach_payload(&local, &sdp, &callbacks)),
            (
                "attach-join-at-answer",
                attach_join_payload(
                    &local,
                    "https://api.flightproxy.teams.microsoft.com/api/v2/ep/conv-golden/conversation/",
                    &callbacks,
                    false,
                ),
            ),
            ("acceptance", acceptance_payload(&local, &sdp, &callbacks)),
            ("media-answer", media_answer_payload(&local, &sdp, &callbacks, &[MODALITY_AUDIO])),
            (
                "media-offer",
                media_offer_payload(&local, &sdp, &callbacks, &[MODALITY_AUDIO, MODALITY_VIDEO]),
            ),
            (
                "start-outgoing-negotiation",
                start_outgoing_negotiation_payload(
                    &local,
                    &sdp,
                    &callbacks,
                    &[MODALITY_AUDIO, MODALITY_VIDEO],
                ),
            ),
            ("acceptance-acknowledgement", acceptance_acknowledgement_payload(&callbacks)),
            ("hangup", hangup_payload(&local)),
            ("rejection", rejection_payload(&local)),
            ("mute", mute_payload(&local, true)),
            (
                "endpoint-state",
                endpoint_state_payload(&local, &next_endpoint_state(&seed, true, 1)),
            ),
            ("media-glare-rejection", media_glare_rejection_payload(&local)),
            (
                "content-sharing",
                content_sharing_payload(&local, &callbacks, "66666666-6666-4666-8666-666666666666"),
            ),
            ("content-sharing-leave", content_sharing_leave_payload(&local, &callbacks)),
            ("source-request-modern", source_request_payload(&subscription, 1, true)),
            ("source-request-legacy", source_request_payload(&subscription, 1, false)),
        ]
    }

    /// THE GOLDEN PINS: every call body, byte for byte what it is today.
    ///
    /// This is the ONE golden fixture set (docs/calling-roadmap/README.md § 3, collision 4;
    /// video-send M1) and it exists for the refactors that follow it. The verified calls — a
    /// one-to-one both ways and a meeting join — rest on bodies nobody can re-measure without a
    /// colleague on the other end, so a change that moves the transport, the classes or a builder
    /// must prove it moved no byte the service reads. A later workstream that adds a kind EXTENDS
    /// this set rather than pinning the same bodies a second time.
    ///
    /// What is compared is the pretty form of the JSON the request serializes. serde_json keeps
    /// object keys sorted here (no `preserve_order`), so the key order on the wire is the order in
    /// the file, and a value, a type or a key that moved is a line that moved.
    ///
    /// A deliberate change re-blesses with `TEAMS_LITE_BLESS_CALL_BODIES=1 cargo test --lib
    /// golden`, and the diff of `src/testdata/call-bodies/` is then the review of what the
    /// service will be sent differently. Never bless to make this pass.
    #[test]
    fn every_call_body_is_byte_for_byte_its_golden_pin() {
        let bless = std::env::var("TEAMS_LITE_BLESS_CALL_BODIES").as_deref() == Ok("1");
        let bodies = golden_bodies();
        let mut names: Vec<&str> = bodies.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), bodies.len(), "two golden bodies share a file name");
        let mut drifted = Vec::new();
        for (name, body) in &bodies {
            let path = format!("{GOLDEN_DIR}/{name}.json");
            let rendered = format!("{}\n", serde_json::to_string_pretty(body).expect("a body is JSON"));
            if bless {
                std::fs::create_dir_all(GOLDEN_DIR).expect("the golden directory");
                std::fs::write(&path, &rendered).expect("bless a golden body");
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(pinned) if pinned == rendered => {}
                Ok(pinned) => drifted.push(format!(
                    "{name}: the body no longer matches {path}\n--- pinned\n{pinned}--- built now\n{rendered}"
                )),
                Err(e) => drifted.push(format!("{name}: no golden pin at {path} ({e})")),
            }
        }
        // A pin with no body behind it is a builder somebody removed without saying so.
        let mut stale = Vec::new();
        if let Ok(entries) = std::fs::read_dir(GOLDEN_DIR) {
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(stem) = file.strip_suffix(".json") else { continue };
                if !names.contains(&stem) {
                    stale.push(file);
                }
            }
        }
        assert!(stale.is_empty(), "golden pins with no body behind them: {stale:?}");
        assert!(drifted.is_empty(), "{}", drifted.join("\n"));
    }
}
