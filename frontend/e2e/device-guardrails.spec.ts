import { expect, test } from "@playwright/test";
import { ApiMock } from "./api.fixture";
import { collectFailures } from "./guardrails";

// Mêmes invariants, mais au viewport et au moteur de l'appareil émulé. WebKit
// est le moteur où se manifestent le zoom automatique iOS sur un champ sous
// 16 px, le comportement de `dvh` face à la barre d'URL et les zones de
// sécurité : les vérifier sous Chromium seul ne prouvait rien.
//
// Ce fichier n'impose aucun viewport : il hérite de celui du projet Playwright.
test("l’appareil émulé ne présente aucune régression responsive ni WCAG", async ({ page, isMobile }) => {
    test.slow();
    const api = new ApiMock();
    await api.install(page);
    await page.emulateMedia({ reducedMotion: "reduce" });

    const failures = await collectFailures(page, { touch: isMobile ?? true });
    expect(failures, `${failures.length} régression(s) :\n${failures.join("\n")}`).toEqual([]);
});
