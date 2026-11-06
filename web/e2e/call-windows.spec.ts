import type { Locator, Page } from "@playwright/test";
import {
  armNextCallMute,
  callFromMenu,
  callRecord,
  dropBackendSockets,
  emitCallInvite,
  emitUnchangedCallState,
  expect,
  gotoApp,
  holdCallStart,
  mockRpc,
  muteWhileDisconnected,
  openConversationNamed,
  openSecondWindow,
  resetCall,
  test,
} from "./helpers";

// One call, several windows — and the microphone follows the call (incoming-and-conversation M1;
// web/src/lib/call.ts, `followMuteFrame` and `followMuteAnswer`, over the backend's `mute_seq`).
//
// A call is drawn in every window that has the app open, and only the one that answered it
// carries its audio. Before this, Mute pressed in another window — the phone, while the laptop
// held the call — published a mute and left the laptop sending the user's voice. What these
// specs pin is the DIRECTION a mistake may go in: a frame may close the microphone, and only the
// window's own press may open it. So every test reads the window that holds the call
// (`data-microphone`, off its TRACK), and the ones about something that must NOT move it record
// every value that attribute took, because a microphone opened for one frame and closed again is
// exactly the failure and a final-state assertion would pass over it.
//
// The mock plays the backend: it records presses one at a time and counts them, can fail, hold or
// lose the answer to the next one (`armNextCallMute`), can record one while every page is
// disconnected (`muteWhileDisconnected`), and can send a frame changed in nothing
// (`emitUnchangedCallState`) — the frame from before a press that must never undo it. Every test
// ends with `resetCall`.

