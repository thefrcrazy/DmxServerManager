import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { ApiMock, INSTANCES } from "./api.fixture";

// Garde-fous mesurés sur l'application réelle. Chaque seuil correspond à une
// anomalie relevée par l'audit du 31 août 2026 : sans ce filet, elles reviennent
// au rythme des fonctionnalités suivantes.
//
// - 16 px sur les champs : en dessous, Safari iOS zoome et ne revient jamais.
// - 44 px sur les cibles : plancher tactile WCAG 2.5.5 / Apple HIG.
// - aucun débordement : ni le document, ni un élément isolé.
// - axe WCAG A/AA sur toutes les routes, pas seulement connexion et tableau de bord.

const MINIMUM_FONT_SIZE = 16;
const MINIMUM_TOUCH_TARGET = 44;

const VIEWPORTS = [
    { key: "mobile", width: 390, height: 844, touch: true },
    { key: "tablette", width: 768, height: 1024, touch: true },
    { key: "laptop", width: 1366, height: 768, touch: false },
] as const;

const ROUTES = [
    { key: "dashboard", url: "/dashboard" },
    { key: "servers", url: "/servers" },
    { key: "activity-operations", url: "/activity?tab=operations" },
    { key: "activity-journal", url: "/activity?tab=journal" },
    { key: "user-settings", url: "/user-settings" },
    { key: "administration", url: "/administration" },
    { key: "create-server", url: "/servers/create" },
    { key: "server-configuration", url: `/servers/${INSTANCES[0]!.id}` },
    { key: "server-console", url: `/servers/${INSTANCES[0]!.id}?tab=console` },
    { key: "server-players", url: `/servers/${INSTANCES[0]!.id}?tab=players` },
    { key: "server-schedules", url: `/servers/${INSTANCES[0]!.id}?tab=schedules` },
] as const;

interface Measurements {
    documentWidth: number;
    viewportWidth: number;
    outsideViewport: string[];
    smallTargets: string[];
    smallFonts: string[];
}

async function measure(page: Page): Promise<Measurements> {
    return page.evaluate(({ minimumFontSize, minimumTouchTarget }) => {
        const describe = (element: Element): string => {
            const classes = (element.getAttribute("class") ?? "")
                .trim().split(/\s+/).filter(Boolean).slice(0, 2);
            const id = element.id ? `#${element.id}` : "";
            return `${element.tagName.toLowerCase()}${id}${classes.length ? `.${classes.join(".")}` : ""}`;
        };

        const visible = Array.from(document.querySelectorAll<HTMLElement>("body *")).filter((element) => {
            const box = element.getBoundingClientRect();
            if (box.width === 0 || box.height === 0) return false;
            const style = getComputedStyle(element);
            return style.visibility !== "hidden" && style.display !== "none" && style.opacity !== "0";
        });

        const interactiveSelector = [
            "a[href]", "button", "select", "summary",
            'input:not([type="hidden"])',
            '[role="button"]', '[role="tab"]', '[role="radio"]', '[role="switch"]', '[role="checkbox"]',
            '[tabindex]:not([tabindex="-1"])',
        ].join(", ");

        const textFieldSelector = [
            'input:not([type="hidden"]):not([type="checkbox"]):not([type="radio"])'
            + ':not([type="range"]):not([type="color"]):not([type="file"])',
            "select",
            "textarea",
        ].join(", ");

        return {
            documentWidth: document.documentElement.scrollWidth,
            viewportWidth: window.innerWidth,
            outsideViewport: visible
                .filter((element) => element.getBoundingClientRect().right > window.innerWidth + 1)
                .map((element) => `${describe(element)} (bord droit ${Math.round(element.getBoundingClientRect().right)}px)`)
                .slice(0, 15),
            smallTargets: visible
                .filter((element) => element.matches(interactiveSelector))
                // Un contrôle dont un ancêtre proche porte déjà une zone tactile
                // suffisante reste atteignable ; on ne mesure que l'élément lui-même.
                .map((element) => ({ element, box: element.getBoundingClientRect() }))
                .filter(({ box }) => box.height < minimumTouchTarget || box.width < minimumTouchTarget)
                .map(({ element, box }) => `${describe(element)} ${Math.round(box.width)}×${Math.round(box.height)}px`
                    + ` "${(element.textContent ?? "").trim().slice(0, 24) || element.getAttribute("aria-label") || ""}"`)
                .slice(0, 25),
            smallFonts: visible
                .filter((element) => element.matches(textFieldSelector))
                .map((element) => ({ element, size: Number.parseFloat(getComputedStyle(element).fontSize) }))
                .filter(({ size }) => size < minimumFontSize)
                .map(({ element, size }) => `${describe(element)} ${size}px`)
                .slice(0, 25),
        };
    }, { minimumFontSize: MINIMUM_FONT_SIZE, minimumTouchTarget: MINIMUM_TOUCH_TARGET });
}

for (const viewport of VIEWPORTS) {
    test.describe(`garde-fous ${viewport.key} (${viewport.width}×${viewport.height})`, () => {
        test.use({ viewport: { width: viewport.width, height: viewport.height } });

        test("aucun débordement, cible trop petite, champ zoomable ni violation WCAG", async ({ page }) => {
            test.slow();
            const api = new ApiMock();
            await api.install(page);
            await page.emulateMedia({ reducedMotion: "reduce" });

            const failures: string[] = [];

            for (const route of ROUTES) {
                await page.goto(route.url);
                await page.waitForLoadState("networkidle").catch(() => undefined);
                const result = await measure(page);

                if (result.documentWidth > result.viewportWidth) {
                    failures.push(`[${route.key}] débordement horizontal du document :`
                        + ` ${result.documentWidth}px pour ${result.viewportWidth}px de viewport`);
                }
                for (const element of result.outsideViewport) {
                    failures.push(`[${route.key}] élément hors viewport : ${element}`);
                }
                if (viewport.touch) {
                    for (const element of result.smallFonts) {
                        failures.push(`[${route.key}] champ sous ${MINIMUM_FONT_SIZE}px (zoom iOS) : ${element}`);
                    }
                    for (const element of result.smallTargets) {
                        failures.push(`[${route.key}] cible tactile sous ${MINIMUM_TOUCH_TARGET}px : ${element}`);
                    }
                }

                const audit = await new AxeBuilder({ page })
                    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
                    .analyze();
                for (const violation of audit.violations) {
                    failures.push(`[${route.key}] axe ${violation.id} (${violation.impact}) :`
                        + ` ${violation.nodes.length} nœud(s) — ${violation.nodes.slice(0, 3).map((node) => node.target.join(" ")).join(", ")}`);
                }
            }

            expect(failures, `${failures.length} régression(s) :\n${failures.join("\n")}`).toEqual([]);
        });
    });
}
