import { expect, type Page } from "@playwright/test";
import {
  disableCalling,
  emitCallInvite,
  gotoApp,
  openConversationNamed,
  realErrors,
  refuseNextCallProtocol,
  resetCall,
  test,
} from "./helpers";

// Settings › This app › Calling protocol (docs/calling-roadmap/incoming-and-conversation.md
// § 6 and § 7): how this window's backend's calls speak to Teams, where it can do what the
// Teams web client does instead of what this app has always done. One row today, the mute.
//
// What the PAGE owns, and what these pin: the choice drawn is the BACKEND's, never this page's
// memory of a press; a refused press is said at the row and leaves the old choice standing; a
// live call that keeps the protocol it started with says so; and a backend whose calls cannot
// change draws no row at all. Which door the backend then publishes on is pinned in Rust,
// where the decision is.

/** Open Settings › This app and hand back the protocol section. */
async function openProtocol(page: Page) {
  await page.locator('[data-testid="open-settings"]').click();
  const section = page.locator('[data-testid="calling-protocol"]');
  await section.scrollIntoViewIfNeeded();
  return section;
}

test.describe("Calling protocol", () => {
  test.afterEach(async ({ page }) => resetCall(page));

  test("offers the mute protocol, starts on this app's own flow, and keeps the choice", async ({
    page,
    consoleErrors,
  }) => {
    await gotoApp(page);
    const section = await openProtocol(page);
    await expect(section).toBeVisible();
    const row = page.getByTestId("calling-protocol-mute");
    const asBefore = page.getByTestId("calling-protocol-mute-legacy");
    const teams = page.getByTestId("calling-protocol-mute-endpoint-state");
    await expect(asBefore).toHaveAttribute("aria-pressed", "true");
    await expect(teams).toHaveAttribute("aria-pressed", "false");
    // The group is named by its own title, so a screen reader says what the two presses choose.
    await expect(page.getByRole("group", { name: "Mute" })).toBeVisible();
    await expect(page.getByTestId("calling-protocol-note")).toHaveText(
      "Applies to calls this backend starts from now on. Each teams-lite install on this machine keeps its own.",
    );

    // From the keyboard too: Tab reaches an option and Space presses it.
    await teams.focus();
    await page.keyboard.press("Space");
    await expect(row).toHaveAttribute("data-value", "endpoint-state");
    await expect(teams).toHaveAttribute("aria-pressed", "true");
    await expect(asBefore).toHaveAttribute("aria-pressed", "false");

    // The choice is the BACKEND's: a fresh page reads it back rather than defaulting.
    await page.reload();
    await openProtocol(page);
    await expect(page.getByTestId("calling-protocol-mute")).toHaveAttribute("data-value", "endpoint-state");

    // Every option clears the 44px touch floor.
    for (const option of [asBefore, teams]) {
      const box = await option.boundingBox();
      expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
    }
    expect(realErrors(consoleErrors)).toEqual([]);
  });

  /** A press the backend did not carry out must not look like one that worked. */
  test("says a refused change at the row, and keeps the old choice", async ({ page }) => {
    await gotoApp(page);
    await openProtocol(page);
    // Held on the wire, so the moment the press is still travelling can be looked at: the row
    // goes on drawing the BACKEND's word, and only holds its options while it waits.
    await refuseNextCallProtocol(page, 1_500);
    const teams = page.getByTestId("calling-protocol-mute-endpoint-state");
    await teams.click();
    await expect(teams).toBeDisabled();
    // ONE read each, taken while the press is held: a retrying assertion would wait the hold out
    // and pass on the state after it, which is how a row drawing its own memory got past.
    expect(await page.getByTestId("calling-protocol-mute").getAttribute("data-value")).toBe("legacy");
    expect(await teams.getAttribute("aria-pressed")).toBe("false");
    await expect(teams).toBeDisabled();
    await expect(page.getByTestId("calling-protocol-error")).toContainText("refuse this change");
    await expect(page.getByTestId("calling-protocol-mute")).toHaveAttribute("data-value", "legacy");
    await expect(page.getByTestId("calling-protocol-mute-legacy")).toBeEnabled();
  });

  /** A call keeps the protocol it started with, so a change made during one would otherwise
   *  look like it did nothing to that call — which it deliberately does not. */
  test("says a call already going keeps the way it started with", async ({ page }) => {
    await gotoApp(page);
    await openConversationNamed(page, "Ava Thompson");
    const conversation = await page
      .locator('[data-testid="composer-shell"]')
      .getAttribute("data-conversation-id");
    await emitCallInvite(page, conversation!);
    await expect(page.locator('[data-testid="call-bar"]')).toHaveAttribute("data-phase", "ringing");

    await openProtocol(page);
    const note = page.getByTestId("calling-protocol-note");
    await expect(note).not.toContainText("keeps the way it started with");
    await page.getByTestId("calling-protocol-mute-endpoint-state").click();
    await expect(page.getByTestId("calling-protocol-mute")).toHaveAttribute("data-value", "endpoint-state");
    await expect(note).toContainText("The call going now keeps the way it started with until it ends.");
  });

  /** A backend that never calls has no calls to speak any protocol: the row is not drawn, rather
   *  than drawn as a choice that changes nothing. */
  test("draws no row in a window whose backend does not take calls", async ({ page }) => {
    await gotoApp(page);
    await disableCalling(page);
    await page.locator('[data-testid="open-settings"]').click();
    await expect(page.locator('[data-testid="maintenance-settings"]')).toBeVisible();
    await expect(page.locator('[data-testid="calling-protocol"]')).toHaveCount(0);
  });
});
