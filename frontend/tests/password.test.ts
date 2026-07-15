import { describe, expect, test } from "bun:test";
import { passwordMeetsPolicy } from "../src/utils/password";

describe("password policy", () => {
    test("requires length, upper/lower case, a digit and a symbol", () => {
        expect(passwordMeetsPolicy("Permanent-Owner-2026!")).toBe(true);
        expect(passwordMeetsPolicy("Short-1!")).toBe(false);
        expect(passwordMeetsPolicy("permanent-owner-2026!")).toBe(false);
        expect(passwordMeetsPolicy("PERMANENT-OWNER-2026!")).toBe(false);
        expect(passwordMeetsPolicy("Permanent-Owner-Test!")).toBe(false);
        expect(passwordMeetsPolicy("PermanentOwner2026")).toBe(false);
    });
});
