// The reading half of `bun run join-live -- --pair --toggle-mute`, kept apart from the driver.
//
// It is pure — it reads roster participants that have already been captured and says what they
// stated — so it is tested with no browser and no meeting (mute-digest.test.ts), while the driver
// that presses Mute stays the one file that touches a live page.

/** Every ENDPOINT the roster named, and what its `endpointState.state.isMuted` said over a run,
 *  in order, with a repeat of the same value folded away — booleans and nothing else. An
 *  endpoint is named by the order it was first seen in, never by its id. */
export type EndpointMutes = Array<{ endpoint: string; isMuted: boolean[] }>;

/** Folds roster participants into {@link EndpointMutes}, one participant at a time, in the
 *  order the frames arrived. */
export class EndpointMuteLog {
  private readonly seen = new Map<string, { endpoint: string; isMuted: boolean[] }>();

  /**
   * Note every endpoint of one roster participant. The roster keys a participant's endpoints by
   * id (`endpoints: { "<id>": { endpointState: { state: { isMuted } } } }`, NATIVE-CALLING.md),
   * so the id is the key here and never printed. An endpoint that states no mute is still
   * listed, with no values: "never stated" is a different answer from `false` every time.
   */
  note(person: Record<string, unknown>): void {
    const endpoints = person.endpoints;
    if (!endpoints || typeof endpoints !== "object" || Array.isArray(endpoints)) return;
    for (const [id, endpoint] of Object.entries(endpoints as Record<string, unknown>)) {
      const muted = (endpoint as { endpointState?: { state?: { isMuted?: unknown } } } | null)
        ?.endpointState?.state?.isMuted;
      const seen = this.seen.get(id) ?? { endpoint: `endpoint ${this.seen.size + 1}`, isMuted: [] };
      this.seen.set(id, seen);
      if (typeof muted === "boolean" && seen.isMuted.at(-1) !== muted) seen.isMuted.push(muted);
    }
  }

  list(): EndpointMutes {
    return [...this.seen.values()].map((e) => ({ endpoint: e.endpoint, isMuted: [...e.isMuted] }));
  }
}

/** What the receiver's roster said at the three moments `--toggle-mute` reads it: just before
 *  the Mute press, just before the Unmute press, and once the Unmute has had time to arrive. */
export type MuteSnapshots = {
  beforeMute: EndpointMutes;
  afterMute: EndpointMutes;
  afterUnmute: EndpointMutes;
};

/**
 * The one line `--toggle-mute` ends on: whether an endpoint's published mute went on BETWEEN the
 * two presses and came back off AFTER the second — judged against the moments of the presses, not
 * against the whole run.
 *
 * Timing is what keeps it honest: the pinned meeting may hold a colleague, and one who joined muted
 * and unmuted at any moment would fold to "true → false" over the whole run. An endpoint counts only
 * if it was NOT muted before the Mute press, WAS muted when the Unmute was pressed, and was NOT
 * muted at the end. A press the sender's stage never showed asked the service nothing, so no
 * reading can pass after one. Each endpoint is matched across the three snapshots by its label —
 * the order it was first seen in — which holds while the receiver's frame buffer has not dropped
 * the frame that first named it (it keeps 600, and the run is some forty seconds).
 */
export function muteRoundTrip(
  snapshots: MuteSnapshots,
  pressed: { muted: boolean; unmuted: boolean },
): string {
  if (!pressed.muted || !pressed.unmuted) {
    return "NOT SEEN — the sender's stage never showed the press, so the service was asked nothing";
  }
  const last = (list: EndpointMutes, endpoint: string) =>
    list.find((e) => e.endpoint === endpoint)?.isMuted.at(-1);
  const read = snapshots.afterUnmute.map((e) => ({
    endpoint: e.endpoint,
    before: last(snapshots.beforeMute, e.endpoint),
    during: last(snapshots.afterMute, e.endpoint),
    after: last(snapshots.afterUnmute, e.endpoint),
  }));
  const went = read.find((r) => r.before !== true && r.during === true && r.after === false);
  if (went) {
    return `SEEN — ${went.endpoint}'s isMuted went ${went.before ?? "unstated"} → true → false, in step with the presses`;
  }
  const half = read.find((r) => r.before !== true && r.during === true);
  if (half) return `HALF — ${half.endpoint} showed the mute and never showed the unmute`;
  return "NOT SEEN — no endpoint's isMuted moved with the presses; the sender's log says what it published";
}
