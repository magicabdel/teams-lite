// A notice shown again while its predecessor is still LEAVING must not be deleted with it.
//
// Sonner keeps a dismissed toast mounted for its exit animation and then deletes the entry by
// id, so a second failed attempt — which takes the old reason back as it starts and fails
// within milliseconds — used to draw its sentence for a fifth of a second. The rule is pinned
// on the module's own calls into sonner, with fake time, because a browser shows it only as a
// flash nobody can read.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toastCalls: { text: string; id: string; type?: string }[] = [];
const dismissed: string[] = [];

vi.mock("sonner", () => {
  const toast = Object.assign(
    (text: string, options: { id: string }) => {
      // Untyped: the one call `present` must never make, because sonner MERGES a toast into
      // the one under its id and an untyped one keeps that one's type.
      toastCalls.push({ text, id: options.id });
    },
    {
      error: (text: string, options: { id: string }) => {
        toastCalls.push({ text, id: options.id, type: "error" });
      },
      info: (text: string, options: { id: string }) => {
        toastCalls.push({ text, id: options.id, type: "info" });
      },
      dismiss: (id: string) => {
        dismissed.push(id);
      },
    },
  );
  return { toast };
});

const { dismissNotice, showNotice } = await import("./notice");

function notice(text: string) {
  return { text, kind: "error" as const, id: "call", testId: "call-notice" };
}

describe("a notice and the one it replaces", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", {});
    toastCalls.length = 0;
    dismissed.length = 0;
    // Each case starts well clear of any earlier dismissal.
    vi.advanceTimersByTime(10_000);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("is drawn at once when nothing was taken back", () => {
    showNotice(notice("first"));
    expect(toastCalls).toEqual([{ text: "first", id: "call", type: "error" }]);
  });

  it("waits for a notice that is still leaving, rather than being deleted with it", () => {
    showNotice(notice("first"));
    dismissNotice("call");
    showNotice(notice("second"));
    expect(toastCalls).toEqual([{ text: "first", id: "call", type: "error" }]);
    vi.advanceTimersByTime(300);
    expect(toastCalls).toEqual([
      { text: "first", id: "call", type: "error" },
      { text: "second", id: "call", type: "error" },
    ]);
  });

  it("draws only the last of several that land while one is leaving", () => {
    dismissNotice("call");
    showNotice(notice("second"));
    showNotice(notice("third"));
    vi.advanceTimersByTime(300);
    expect(toastCalls).toEqual([{ text: "third", id: "call", type: "error" }]);
  });

  it("drops a waiting notice when its attempt is taken back too", () => {
    dismissNotice("call");
    showNotice(notice("stale"));
    dismissNotice("call");
    vi.advanceTimersByTime(1_000);
    expect(toastCalls).toEqual([]);
  });

  /** A REPORT that replaces a failure under the same id must not keep the failure's red ink:
   *  sonner merges a toast into the one under its id, so an untyped report kept `type:
   *  "error"`. Measured on the call capture — "You answered on another device." drawn red,
   *  because a refused camera had been said a moment before. */
  it("states a report's own type, so it never inherits a failure's", () => {
    // An id of its own: the cases above leave "call" dismissed at a later fake moment.
    const id = "call-kind";
    showNotice({ ...notice("the camera was refused"), id });
    showNotice({ ...notice("You answered on another device."), kind: "report", id });
    expect(toastCalls).toEqual([
      { text: "the camera was refused", id, type: "error" },
      { text: "You answered on another device.", id, type: "info" },
    ]);
  });

  it("leaves another subject's notice alone", () => {
    dismissNotice("call");
    showNotice({ ...notice("the file"), id: "call-recording" });
    expect(toastCalls).toEqual([{ text: "the file", id: "call-recording", type: "error" }]);
  });
});
