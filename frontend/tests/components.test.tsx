import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import Button from "../src/components/ui/Button";
import Input from "../src/components/ui/Input";
import Tabs from "../src/components/ui/Tabs";
import Toast from "../src/components/shared/Toast";
import ToastContainer from "../src/components/shared/ToastContainer";
import { LanguageProvider } from "../src/contexts/LanguageContext";
import { ToastProvider, useToast } from "../src/contexts/ToastContext";

afterEach(cleanup);

function withLanguage(children: React.ReactNode) {
    return <LanguageProvider>{children}</LanguageProvider>;
}

describe("Button", () => {
    test("un lien désactivé ne navigue pas", async () => {
        // `as="link"` rendait un <Link> pleinement cliquable : l'état désactivé
        // était accepté par le type et silencieusement ignoré au rendu.
        render(
            <MemoryRouter>
                <Button as="link" to="/servers" disabled>Ouvrir</Button>
            </MemoryRouter>,
        );
        const link = screen.getByRole("link", { name: "Ouvrir" });
        expect(link.getAttribute("aria-disabled")).toBe("true");
        expect(link.getAttribute("tabindex")).toBe("-1");
        expect(link.getAttribute("href")).toBeNull();
    });

    test("le chargement neutralise le bouton et l’annonce", () => {
        render(<Button isLoading>Enregistrer</Button>);
        const button = screen.getByRole("button", { name: /Enregistrer/ });
        expect((button as HTMLButtonElement).disabled).toBe(true);
        expect(button.getAttribute("aria-busy")).toBe("true");
    });
});

describe("Input", () => {
    test("une erreur est reliée au champ et le marque invalide", () => {
        // Le message était rendu sans lien programmatique : un lecteur d'écran
        // n'annonçait ni l'état invalide ni la raison du refus.
        render(<Input aria-label="Port" error="Port déjà utilisé" />);
        const field = screen.getByLabelText("Port");
        expect(field.getAttribute("aria-invalid")).toBe("true");
        const describedBy = field.getAttribute("aria-describedby");
        expect(describedBy).toBeTruthy();
        expect(document.getElementById(describedBy!)?.textContent).toBe("Port déjà utilisé");
    });

    test("sans erreur, aucun attribut d’invalidité n’est posé", () => {
        render(<Input aria-label="Port" />);
        expect(screen.getByLabelText("Port").getAttribute("aria-invalid")).toBeNull();
    });
});

describe("Tabs", () => {
    const tabs = [
        { id: "a" as const, label: "Terminal" },
        { id: "b" as const, label: "Config" },
        { id: "c" as const, label: "Joueurs" },
    ];

    function Harness() {
        const [active, setActive] = useState<"a" | "b" | "c">("a");
        return <Tabs tabs={tabs} activeTab={active} onTabChange={setActive} label="Onglets" idPrefix="x" />;
    }

    test("un seul onglet est dans l’ordre de tabulation", () => {
        render(<Harness />);
        const list = screen.getByRole("tablist", { name: "Onglets" });
        const items = within(list).getAllByRole("tab");
        expect(items.filter((tab) => tab.getAttribute("tabindex") === "0")).toHaveLength(1);
        expect(items.filter((tab) => tab.getAttribute("tabindex") === "-1")).toHaveLength(2);
    });

    test("les flèches parcourent les onglets en boucle", async () => {
        const user = userEvent.setup();
        render(<Harness />);
        await user.click(screen.getByRole("tab", { name: "Terminal" }));
        await user.keyboard("{ArrowRight}");
        expect(screen.getByRole("tab", { name: "Config" }).getAttribute("aria-selected")).toBe("true");
        await user.keyboard("{ArrowLeft}{ArrowLeft}");
        expect(screen.getByRole("tab", { name: "Joueurs" }).getAttribute("aria-selected")).toBe("true");
        await user.keyboard("{Home}");
        expect(screen.getByRole("tab", { name: "Terminal" }).getAttribute("aria-selected")).toBe("true");
    });

    test("chaque onglet désigne son panneau", () => {
        render(<Harness />);
        expect(screen.getByRole("tab", { name: "Config" }).getAttribute("aria-controls")).toBe("x-panel-b");
    });
});

describe("Notifications", () => {
    test("les erreurs vont dans une région assertive et ne s’effacent pas seules", () => {
        render(withLanguage(
            <ToastProvider>
                <ToastContainer />
                <Toast toast={{ id: "1", message: "Échec de la sauvegarde", type: "error", duration: 0 }} />
            </ToastProvider>,
        ));
        expect(screen.getByRole("alert").getAttribute("aria-live")).toBe("assertive");
        expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
        expect(screen.getByText("Échec de la sauvegarde")).toBeDefined();
    });

    test("la fermeture porte un nom accessible", () => {
        render(withLanguage(
            <ToastProvider>
                <Toast toast={{ id: "1", message: "Fait", type: "success", duration: 0 }} />
            </ToastProvider>,
        ));
        expect(screen.getByRole("button", { name: "Fermer" })).toBeDefined();
    });

    test("une erreur émise par le contexte est persistante par défaut", async () => {
        function Emitter() {
            const toast = useToast();
            return <button type="button" onClick={() => toast.error("Boum")}>émettre</button>;
        }
        const user = userEvent.setup();
        render(withLanguage(
            <ToastProvider>
                <ToastContainer />
                <Emitter />
            </ToastProvider>,
        ));
        await user.click(screen.getByRole("button", { name: "émettre" }));
        expect(within(screen.getByRole("alert")).getByText("Boum")).toBeDefined();
    });
});
