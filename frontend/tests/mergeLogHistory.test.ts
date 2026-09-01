import { describe, expect, test } from "bun:test";
import { mergeLogHistory } from "../src/hooks/useServerEvents";

describe("mergeLogHistory", () => {
    test("garde l'historique seul quand aucune ligne n'est arrivée en direct", () => {
        expect(mergeLogHistory(["a", "b", "c"], [], 10)).toEqual(["a", "b", "c"]);
    });

    test("concatène quand rien ne se recouvre", () => {
        expect(mergeLogHistory(["a", "b"], ["c", "d"], 10)).toEqual(["a", "b", "c", "d"]);
    });

    test("ne recopie pas les lignes déjà présentes à la fin de l'historique", () => {
        expect(mergeLogHistory(["a", "b", "c"], ["b", "c", "d"], 10)).toEqual(["a", "b", "c", "d"]);
    });

    test("absorbe un direct entièrement contenu dans l'historique", () => {
        expect(mergeLogHistory(["a", "b", "c"], ["c"], 10)).toEqual(["a", "b", "c"]);
    });

    test("retient le recouvrement le plus long et non le premier trouvé", () => {
        // « x » apparaît deux fois : seul le suffixe complet doit compter.
        expect(mergeLogHistory(["x", "y", "x"], ["x", "y", "x", "z"], 10))
            .toEqual(["x", "y", "x", "z"]);
    });

    test("respecte la limite en conservant les lignes les plus récentes", () => {
        expect(mergeLogHistory(["a", "b", "c"], ["d", "e"], 3)).toEqual(["c", "d", "e"]);
    });

    test("gère des lignes répétées sans faux recouvrement", () => {
        expect(mergeLogHistory(["log", "log", "log"], ["log", "log", "fin"], 10))
            .toEqual(["log", "log", "log", "fin"]);
    });

    test("reste linéaire sur de gros volumes", () => {
        const history = Array.from({ length: 10_000 }, (_, index) => `ligne ${index}`);
        const live = Array.from({ length: 10_000 }, (_, index) => `ligne ${index + 9_000}`);
        const started = performance.now();
        const merged = mergeLogHistory(history, live, 10_000);
        // La version quadratique dépassait la seconde sur ce volume.
        expect(performance.now() - started).toBeLessThan(250);
        expect(merged.at(-1)).toBe("ligne 18999");
        expect(merged).toHaveLength(10_000);
    });
});
