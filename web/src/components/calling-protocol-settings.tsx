import { useState } from "react";
import {
  CALLING_PROTOCOL_NOTE,
  MUTE_PROTOCOL_CHOICES,
  callingProtocolOffered,
  liveCallKeepsItsProtocol,
  type ProtocolChoice,
} from "~/lib/calling-protocol";
import type { CallProtocol } from "~/lib/call";
import { cn } from "~/lib/utils";
import { useAppState, useController } from "./controller-context";
import { FadeArc } from "./loading-ui/fade-arc";

/**
 * Settings › This app › Calling protocol (experimental): how this window's backend's calls speak
 * to Teams, where it has a choice between the flow this app has always used and the Teams web client's.
 *
 * The CHOICE on screen is the backend's, never this page's memory of a press: it is read from
 * `call_state.protocol`, which every page is sent, so two open windows agree and a refused press
 * leaves the old choice standing. A press waits for the backend's answer, and a refusal is said
 * here, under the row that asked (the composer's rule — an action that did not happen must not
 * look like it did).
 *
 * The section is not drawn at all where it would change nothing: a backend that does not call,
 * and one too old to have the setting (see `callingProtocolOffered`).
 */
export function CallingProtocolSettings() {
  const status = useAppState((s) => s.callStatus);
  const controller = useController();
  // The choice a press is waiting on, so the spinner stands on the option that was pressed.
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (!callingProtocolOffered(status)) return null;
  const protocol = status.protocol;
  const keeps = liveCallKeepsItsProtocol(status);

  const choose = async (changes: Partial<CallProtocol>, value: string) => {
    setPending(value);
    setError(null);
    try {
      await controller.setCallProtocol(changes);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(null);
    }
  };

  return (
    <div
      data-testid="calling-protocol"
      className="flex flex-col gap-3 rounded-xl bg-card p-4 shadow-chip"
    >
      <div className="flex flex-col">
        <span className="text-[13px] font-medium text-foreground">
          Calling protocol <span className="font-normal text-text-faint">(experimental)</span>
        </span>
        <span className="text-[11px] text-text-faint">
          How this app's calls speak to Teams, where it can do what the Teams web client does
          instead of what this app has always done.
        </span>
      </div>

      <ProtocolRow
        title="Mute"
        testId="calling-protocol-mute"
        choices={MUTE_PROTOCOL_CHOICES}
        value={protocol.mute}
        pending={pending}
        onChoose={(mute) => void choose({ mute }, mute)}
      />

      <span data-testid="calling-protocol-note" className="text-[11px] leading-snug text-text-faint">
        {CALLING_PROTOCOL_NOTE}
        {keeps && <> {keeps}</>}
      </span>
      {error && (
        <span data-testid="calling-protocol-error" className="text-[11px] leading-snug text-destructive">
          {error}
        </span>
      )}
    </div>
  );
}

/** One protocol choice: a group of 44px-tall presses that each name what they do, the chosen one
 *  pressed.
 *
 *  Pressed buttons rather than a switch because the options are NAMED rather than on and off —
 *  "As before" against "the way Teams does" — and because the incoming-call row this section is
 *  built for has three of them. And pressed buttons rather than `role="radio"`, which is the
 *  app's own convention for one choice among a few (the appearance picker, the diff layout): a
 *  radio group promises arrow-key movement a screen reader user will reach for, and buttons
 *  promise only Tab and Space, which is what these do. The pressed option is the backend's word;
 *  a press only asks. */
function ProtocolRow<T extends string>(props: {
  title: string;
  testId: string;
  choices: readonly ProtocolChoice<T>[];
  value: T;
  /** The option a press is waiting on — every option is held while one is. */
  pending: string | null;
  onChoose: (value: T) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span id={`${props.testId}-title`} className="text-[12px] font-medium text-text-dim">
        {props.title}
      </span>
      <div
        role="group"
        aria-labelledby={`${props.testId}-title`}
        data-testid={props.testId}
        data-value={props.value}
        className="flex flex-col gap-1.5"
      >
        {props.choices.map((choice) => {
          const checked = choice.value === props.value;
          return (
            <button
              key={choice.value}
              type="button"
              aria-pressed={checked}
              data-testid={`${props.testId}-${choice.value}`}
              disabled={props.pending !== null}
              onClick={() => {
                if (!checked) props.onChoose(choice.value);
              }}
              className={cn(
                "flex min-h-11 w-full items-start gap-3 rounded-lg border px-3 py-2 text-left transition-colors",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                "disabled:cursor-default",
                checked
                  ? "border-primary/40 bg-primary/10"
                  : "border-transparent bg-field hover:bg-element",
              )}
            >
              <span
                aria-hidden
                className={cn(
                  "mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border",
                  checked ? "border-primary" : "border-text-faint",
                )}
              >
                {checked && <span className="size-2 rounded-full bg-primary" />}
              </span>
              <span className="flex min-w-0 flex-col">
                <span className="text-[13px] text-foreground">{choice.label}</span>
                <span className="text-[11px] leading-snug text-text-faint">{choice.detail}</span>
              </span>
              {props.pending === choice.value && <FadeArc className="ml-auto size-4 shrink-0" />}
            </button>
          );
        })}
      </div>
    </div>
  );
}
