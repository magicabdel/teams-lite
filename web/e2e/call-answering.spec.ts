import { expect, type Page } from "@playwright/test";
import { CALL_END_UNANSWERED } from "../src/lib/call";
import {
  callFromMenu,
  callRecord,
  emitCallInvite,
  endCallWithCode,
  endCallWithReason,
  gotoApp,
  holdCallStart,
  joinMeetingFromMenu,
  nobodyAnswersNextCall,
  openConversationNamed,
  resetCall,
  test,
} from "./helpers";

// A RING that ended somewhere else, and one nobody answered — what the app SAYS about a call
// the user did not end themselves (docs/calling-roadmap/incoming-and-conversation.md § 4.12 and
// § 7). Each of these used to read "The call ended.", which is the one sentence that tells the
// user nothing about a call they missed, or took on their phone.
//
// The mock states every ending the way the service's own frame does — a code, and who took the
// call — and reads it exactly as the backend does (`calling::end_reason_for`). That is the only
// way to these endings today: on the tenant, an endpoint that never attached to the ring is told
// none of them (NATIVE-CALLING.md § 8a). The one that IS reachable today is the backend's own
// ring bound, in both directions.

/** The mock's own account (`SELF_MRI` in web/mock/server.ts), spelled in another case on
 *  purpose: the ids come from two services and nothing measured that they agree on case. */
const SELF_IN_ANOTHER_CASE = "8:ORGID:00000000-0000-4000-8000-000000000000";

/** The notice that is current — sonner keeps the one it is replacing mounted for its exit. */
function currentNotice(page: Page) {
  return page.locator('[data-testid="call-notice"]:not([data-removed="true"])');
}

/** Open Ava's chat and ring this machine from it; resolves once the card is ringing. */
async function ringFromAva(page: Page): Promise<void> {
  const conversation = await page
    .locator('[data-testid="composer-shell"]')
    .getAttribute("data-conversation-id");
  expect(conversation).toBeTruthy();
  await emitCallInvite(page, conversation!);
  await expect(page.locator('[data-testid="call-bar"]')).toHaveAttribute("data-phase", "ringing");
}

test.describe("a ring that ended elsewhere", () => {
  test.afterEach(async ({ page }) => resetCall(page));

  /** E2E 1. The card goes AT ONCE — a ring nobody here can take any more is not one to keep
   *  drawing — and one sentence says who took it. The user's own other device is "you"; anybody
   *  else is named the way this app names them, never by the frame's own words where the app
   *  knows better. And nothing is sent: the user did not decline, so nothing must say they did. */
  test("goes at once and says who answered it, by self and by another", async ({ page }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Ava Thompson");
    const bar = page.locator('[data-testid="call-bar"]');

    await ringFromAva(page);
    await endCallWithCode(page, 450, { id: SELF_IN_ANOTHER_CASE, displayName: "You, on the phone" });
    await expect(bar).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("You answered on another device.");

    // Somebody else — a delegate picking it up. The frame's own name is NOT what is drawn:
    // the mock's directory knows this person, which is the backend's `display_name_for_mri`.
    await ringFromAva(page);
    await endCallWithCode(page, 450, { id: "8:orgid:liam-nguyen", displayName: "L. Nguyen (delegate)" });
    await expect(bar).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("Liam Nguyen answered this call.");

    // Forwarded, and sent to voicemail: the other two endings a ring has that are not a miss.
    await ringFromAva(page);
    await endCallWithCode(page, 451);
    await expect(currentNotice(page)).toHaveText("The call was forwarded.");
    await ringFromAva(page);
    await endCallWithCode(page, 452);
    await expect(currentNotice(page)).toHaveText("The call went to voicemail.");

    // Nothing reached the service for any of them: no decline, no hangup of any kind.
    expect((await callRecord(page)).hangups).toEqual([]);
  });

  /** The same codes said about a call the user is IN describe nothing: "you answered on
   *  another device" about a conversation they are having would be false. */
  test("never says a call the user answered here ended elsewhere", async ({ page }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Ava Thompson");
    await ringFromAva(page);
    await page.locator('[data-testid="call-answer"]').click();
    const stage = page.locator('[data-testid="call-stage"]');
    await expect(stage).toHaveAttribute("data-phase", "connected", { timeout: 10_000 });

    await endCallWithCode(page, 450, { id: SELF_IN_ANOTHER_CASE, displayName: "You" });
    await expect(stage).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("The call ended.");
  });

  /** Nor about one the user is JOINING: a meeting join sits in `connecting`, with nothing
   *  connected — the phase a ring of ours has too — so it is the call's DIRECTION that keeps a
   *  487 from reading "Missed call". The join is held on the wire so the ending lands inside
   *  it, which is the whole lobby wait on the tenant. */
  test("never says a meeting being joined was missed", async ({ page }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Design Sync");
    await holdCallStart(page, "join", 2_000);
    await joinMeetingFromMenu(page);
    const stage = page.locator('[data-testid="call-stage"]');
    await expect(stage).toHaveAttribute("data-phase", "connecting");
    // The microphone is adopted and the join POST is out: this is a call the user is in.
    await expect(stage).toHaveAttribute("data-media", "held");

    await endCallWithCode(page, 487);
    await expect(stage).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("The call ended.");
  });
});

test.describe("a ring nobody answered", () => {
  test.afterEach(async ({ page }) => resetCall(page));

  /** E2E 2. Which side nobody was on is the whole sentence: a ring the user never took is a
   *  missed call — whether the caller gave up (487) or this app's own ring bound ran out — and a
   *  call they placed that nobody took says that person did not answer. */
  test("says a ring was missed, and that a call placed was not answered", async ({ page }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Ava Thompson");
    const bar = page.locator('[data-testid="call-bar"]');

    // The caller gave up.
    await ringFromAva(page);
    await endCallWithCode(page, 487);
    await expect(bar).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("Missed call from Ava Thompson.");

    // A call the USER placed, ringing Ava's devices until the bound runs out. It stands
    // BETWEEN the two missed calls on purpose: a ring dismisses no notice, so a second
    // "Missed call…" asserted right after the first would pass on the first one still
    // standing, and could not tell a ring bound that said nothing.
    await nobodyAnswersNextCall(page);
    await callFromMenu(page);
    const stage = page.locator('[data-testid="call-stage"]');
    await expect(stage).toHaveAttribute("data-phase", "dialing");
    await expect(stage).toHaveCount(0, { timeout: 10_000 });
    await expect(currentNotice(page)).toHaveText("Ava Thompson did not answer.");

    // This app's own ring bound ran out on a ring (`CALL_RING_TIMEOUT`, a minute on the
    // backend) — the notice before it says something else, so this one is the bound's own.
    await ringFromAva(page);
    await endCallWithReason(page, CALL_END_UNANSWERED);
    await expect(bar).toHaveCount(0);
    await expect(currentNotice(page)).toHaveText("Missed call from Ava Thompson.");

    // The bound sends nothing, in either direction: declining on the user's behalf is a thing
    // the caller would be SHOWN, which a timer must not decide.
    expect((await callRecord(page)).hangups).toEqual([]);
  });
});
