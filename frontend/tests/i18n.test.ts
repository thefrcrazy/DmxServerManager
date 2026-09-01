import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import { fr } from "../src/i18n/fr";
import { en } from "../src/i18n/en";

type Tree = { [key: string]: string | Tree };

function flatten(tree: Tree, prefix = ""): string[] {
    return Object.entries(tree).flatMap(([key, value]) => {
        const path = prefix ? `${prefix}.${key}` : key;
        return typeof value === "string" ? [path] : flatten(value, path);
    });
}

function sourceFiles(directory: string): string[] {
    return readdirSync(directory).flatMap((entry) => {
        const path = join(directory, entry);
        if (statSync(path).isDirectory()) {
            return entry === "i18n" || entry === "generated" ? [] : sourceFiles(path);
        }
        return /\.tsx?$/.test(entry) ? [path] : [];
    });
}

const frenchKeys = new Set(flatten(fr as unknown as Tree));
const englishKeys = new Set(flatten(en as unknown as Tree));

describe("catalogues de traduction", () => {
    test("les deux langues déclarent exactement les mêmes clés", () => {
        const missingInEnglish = [...frenchKeys].filter((key) => !englishKeys.has(key));
        const missingInFrench = [...englishKeys].filter((key) => !frenchKeys.has(key));
        expect(missingInEnglish, `absentes de en.ts : ${missingInEnglish.join(", ")}`).toEqual([]);
        expect(missingInFrench, `absentes de fr.ts : ${missingInFrench.join(", ")}`).toEqual([]);
    });

    test("aucune clé littérale employée dans le code n’est absente du catalogue", () => {
        // `t()` retombe sur le chemin brut quand la clé manque : l'interface
        // affiche alors « user_settings.color » au lieu d'un libellé, sans que
        // rien n'échoue. Seules les clés littérales sont vérifiables ici ; celles
        // construites dynamiquement sont ignorées.
        const literal = /\bt\(\s*"([a-z0-9_]+(?:\.[a-z0-9_]+)+)"\s*\)/gi;
        const unknown = new Set<string>();
        for (const file of sourceFiles("src")) {
            const source = readFileSync(file, "utf8");
            for (const match of source.matchAll(literal)) {
                const key = match[1]!;
                if (!frenchKeys.has(key)) unknown.add(`${key} (${file})`);
            }
        }
        expect([...unknown], `clés introuvables : ${[...unknown].join(", ")}`).toEqual([]);
    });
});
