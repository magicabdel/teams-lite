/**
 * THE CALL STAND-IN'S WIRE AS THE MOCK SPELLS IT (`web/mock/server.ts`).
 *
 * The mock tells the page's simulated media to refuse the microphone the only way it can — a
 * line in the offer it hands back (`SIMULATED_MICROPHONE_REFUSAL`). It cannot import this
 * module: `mock/server.ts` calls `Bun.serve` at module scope, so it cannot be imported into a
 * vitest run either. So the spelling is re-stated there and pinned here by reading the file —
 * the shape `mock-pet-wire.test.ts` already has. A drifted spelling would not fail loudly: the
 * stand-in would quietly open its microphone and the spec that proves a failed Answer never
 * declines would be answering instead.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { MicrophoneUnavailableError, SIMULATED_MICROPHONE_REFUSAL, simulatedCallMedia } from "./call-media";

const MOCK = readFileSync(join(__dirname, "../../mock/server.ts"), "utf8");

describe("the mock's microphone refusal", () => {
  it("is spelled in the mock exactly as the stand-in reads it", () => {
    expect(MOCK).toContain(
      `const MOCK_MICROPHONE_REFUSAL_LINE = ${JSON.stringify(SIMULATED_MICROPHONE_REFUSAL)};`,
    );
  });

  // Only the refusing half is checked here: the stand-in that opens builds a silent audio
  // stream, which needs a browser's AudioContext. The e2e answers through it on every call.
  it("makes the stand-in refuse the microphone", () => {
    const offer = `v=0\r\ns=mock\r\n${SIMULATED_MICROPHONE_REFUSAL}\r\n`;
    expect(() => simulatedCallMedia({ answering: true, remoteOffer: offer })).toThrow(
      MicrophoneUnavailableError,
    );
  });
});
