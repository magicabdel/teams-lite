import { HugeiconsIcon } from "@hugeicons/react";
import { CallIcon } from "@hugeicons/core-free-icons";
import {
  canJoinMeeting,
  isMeetingJoinLink,
  meetingUnavailableReason,
  type MeetingAddress,
} from "~/lib/call";
import { useAppState, useController } from "./controller-context";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

/**
 * "Join here" — walk into a meeting with audio, in this app.
 *
 * In a calendar event it sits beside the "Open in" that reaches real Teams, and it does not
 * replace it: this joins with a microphone and nothing else, so a meeting where the user
 * has to see a shared screen is still a meeting they open in Teams. Offering both is the
 * honest shape.
 *
 * Two surfaces reach it, and each covers what the other cannot. A calendar event joins by
 * the LINK it carries; a meeting's own CHAT joins by its THREAD, which is the only address
 * that exists there — this tenant's invitations carry the short `/meet/{code}` link, and
 * that code lives in the event and nowhere in the conversation. So the button takes either
 * {@link MeetingAddress} and states the one it holds.
 *
 * Drawn only for an address this app can really join. Where a join would not work right now it
 * stays, disabled — the meeting is real and this machine cannot join it yet — and says why in
 * WORDS: in its label, after what the control is for, and under its row through
 * {@link MeetingJoinReason}, which the surface that draws the button puts there. A disabled
 * control fires no pointer events and a phone has no hover, so a reason held only in a tooltip
 * is a sentence that does not exist where this app is read most. The tooltip stays for a
 * pointer, as the nicety it is.
 */
export function MeetingJoinButton(props: {
  meeting: MeetingAddress;
  subject?: string;
  /** How it is drawn. `pill` is the labelled primary button a calendar event's details
   *  panel puts beside its link out, where words are what tell the two apart. `icon` is the
   *  bare glyph a CHAT HEADER takes, because that header already holds one control of that
   *  exact shape in every other conversation. It is still what the CALENDAR and the incoming-call
   *  banner draw; a conversation's own join is a row of its menu (components/conversation-menu.tsx). */
  shape?: "pill" | "icon";
  /** Called when the join has been DISPATCHED — not when it has succeeded, which this button
   *  never learns: the join is fire-and-forget and its outcome arrives as a `call_state`
   *  frame, with a failure reported by the app's own notice (§ Audio calls). It exists so the
   *  dialog that holds a pasted link can close itself, and it deliberately promises no more
   *  than the press: a caller that treated it as success would be claiming something this
   *  side cannot know. */
  onStarted?: () => void;
}) {
  const controller = useController();
  const status = useAppState((s) => s.callStatus);
  const { meeting } = props;

  if (!meetingJoinIsOffered(meeting)) return null;

  const ready = canJoinMeeting(status);
  const reason = meetingUnavailableReason(status);
  const icon = props.shape === "icon";
  const button = (
    <button
      type="button"
      data-testid="meeting-join-here"
      data-shape={icon ? "icon" : "pill"}
      // WHICH meeting this button joins, in the page's own state — one attribute per shape
      // of address, and never both.
      //
      // Joining is outward — everybody in the meeting sees the user arrive — so a driver
      // must be able to prove its target before it clicks, exactly as it re-reads
      // `[data-testid="composer-shell"]`'s `data-conversation-id` before every keystroke.
      // Without this, a script could only assume, and an assumption is what put three
      // messages in real colleagues' chats (AGENTS.md § Automation safety).
      data-join-url={meeting.kind === "link" ? meeting.joinUrl : undefined}
      data-meeting-thread={meeting.kind === "thread" ? meeting.thread : undefined}
      disabled={!ready}
      // What the control is FOR, and then why it cannot, where there is a reason — the shape
      // the conversation menu's Join row has. The reason alone used to REPLACE the label, which
      // cost a screen reader the one thing a button announces first.
      aria-label={ready ? JOIN_LABEL : `${JOIN_LABEL} — ${reason}`}
      onClick={() => {
        void controller.joinMeeting(meeting, props.subject);
        props.onStarted?.();
      }}
      className={
        icon
          ? // The call button's own box, to the pixel: this is one row of header controls, and
            // a conversation the user walks into must not move the controls of the one they
            // came from.
            "grid size-9 shrink-0 place-items-center rounded-lg text-text-dim transition-colors hover:bg-accent hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
          : // `h-8` is the height a small button has everywhere in this app (`ui/button`'s
            // own `sm`), because the pill shares a row with one: the calendar's way out, and
            // "Open chat" on the incoming-call card. Two controls of one row at two heights
            // read as two designs.
            "flex h-8 items-center gap-1.5 rounded-lg bg-primary px-3 text-[13px] font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-40"
      }
    >
      {/* The HANDSET in both shapes, and in a chat header the same one the call button
          wears. It is not a claim that this rings anybody: it says "start talking to the
          people in this conversation, here", which is what both actions do. A glyph that
          tried to say "join" instead was measured and rejected — `MeetingRoomIcon` reads as
          a bare panel at 20px, which is worse than a mark somebody already knows. What the
          click really does is in the tooltip and in the label a screen reader gets, and the
          row itself already says "Meeting chat" under the title. */}
      <HugeiconsIcon
        icon={CallIcon}
        className={icon ? "size-5" : "size-3.5"}
        strokeWidth={icon ? 1.6 : 1.8}
      />
      {!icon && "Join here"}
    </button>
  );

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* A disabled button fires no pointer events, so the tooltip hangs off a
            wrapper the user can still hover and focus. */}
        <span tabIndex={0}>{button}</span>
      </TooltipTrigger>
      <TooltipContent>
        {reason || "Join with audio, here. Open it in Teams for video and screen sharing."}
      </TooltipContent>
    </Tooltip>
  );
}

/** What the Join control is for — its whole label when it can act, and the first half of it
 *  when it cannot. */
const JOIN_LABEL = "Join this meeting with audio";

/**
 * Why the {@link MeetingJoinButton} beside it cannot join right now, drawn IN FLOW — or
 * nothing, when it can, or when there is no button at all.
 *
 * A component of its own rather than part of the button, because a sentence does not fit in
 * the row the button shares: the calendar's footer puts it beside "Open in", the incoming-call
 * card beside "Open chat". So the surface draws the row, and this under it. It reads the same
 * state through the same two functions the button does, so the two can never disagree about
 * whether there is a button to explain, or what is wrong with it.
 */
export function MeetingJoinReason(props: { meeting: MeetingAddress }) {
  const status = useAppState((s) => s.callStatus);
  if (!meetingJoinIsOffered(props.meeting) || canJoinMeeting(status)) return null;
  const reason = meetingUnavailableReason(status);
  if (!reason) return null;
  // The conversation menu's own reason line, to the class: one sentence, one look, wherever a
  // disabled call or Join control explains itself.
  return (
    <p data-testid="meeting-join-reason" className="text-[11px] leading-snug text-text-faint">
      {reason}
    </p>
  );
}

/**
 * Whether this app offers a Join for `meeting` at all.
 *
 * An address this app cannot join is not offered. For a LINK the check is a small port of
 * `calling::MeetingJoin::from_join_url`; a THREAD was checked where the address was built
 * (`meetingAddressOf`, the port of `from_thread_id`). The backend parses it again either way,
 * so the worst a mismatch costs is a button that reports a refusal.
 */
function meetingJoinIsOffered(meeting: MeetingAddress): boolean {
  return meeting.kind !== "link" || isMeetingJoinLink(meeting.joinUrl);
}
