import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";
import { collectFailures } from "./guardrails";

// Garde-fous à viewport explicite, déterministes et rapides. Le pendant sur
// appareil émulé — WebKit compris — vit dans device-guardrails.spec.ts.
const VIEWPORTS = [
    { key: "mobile", width: 390, height: 844, touch: true },
    { key: "tablette", width: 768, height: 1024, touch: true },
    { key: "laptop", width: 1366, height: 768, touch: false },
] as const;

for (const viewport of VIEWPORTS) {
    test.describe(`garde-fous ${viewport.key} (${viewport.width}×${viewport.height})`, () => {
        test.use({ viewport: { width: viewport.width, height: viewport.height } });

        test("aucun débordement, cible trop petite, champ zoomable ni violation WCAG", async ({ page }) => {
            test.slow();
            const api = new ApiMock();
            await api.install(page);
            await page.emulateMedia({ reducedMotion: "reduce" });

            const failures = await collectFailures(page, { touch: viewport.touch });
            expect(failures, `${failures.length} régression(s) :\n${failures.join("\n")}`).toEqual([]);
        });
    });
}
