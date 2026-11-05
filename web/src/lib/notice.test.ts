// A notice shown again while its predecessor is still LEAVING must not be deleted with it.
//
// Sonner keeps a dismissed toast mounted for its exit animation and then deletes the entry by
// id, so a second failed attempt — which takes the old reason back as it starts and fails
// within milliseconds — used to draw its sentence for a fifth of a second. The rule is pinned
// on the module's own calls into sonner, with fake time, because a browser shows it only as a
// flash nobody can read.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toastCalls: { text: string; id: string }[] = [];
const dismissed: string[] = [];

vi.mock("sonner", () => {
  const toast = Object.assign(
    (text: string, options: { id: string }) => {
      toastCalls.push({ text, id: options.id });
    },
    {
      error: (text: string, options: { id: string }) => {
        toastCalls.push({ text, id: options.id });
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
    expect(toastCalls).toEqual([{ text: "first", id: "call" }]);
  });

  it("waits for a notice that is still leaving, rather than being deleted with it", () => {
    showNotice(notice("first"));
    dismissNotice("call");
    showNotice(notice("second"));
    expect(toastCalls).toEqual([{ text: "first", id: "call" }]);
    vi.advanceTimersByTime(300);
    expect(toastCalls).toEqual([
      { text: "first", id: "call" },
      { text: "second", id: "call" },
    ]);
  });

  it("draws only the last of several that land while one is leaving", () => {
    dismissNotice("call");
    showNotice(notice("second"));
    showNotice(notice("third"));
    vi.advanceTimersByTime(300);
    expect(toastCalls).toEqual([{ text: "third", id: "call" }]);
  });

  it("drops a waiting notice when its attempt is taken back too", () => {
    dismissNotice("call");
    showNotice(notice("stale"));
    dismissNotice("call");
    vi.advanceTimersByTime(1_000);
    expect(toastCalls).toEqual([]);
  });

  it("leaves another subject's notice alone", () => {
    dismissNotice("call");
    showNotice({ ...notice("the file"), id: "call-recording" });
    expect(toastCalls).toEqual([{ text: "the file", id: "call-recording" }]);
  });
});
