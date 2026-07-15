import { describe, expect, test } from "bun:test";
import { parseAnsi } from "@/components/shared/SafeAnsi";

describe("rendu ANSI sûr", () => {
    test("rend les couleurs SGR sans interpréter le contenu comme du HTML", () => {
        const segments = parseAnsi("normal \u001b[31m<error>\u001b[0m fin");
        expect(segments.map((segment) => segment.text).join("")).toBe("normal <error> fin");
        expect(segments.find((segment) => segment.text === "<error>")?.style.color).toBe("#f85149");
    });

    test("supprime les séquences de contrôle non prises en charge", () => {
        const segments = parseAnsi("titre\u001b[2J\u001b]8;;https://example.com\u0007lien\u001b]8;;\u0007");
        expect(segments.map((segment) => segment.text).join("")).toBe("titrelien");
    });

    test("borne les couleurs RGB invalides", () => {
        const [segment] = parseAnsi("\u001b[38;2;999;0;0mtexte");
        expect(segment?.style.color).toBeUndefined();
    });
});
