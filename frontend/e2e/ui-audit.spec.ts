import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { ApiMock, INSTANCES } from "./api.fixture";

const enabled = process.env.DMX_UI_AUDIT === "1";
const outDir = resolve(process.cwd(), "../artifacts/ui-audit");

const VIEWPORTS = [
    { key: "laptop-1366", width: 1366, height: 768 },
    { key: "laptop-1280", width: 1280, height: 800 },
    { key: "tablet-land-1024", width: 1024, height: 768 },
    { key: "tablet-port-834", width: 834, height: 1112 },
    { key: "tablet-port-768", width: 768, height: 1024 },
    { key: "mobile-390", width: 390, height: 844 },
    { key: "mobile-360", width: 360, height: 740 },
];

const ROUTES = [
    { key: "dashboard", url: "/dashboard" },
    { key: "servers", url: "/servers" },
    { key: "activity-ops", url: "/activity?tab=operations" },
    { key: "activity-journal", url: "/activity?tab=journal" },
    { key: "account", url: "/user-settings" },
    { key: "administration", url: "/administration" },
    { key: "create-server", url: "/servers/create" },
    { key: "srv-overview", url: `/servers/${INSTANCES[0]!.id}` },
    { key: "srv-config", url: `/servers/${INSTANCES[0]!.id}?tab=config` },
    { key: "srv-console", url: `/servers/${INSTANCES[0]!.id}?tab=console` },
    { key: "srv-files", url: `/servers/${INSTANCES[0]!.id}?tab=files` },
    { key: "srv-backups", url: `/servers/${INSTANCES[0]!.id}?tab=backups` },
    { key: "srv-metrics", url: `/servers/${INSTANCES[0]!.id}?tab=metrics` },
    { key: "srv-players", url: `/servers/${INSTANCES[0]!.id}?tab=players` },
    { key: "srv-schedules", url: `/servers/${INSTANCES[0]!.id}?tab=schedules` },
    { key: "srv-mods", url: `/servers/${INSTANCES[0]!.id}?tab=mods` },
];

type Probe = {
    docScrollWidth: number;
    innerWidth: number;
    overflowers: { sel: string; right: number; width: number }[];
    smallTargets: { sel: string; w: number; h: number; text: string }[];
    smallFontInputs: { sel: string; fontSize: string; type: string }[];
    tinyText: { sel: string; fontSize: string; sample: string }[];
    hScrollers: { sel: string; scrollWidth: number; clientWidth: number }[];
    fixedWide: { sel: string; width: number; minWidth: string }[];
};