test.describe("One call in several windows", () => {
  test.afterEach(async ({ page }) => {
    await resetCall(page);
  });

  test("a second window's Mute silences the microphone the first one holds, and cannot unmute it", async ({
    page,
  }) => {
    const callId = await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    await expect(stage).toHaveAttribute("data-microphone", "live");

    const second = await openSecondWindow(page);
    try {
      const otherStage = second.locator('[data-testid="call-stage"]');
      await expect(otherStage).toHaveAttribute("data-phase", "connected");
      // It draws the call and holds none of it: no media, so no track to report.
      await expect(otherStage).toHaveAttribute("data-media", "none");
      await expect(otherStage).not.toHaveAttribute("data-microphone", /./);
      const otherMute = second.locator('[data-testid="call-mute"]').first();
      await expect(otherMute).toHaveAttribute("aria-pressed", "false");
      await expect(second.locator('[data-testid="call-unmute-refusal"]')).toHaveCount(0);

      await otherMute.click();
      // The press in the second window closed the TRACK in the first.
      await expect(stage).toHaveAttribute("data-microphone", "muted");
      await expect(page.locator('[data-testid="call-mute"]').first()).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      // And the second window may not take it back: a frame never opens a microphone, so its
      // Unmute would change what the meeting is told and nothing anybody could hear. It says so
      // IN FLOW, on a line of its own that a phone's crowded header cannot clip.
      await expect(otherMute).toHaveAttribute("aria-pressed", "true");
      await expect(otherMute).toBeDisabled();
      const refusal = second.locator('[data-testid="call-unmute-refusal"]');
      await expect(refusal).toBeVisible();
      await expect(refusal).toContainText("does not carry the call's audio");
      await expect(otherMute).toHaveAttribute("aria-label", /does not carry the call's audio/);
      // On a phone's width the header row has no room left for a sentence, which is why the
      // reason is a line of its own: it WRAPS rather than being clipped.
      const desktop = second.viewportSize();
      await second.setViewportSize({ width: 390, height: 844 });
      await expect(refusal).toBeVisible();
      // Polled, because the stage animates its own box to a new window size.
      await expect
        .poll(() =>
          refusal.evaluate((element) => ({
            clipped: element.scrollWidth > element.clientWidth,
            insideTheScreen: element.getBoundingClientRect().right <= window.innerWidth,
          })),
        )
        .toEqual({ clipped: false, insideTheScreen: true });
      if (desktop) await second.setViewportSize(desktop);
      // Folded away, the window's one status line says it instead.
      await second.locator('[data-testid="call-stage-minimize"]').click();
      await expect(otherStage).toHaveAttribute("data-mode", "mini");
      await expect(second.locator('[data-testid="call-mini-unmute-refusal"]')).toHaveText(
        "Muted · no audio in this window",
      );
      await second.locator('[data-testid="call-stage-expand"]').click();

      // A window this build did not write — an older page, a phone on a build before this — can
      // still send an Unmute. It moves the backend, and never this microphone.
      const seen = await watchMicrophone(stage);
      const unmute = await mockRpc("call_mute", { call_id: callId, muted: false });
      expect(unmute.error).toBeUndefined();
      await expect(stage).toHaveAttribute("data-mute-seq", "2");
      expect(await seen()).toEqual([]);

      // The window holding the call unmutes it, and the second window follows.
      await page.bringToFront();
      await page.locator('[data-testid="call-mute"]').first().click();
      await expect(stage).toHaveAttribute("data-microphone", "live");
      await expect(otherMute).toHaveAttribute("aria-pressed", "false");
      await expect(otherMute).toBeEnabled();
      await expect(second.locator('[data-testid="call-unmute-refusal"]')).toHaveCount(0);
    } finally {
      await second.close();
    }
  });

  test("a second window's Mute that was refused says so there, and changes nothing", async ({
    page,
  }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    const second = await openSecondWindow(page);
    try {
      await expect(second.locator('[data-testid="call-stage"]')).toHaveAttribute("data-phase", "connected");
      await armNextCallMute(page, { fail: true });
      const holderSaid = await watchForNotice(page);
      const otherMute = second.locator('[data-testid="call-mute"]').first();
      await otherMute.click();
      await expect(second.locator('[data-testid="call-notice"]')).toContainText(
        "The mute did not reach the call",
      );
      // Nothing closed anywhere, and nothing claims it did — in the window holding the call
      // nothing was pressed, so nothing is said there either.
      await expect(otherMute).toHaveAttribute("aria-pressed", "false");
      await expect(stage).toHaveAttribute("data-microphone", "live");
      expect(await holderSaid()).toBe(false);
    } finally {
      await second.close();
    }
  });

  test("a second window's Mute whose answer was lost says nothing false", async ({ page }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    const second = await openSecondWindow(page);
    try {
      await expect(second.locator('[data-testid="call-stage"]')).toHaveAttribute("data-phase", "connected");
      // Recorded and told to every page, and then the answer is lost with the sockets.
      await armNextCallMute(page, { lose_answer: true });
      const said = await watchForNotice(second);
      await second.locator('[data-testid="call-mute"]').first().click();
      await expect(stage).toHaveAttribute("data-microphone", "muted");
      // "Nothing was muted" would be false: the microphone the call is heard through is closed.
      // Recorded rather than counted at the end: the notice leaves on its own, so an assertion
      // that waits for it to be gone passes over one that was shown.
      await afterTheAnswerWouldHaveCome(second);
      expect(await said()).toBe(false);
      await expect(second.locator('[data-testid="call-mute"]').first()).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    } finally {
      await second.close();
    }
  });

  test("a frame from before a press never undoes it: a Mute that failed, an Unmute on its way", async ({
    page,
  }) => {
    const callId = await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    const mute = page.locator('[data-testid="call-mute"]').first();
    await expect(stage).toHaveAttribute("data-mute-seq", "0");

    // A Mute that never reaches the backend. The track closed at the press, and every frame after
    // it is still from before it — saying "live".
    await armNextCallMute(page, { fail: true });
    let seen = await watchMicrophone(stage);
    await mute.click();
    await expect(stage).toHaveAttribute("data-microphone", "muted");
    await expect.poll(async () => (await callRecord(page)).mutes).toEqual([
      { muted: true, refused: true },
    ]);
    expect(await emitUnchangedCallState(page)).toEqual({ muted: false, mute_seq: 0 });
    // A frame ordered AFTER the stale one, so its arrival proves the stale one was read.
    expect((await mockRpc("call_mute", { call_id: callId, muted: true })).error).toBeUndefined();
    await expect(stage).toHaveAttribute("data-mute-seq", "1");
    expect(await seen()).toEqual(["muted"]);

    // An Unmute still on its way, and a frame from before it — saying "muted" — arriving first.
    await armNextCallMute(page, { hold_ms: 1500 });
    seen = await watchMicrophone(stage);
    await mute.click();
    await expect(stage).toHaveAttribute("data-microphone", "live");
    expect(await emitUnchangedCallState(page)).toEqual({ muted: true, mute_seq: 1 });
    // The held press lands as the next frame, which is again ordered after the stale one.
    await expect(stage).toHaveAttribute("data-mute-seq", "2");
    await expect(stage).toHaveAttribute("data-microphone", "live");
    expect(await seen()).toEqual(["live"]);
    await expect(mute).toHaveAttribute("aria-pressed", "false");
  });

  test("Mute then Unmute inside one round trip lands on the last press", async ({ page }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    const mute = page.locator('[data-testid="call-mute"]').first();

    // The Mute is held, so the Unmute is pressed before its frame comes back. Its frame then
    // closes the microphone and the Unmute's own frame cannot open it: only the backend's answer
    // to the newest press settles it.
    await armNextCallMute(page, { hold_ms: 800 });
    await mute.click();
    await expect(mute).toHaveAttribute("aria-pressed", "true");
    await mute.click();
    await expect(stage).toHaveAttribute("data-mute-seq", "2");
    await expect(stage).toHaveAttribute("data-microphone", "live");
    await expect(mute).toHaveAttribute("aria-pressed", "false");
    const status = (await mockRpc("call_status", {})).result as { call: { muted: boolean } };
    expect(status.call.muted).toBe(false);
  });

  test("an Unmute that did not arrive takes itself back, and is never sent again", async ({
    page,
  }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    const mute = page.locator('[data-testid="call-mute"]').first();
    await mute.click();
    await expect(stage).toHaveAttribute("data-mute-seq", "1");

    // Live under a backend, a meeting and every other window that still say "muted" is the one
    // state no other window could do anything about — so the failed Unmute closes it again.
    await armNextCallMute(page, { fail: true });
    const seen = await watchMicrophone(stage);
    await mute.click();
    await expect(stage).toHaveAttribute("data-microphone", "muted");
    expect(await seen()).toEqual(["live", "muted"]);
    await expect(mute).toHaveAttribute("aria-pressed", "true");

    // And a reconnect does not open it on a press the user watched fail.
    const reads = (await callRecord(page)).status_reads;
    await dropBackendSockets(page);
    await expect.poll(async () => (await callRecord(page)).status_reads).toBeGreaterThan(reads);
    await afterTheAnswerWouldHaveCome(page);
    expect((await callRecord(page)).mutes).toEqual([
      { muted: true, refused: false },
      { muted: false, refused: true },
    ]);
    await expect(stage).toHaveAttribute("data-microphone", "muted");
  });

  test("a Mute pressed while this window could not hear is applied when it reconnects", async ({
    page,
  }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    // The phone mutes while the laptop's socket is down: no frame reaches the laptop, so the
    // status read it makes on reconnecting is the only way it learns.
    await muteWhileDisconnected(page, true);
    await expect(stage).toHaveAttribute("data-mute-seq", "1");
    await expect(stage).toHaveAttribute("data-microphone", "muted");
  });

  test("a Mute that did not arrive is sent again when this window reconnects", async ({ page }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');

    await armNextCallMute(page, { fail: true });
    await page.locator('[data-testid="call-mute"]').first().click();
    await expect(stage).toHaveAttribute("data-microphone", "muted");
    await expect.poll(async () => (await callRecord(page)).mutes).toEqual([
      { muted: true, refused: true },
    ]);

    // The socket comes back, the call is still up and nothing was pressed since: the press is
    // delivered — the user's own, lost on the way, and not a decision.
    await dropBackendSockets(page);
    await expect.poll(async () => (await callRecord(page)).mutes).toEqual([
      { muted: true, refused: true },
      { muted: true, refused: false },
    ]);
    await expect(stage).toHaveAttribute("data-mute-seq", "1");
    await expect(stage).toHaveAttribute("data-microphone", "muted");
  });

  test("a lost Mute is sent again only once, even when that fails too", async ({ page }) => {
    await answerACall(page);
    await armNextCallMute(page, { fail: true });
    await page.locator('[data-testid="call-mute"]').first().click();
    await expect.poll(async () => (await callRecord(page)).mutes).toHaveLength(1);

    // The re-send is refused as well…
    await armNextCallMute(page, { fail: true });
    await dropBackendSockets(page);
    await expect.poll(async () => (await callRecord(page)).mutes).toEqual([
      { muted: true, refused: true },
      { muted: true, refused: true },
    ]);
    // …and a second reconnect sends nothing more.
    const reads = (await callRecord(page)).status_reads;
    await dropBackendSockets(page);
    await expect.poll(async () => (await callRecord(page)).status_reads).toBeGreaterThan(reads);
    await afterTheAnswerWouldHaveCome(page);
    expect((await callRecord(page)).mutes).toHaveLength(2);
    await expect(page.locator('[data-testid="call-stage"]')).toHaveAttribute("data-microphone", "muted");
  });

  test("a lost Mute is never sent over a press made while this window could not hear", async ({
    page,
  }) => {
    await answerACall(page);
    const stage = page.locator('[data-testid="call-stage"]');
    await armNextCallMute(page, { fail: true });
    await page.locator('[data-testid="call-mute"]').first().click();
    await expect.poll(async () => (await callRecord(page)).mutes).toHaveLength(1);

    // A window this build did not write presses Unmute during the outage. The lost Mute is older
    // than that press, so it is not a press to deliver any more — and the microphone it closed
    // stays closed, because no frame opens one.
    await muteWhileDisconnected(page, false);
    await expect(stage).toHaveAttribute("data-mute-seq", "1");
    await afterTheAnswerWouldHaveCome(page);
    expect((await callRecord(page)).mutes).toEqual([
      { muted: true, refused: true },
      { muted: false, refused: false },
    ]);
    await expect(stage).toHaveAttribute("data-microphone", "muted");
  });

  test("the window still opening its microphone keeps its own Mute, and may unmute", async ({
    page,
  }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Ava Thompson");
    // The reservation answers late: the stage is up and dialling while this window's microphone
    // — and the browser's permission prompt — is still opening.
    await holdCallStart(page, "prepare", 1500);
    await callFromMenu(page);
    const stage = page.locator('[data-testid="call-stage"]');
    await expect(stage).toHaveAttribute("data-phase", "dialing");
    await expect(stage).toHaveAttribute("data-media", "none");

    const mute = page.locator('[data-testid="call-mute"]').first();
    await mute.click();
    // The backend has the press, and this window has its frame — and still holds no microphone.
    await expect(stage).toHaveAttribute("data-mute-seq", "1");
    await expect(mute).toHaveAttribute("aria-pressed", "true");
    // This is the window about to carry the audio, so it is not told it cannot unmute. Read AT
    // ONCE, while the microphone is still opening: the refusal would go by itself the moment it
    // is adopted, so an assertion that waited would pass over one that was drawn.
    expect(await stage.getAttribute("data-media")).toBe("none");
    expect(await page.locator('[data-testid="call-unmute-refusal"]').count()).toBe(0);
    expect(await mute.isEnabled()).toBe(true);

    // The press is applied to the microphone the moment it is adopted.
    await expect(stage).toHaveAttribute("data-media", "held");
    await expect(stage).toHaveAttribute("data-microphone", "muted");
    await expect(mute).toHaveAttribute("aria-pressed", "true");
  });
});

test.describe("One ring in several windows", () => {
  test.afterEach(async ({ page }) => {
    await resetCall(page);
  });

  /** The backend CLAIMS a ring when an Answer starts carrying it out (incoming-and-conversation
   *  M4a), so a second window cannot answer it too — two attaches would claim the leg twice. The
   *  window answering keeps its "Answering…"; every other window offers no Answer and says why;
   *  and nothing in the second window ever posts. */
  test("a ring another window is answering offers no Answer here, and says why", async ({ page }) => {
    await ringBothWindows(page, async (second) => {
      await holdCallStart(page, "accept", 2500);
      await page.locator('[data-testid="call-answer"]').click();
      await expect(page.locator('[data-testid="call-answer"]')).toHaveText("Answering…");
      // The window answering is not told another window is: it is the one.
      await expect(page.locator('[data-testid="call-accept-blocked"]')).toHaveCount(0);
      await expect(page.locator('[data-testid="call-phase"]')).toBeVisible();
      await expect(second.locator('[data-testid="call-answer"]')).toHaveCount(0);
      await expect(second.locator('[data-testid="call-accept-blocked"]')).toHaveText(
        "This call is being answered in another window.",
      );
      // The answer lands, and both windows draw the live call it made.
      await expect(page.locator('[data-testid="call-stage"]')).toHaveAttribute("data-phase", "connected");
      await expect(second.locator('[data-testid="call-stage"]')).toHaveAttribute("data-phase", "connected");
      expect((await callRecord(page)).hangups).toEqual([]);
    });
  });

  /** Decline pressed in one window while another window's Answer is on the wire: two presses that
   *  disagree, and a decline is the one of them the caller is SHOWN and nothing takes back. So the
   *  ring is let go here and the caller is never declined. */
  test("a Decline under another window's Answer never declines the caller", async ({ page }) => {
    await ringBothWindows(page, async (second) => {
      await holdCallStart(page, "accept", 2500);
      await page.locator('[data-testid="call-answer"]').click();
      await expect(second.locator('[data-testid="call-accept-blocked"]')).toBeVisible();
      await second.locator('[data-testid="call-hangup"]').click();
      await expect.poll(async () => (await callRecord(page)).hangups).toEqual([
        { intent: "decline", declined: false },
      ]);
      // And neither window leaves its reader guessing: the one that pressed Decline says the
      // caller was not declined, and the one that was answering says the call was let go.
      await expect(second.locator('[data-testid="call-notice"]')).toContainText(
        "Another window was answering this call, so the caller was not declined.",
      );
      await expect(page.locator('[data-testid="call-notice"]')).toContainText(
        "The call was let go in another window before it was answered.",
      );
    });
  });

  /** The ANSWERING window's own Decline is the user changing their mind, and declines as it
   *  always did — its press names the answer that holds the claim. */
  test("the answering window's own Decline still declines", async ({ page }) => {
    await ringBothWindows(page, async () => {
      await holdCallStart(page, "accept", 2500);
      await page.locator('[data-testid="call-answer"]').click();
      await expect(page.locator('[data-testid="call-answer"]')).toHaveText("Answering…");
      await page.locator('[data-testid="call-hangup"]').click();
      await expect.poll(async () => (await callRecord(page)).hangups).toEqual([
        { intent: "decline", declined: true },
      ]);
    });
  });

  /** Two Answers that RACE — the second window's still opening its microphone while the first
   *  takes the call — meet at the backend's claim: the late one posts nothing, gives its
   *  microphone back, and says where the call went. Never a decline, never a failure. */
  test("an Answer that lost the race says the call was answered in another window", async ({ page }) => {
    await ringBothWindows(page, async (second) => {
      await holdCallStart(page, "answer", 2500);
      // The second window's answer is held while it would be opening its microphone; the first
      // window's, pressed meanwhile, goes straight through and takes the call.
      await second.locator('[data-testid="call-answer"]').click();
      await page.locator('[data-testid="call-answer"]').click();
      await expect(page.locator('[data-testid="call-stage"]')).toHaveAttribute("data-phase", "connected");
      await expect(second.locator('[data-testid="call-notice"]')).toContainText(
        "This call was answered in another window.",
      );
      await expect(second.locator('[data-testid="call-stage"]')).toHaveAttribute("data-media", "none");
      await expect(page.locator('[data-testid="call-stage"]')).toHaveAttribute("data-media", "held");
      expect((await callRecord(page)).hangups).toEqual([]);
    });
  });
});

/** Ring BOTH windows with one one-to-one call, then hand the second window to `act`. */
async function ringBothWindows(page: Page, act: (second: Page) => Promise<void>): Promise<void> {
  await gotoApp(page);
  await openConversationNamed(page, "Ava Thompson");
  const conversationId = await page
    .locator('[data-testid="composer-shell"]')
    .getAttribute("data-conversation-id");
  expect(conversationId).toBeTruthy();
  const second = await openSecondWindow(page);
  try {
    await emitCallInvite(page, conversationId!);
    await expect(page.locator('[data-testid="call-bar"]')).toHaveAttribute("data-phase", "ringing");
    await expect(second.locator('[data-testid="call-bar"]')).toHaveAttribute("data-phase", "ringing");
    await act(second);
  } finally {
    await second.close();
  }
}

/** Ring the first window with a one-to-one call and answer it there, so it holds the call's
 *  microphone. Returns the call's id. */
async function answerACall(page: Page): Promise<string> {
  await gotoApp(page);
  await openConversationNamed(page, "Ava Thompson");
  const conversationId = await page
    .locator('[data-testid="composer-shell"]')
    .getAttribute("data-conversation-id");
  expect(conversationId).toBeTruthy();
  const callId = await emitCallInvite(page, conversationId!);
  await page.locator('[data-testid="call-answer"]').click();
  const stage = page.locator('[data-testid="call-stage"]');
  await expect(stage).toHaveAttribute("data-phase", "connected");
  await expect(stage).toHaveAttribute("data-media", "held");
  return callId;
}

/** Give a press, a re-send or a notice the moment it would take. A spec proving that NOTHING
 *  happened has to wait past the moment it would have: a re-send goes out as soon as the page
 *  has read the answer to its status read, which the mock counts when it answers it — before
 *  the page has read it. */
async function afterTheAnswerWouldHaveCome(page: Page): Promise<void> {
  await page.waitForTimeout(900);
}

/** Record whether a call notice appears on `page` from now on. A notice leaves on its own, so
 *  "no notice" has to be watched for rather than counted afterwards. */
async function watchForNotice(page: Page): Promise<() => Promise<boolean>> {
  const key = `__noticeSeen${Math.floor(Math.random() * 1e9)}`;
  await page.evaluate((slot) => {
    const flags = window as unknown as Record<string, boolean>;
    flags[slot] = document.querySelector('[data-testid="call-notice"]') !== null;
    new MutationObserver(() => {
      if (document.querySelector('[data-testid="call-notice"]')) flags[slot] = true;
    }).observe(document.body, { childList: true, subtree: true });
  }, key);
  return () => page.evaluate((slot) => (window as unknown as Record<string, boolean>)[slot] === true, key);
}

/**
 * Record every value `data-microphone` takes on `stage` from now on, and hand back a reader of
 * that list. A microphone opened for one frame and closed again is the failure these specs exist
 * for, and an assertion about where it ended up would pass straight over it.
 */
async function watchMicrophone(stage: Locator): Promise<() => Promise<string[]>> {
  const key = `__micSeen${Math.floor(Math.random() * 1e9)}`;
  await stage.evaluate((element, slot) => {
    const seen: string[] = [];
    (window as unknown as Record<string, string[]>)[slot] = seen;
    let last = element.getAttribute("data-microphone");
    new MutationObserver(() => {
      const now = element.getAttribute("data-microphone");
      if (now !== last && now !== null) seen.push(now);
      last = now;
    }).observe(element, { attributes: true, attributeFilter: ["data-microphone"] });
  }, key);
  return () =>
    stage.page().evaluate((slot) => [...(window as unknown as Record<string, string[]>)[slot]!], key);
}
