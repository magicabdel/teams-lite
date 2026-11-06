/**
 * Settings › This app › Calling protocol: the words and the decisions behind the row, kept pure
 * so they are unit-tested (the component in calling-protocol-settings.tsx draws them).
 *
 * The setting decides how THIS window's backend's calls speak to Teams, where it has a choice
 * between the flow this app has always used and the one the Teams web client uses
 * (docs/calling-roadmap/incoming-and-conversation.md § 6). It is per BACKEND — stored under the
 * backend's own port — so the released build beside the staged one may try a protocol while the
 * other keeps the existing one, and it applies from the next call: a call keeps the protocol it
 * started with.
 *
 * One choice exists today, the mute. The roadmap's other two rows (how an incoming call is taken,
 * and whether a long call is kept alive) join it in the milestone that gives them something to
 * switch, because a choice that changes nothing reads as a bug.
 */

import { isLive, type CallProtocol, type CallStatus, type MuteProtocol } from "./call";

/** One option of a protocol row: the value the backend stores, and what the reader is told. */
export type ProtocolChoice<T extends string> = {
  value: T;
  label: string;
  detail: string;
};

/** The mute row's two options, in the order the backend lists its words
 *  (`calling::MuteProtocol::WORDS`). This app's existing flow is first and is the default.
 *
 *  It is NOT labelled "Measured", which the roadmap's planned copy called it: that word is true of
 *  the incoming-call row this section will grow, whose answer was measured on the tenant, and false
 *  here — the legacy mute publish has never been measured either (`MuteProtocol::Legacy`). */
export const MUTE_PROTOCOL_CHOICES: readonly ProtocolChoice<MuteProtocol>[] = [
  {
    value: "legacy",
    label: "As before (default)",
    detail: "How this app has always told a call that you muted.",
  },
  {
    value: "endpoint-state",
    label: "Publish it the way Teams does",
    detail:
      "The Teams web client's own way, which is what a meeting reads to show you muted beside " +
      "your name. Your microphone mutes the same either way.",
  },
];

/** What the row says under its options, always: where the choice applies, and where not. */
export const CALLING_PROTOCOL_NOTE =
  "Applies to calls this backend starts from now on. Each teams-lite install on this machine " +
  "keeps its own.";

/** Whether the row is offered at all: by a backend that calls, and that states a protocol.
 *
 *  A read-only backend never calls, so a protocol for its calls would be a choice about nothing;
 *  and a backend too old to state one has no setting to change. Either way the row is not drawn
 *  — a control that changes nothing reads as a bug. */
export function callingProtocolOffered(
  status: CallStatus,
): status is CallStatus & { protocol: CallProtocol } {
  return status.enabled && status.protocol !== undefined;
}

/** The sentence a live call earns when it keeps a different protocol from the one now chosen,
 *  or null. It is the one place the "from the next call" rule would otherwise surprise the
 *  reader: they change the setting during a call and the call carries on as it started. */
export function liveCallKeepsItsProtocol(status: CallStatus): string | null {
  const call = status.call;
  if (!isLive(call) || !call?.protocol || !status.protocol) return null;
  if (call.protocol.mute === status.protocol.mute) return null;
  return "The call going now keeps the way it started with until it ends.";
}