async function probe(page: Page): Promise<Probe> {
    return page.evaluate(() => {
        const describe = (el: Element): string => {
            const parts: string[] = [el.tagName.toLowerCase()];
            if (el.id) parts.push(`#${el.id}`);
            const cls = (el.getAttribute("class") ?? "").trim().split(/\s+/).filter(Boolean).slice(0, 3);
            if (cls.length) parts.push(`.${cls.join(".")}`);
            return parts.join("");
        };
        const vw = window.innerWidth;
        const all = Array.from(document.querySelectorAll<HTMLElement>("body *"));
        const visible = all.filter((el) => {
            const r = el.getBoundingClientRect();
            if (r.width === 0 || r.height === 0) return false;
            const cs = getComputedStyle(el);
            return cs.visibility !== "hidden" && cs.display !== "none" && cs.opacity !== "0";
        });

        const overflowers = visible
            .filter((el) => el.getBoundingClientRect().right > vw + 1)
            .map((el) => ({ sel: describe(el), right: Math.round(el.getBoundingClientRect().right), width: Math.round(el.getBoundingClientRect().width) }))
            .slice(0, 25);

        const interactiveSel = 'a[href], button, input:not([type="hidden"]), select, textarea, [role="button"], [role="tab"], [role="switch"], [role="checkbox"], summary, [tabindex]:not([tabindex="-1"])';
        const smallTargets = visible
            .filter((el) => el.matches(interactiveSel))
            .map((el) => { const r = el.getBoundingClientRect(); return { el, r }; })
            .filter(({ r }) => r.height < 44 || r.width < 44)
            .map(({ el, r }) => ({ sel: describe(el), w: Math.round(r.width), h: Math.round(r.height), text: (el.textContent ?? "").trim().slice(0, 30) || (el.getAttribute("aria-label") ?? "") }))
            .slice(0, 40);

        const smallFontInputs = visible
            .filter((el) => el.matches('input:not([type="hidden"]):not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="color"]), select, textarea'))
            .map((el) => ({ el, fs: getComputedStyle(el).fontSize }))
            .filter(({ fs }) => Number.parseFloat(fs) < 16)
            .map(({ el, fs }) => ({ sel: describe(el), fontSize: fs, type: (el as HTMLInputElement).type ?? el.tagName }))
            .slice(0, 30);

        const tinyText = visible
            .filter((el) => el.children.length === 0 && (el.textContent ?? "").trim().length > 2)
            .map((el) => ({ el, fs: getComputedStyle(el).fontSize }))
            .filter(({ fs }) => Number.parseFloat(fs) < 12)
            .map(({ el, fs }) => ({ sel: describe(el), fontSize: fs, sample: (el.textContent ?? "").trim().slice(0, 32) }))
            .slice(0, 25);

        const hScrollers = visible
            .filter((el) => el.scrollWidth > el.clientWidth + 2 && el.clientWidth > 0)
            .map((el) => ({ sel: describe(el), scrollWidth: el.scrollWidth, clientWidth: el.clientWidth }))
            .slice(0, 25);

        const fixedWide = visible
            .map((el) => ({ el, cs: getComputedStyle(el) }))
            .filter(({ el, cs }) => {
                const mw = cs.minWidth;
                if (!mw.endsWith("px")) return false;
                return Number.parseFloat(mw) > vw - 32 && el.getBoundingClientRect().width > 0;
            })
            .map(({ el, cs }) => ({ sel: describe(el), width: Math.round(el.getBoundingClientRect().width), minWidth: cs.minWidth }))
            .slice(0, 20);

        return {
            docScrollWidth: document.documentElement.scrollWidth,
            innerWidth: vw,
            overflowers,
            smallTargets,
            smallFontInputs,
            tinyText,
            hScrollers,
            fixedWide,
        };
    });
}

test.describe("audit UI responsive", () => {
    test.skip(!enabled, "audit manuel");
    test.describe.configure({ mode: "serial" });
    test.setTimeout(600_000);

    test("mesure toutes les vues sur laptop / tablette / mobile", async ({ page }) => {
        const api = new ApiMock();
        await api.install(page);
        await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "dark" });
        await mkdir(outDir, { recursive: true });

        const report: Record<string, Record<string, Probe & { axe?: unknown[] }>> = {};

        for (const vp of VIEWPORTS) {
            await page.setViewportSize({ width: vp.width, height: vp.height });
            for (const route of ROUTES) {
                await page.goto(route.url);
                await page.waitForLoadState("networkidle").catch(() => undefined);
                await page.waitForTimeout(500);
                const result = await probe(page);
                report[vp.key] ??= {};
                let axe: unknown[];
                try {
                    const r = await new AxeBuilder({ page })
                        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
                        .analyze();
                    axe = r.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.length, help: v.help, targets: v.nodes.slice(0, 3).map((n) => n.target.join(" ")) }));
                } catch (error) {
                    axe = [{ id: "axe-failed", impact: String(error) }];
                }
                report[vp.key]![route.key] = { ...result, axe };
                if (vp.key === "mobile-390" || vp.key === "tablet-port-768" || vp.key === "laptop-1366") {
                    await page.screenshot({
                        path: resolve(outDir, `${vp.key}--${route.key}.png`),
                        animations: "disabled",
                        caret: "hide",
                        scale: "css",
                        fullPage: false,
                    });
                }
            }
        }

        await writeFile(resolve(outDir, "probe.json"), JSON.stringify(report, null, 2), "utf8");
        expect(Object.keys(report).length).toBeGreaterThan(0);
    });
});
