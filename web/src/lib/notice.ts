// teams-lite web — transient notices (sonner).
//
// The one surface for something that HAPPENED and has no place of its own to be said
// in: a call that ended for a reason the user did not choose, a capture the browser
// refused, a call that never went out. Each is about an action whose surface is already
// gone by the time there is anything to say, so it is drawn over the app and then it
// leaves.
//
// It exists because the alternative was tried and was worse: the reason lived in the
// STORE, as `callError`, and state has to be cleared by something. Nothing cleared it —
// the next call did, eventually — so `not connected` sat over the chat list until the
// user placed another call. A transient notice is an EVENT, and this module is the one
// place that says so.
//
// What must NOT come through here, and each for a stated reason:
//
//   • a failed SEND — it belongs beside the words still in the composer, because the
//     user acts on it (see lib/send-failure.ts and CLAUDE.md § Sending messages);
//   • a merge request's approval — held open in the menu the click was made in;
//   • the write lock, a broken sign-in, an available update — each is a STATE of the
//     app rather than an event, and a state that scrolls away is a state nobody can
//     check. Those keep their banners and their row.
//
// A thin, best-effort layer, like ./notify.ts: it never throws and it decides nothing.

import { toast } from "sonner";

/** How long a notice about something the app DID stays. Six seconds is long enough to
 *  read one sentence, and short enough that it is gone before it is in the way. */
export const NOTICE_MS = 6_000;

/** A failure stays longer than a report: it is the one the user may have to act on,
 *  and it is the one they did not expect to read. */
export const ERROR_NOTICE_MS = 9_000;

export type Notice = {
  /** One sentence. Already in the user's words — nothing here rephrases. */
  text: string;
  /** A failure, or a report of something that happened. Only the ink differs. */
  kind: "error" | "report";
  /** Stable per subject, so a second notice about the same thing REPLACES the first
   *  instead of stacking two sentences about one call. */
  id: string;
  /** The hook automation reads (`data-testid` on the notice itself). Named at the call
   *  site so it is visible there that a driver depends on it. */
  testId: string;
};

/** The notice for one call: why it ended, or why it did not happen. One id, because a
 *  call has one thing to say at a time. */
export const CALL_NOTICE = "call";

/** And the notice for a RECORDING of a call, which is a different subject: what happened to
 *  a FILE. It has an id of its own because the two arrive together — a call that dropped for
 *  a reason the user did not choose also ends the recording — and one id would let the file's
 *  fate replace the reason the call ended, which is the half they cannot infer. */
export const RECORDING_NOTICE = "call-recording";

/**
 * How long a dismissed notice takes to leave: sonner keeps it mounted for its exit animation
 * (`TIME_BEFORE_UNMOUNT`, 200 ms in 2.0.7) and then deletes the entry BY ID — so a notice shown
 * again under the same id inside that window is deleted with the old one, a beat after it was
 * drawn. That is exactly what a second failed attempt does: the attempt takes the old reason
 * back as it starts, fails within milliseconds, and says why. This is that window plus a margin.
 */
const NOTICE_EXIT_MS = 250;

/** When each subject's notice was last taken back, and the notice waiting for that exit to
 *  finish. One pending notice per subject: a later one replaces it, as it would on screen. */
const lastDismissed = new Map<string, number>();
const pending = new Map<string, { notice: Notice; timer: ReturnType<typeof setTimeout> }>();

/** Show one notice, replacing whatever is on screen under the same id.
 *
 *  A notice that lands while the previous one under its id is still LEAVING waits for it to
 *  go, rather than being deleted with it (see {@link NOTICE_EXIT_MS}). */
export function showNotice(notice: Notice): void {
  // Client-only. The module store behind `toast` outlives a single SSR render on the
  // server, so a notice queued there would belong to whichever request rendered next.
  if (typeof window === "undefined") return;
  if (!notice.text) return;
  const leaving = NOTICE_EXIT_MS - (Date.now() - (lastDismissed.get(notice.id) ?? -Infinity));
  const waiting = pending.get(notice.id);
  if (leaving > 0) {
    if (waiting) {
      waiting.notice = notice;
      return;
    }
    const entry = {
      notice,
      timer: setTimeout(() => {
        pending.delete(notice.id);
        present(entry.notice);
      }, leaving),
    };
    pending.set(notice.id, entry);
    return;
  }
  if (waiting) {
    clearTimeout(waiting.timer);
    pending.delete(notice.id);
  }
  present(notice);
}

function present(notice: Notice): void {
  const options = {
    id: notice.id,
    testId: notice.testId,
    duration: notice.kind === "error" ? ERROR_NOTICE_MS : NOTICE_MS,
    // No glyph. Sonner draws its own tick and cross for a typed toast, and every glyph in
    // this app comes from one library (CLAUDE.md § Hugeicons) — a second set on the one
    // card that floats over everything is exactly where it would show.
    icon: null,
  };
  // A REPORT is sent with a type of its own (`info`), never as an untyped toast. Sonner
  // replaces a toast under the same id by MERGING the new one into it, so an untyped report
  // kept the `type: "error"` of the failure it replaced — and "You answered on another
  // device." was drawn in the failure's red ink, because a refused camera had been said a
  // moment before. The ink is keyed off that type (app-toaster.tsx), so the type has to be
  // stated every time.
  if (notice.kind === "error") toast.error(notice.text, options);
  else toast.info(notice.text, options);
}

/** Take one back — used when the thing it was about is being tried again, so the old
 *  reason cannot sit over the new attempt. */
export function dismissNotice(id: string): void {
  if (typeof window === "undefined") return;
  toast.dismiss(id);
  lastDismissed.set(id, Date.now());
  // A notice still waiting to be drawn belonged to the attempt this dismissal takes back.
  const waiting = pending.get(id);
  if (waiting) {
    clearTimeout(waiting.timer);
    pending.delete(id);
  }
}
