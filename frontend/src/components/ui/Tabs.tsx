import React, { KeyboardEvent, ReactNode, useEffect, useRef } from "react";

interface Tab<T extends string> {
    id: T;
    label: string;
    icon?: ReactNode;
    /** Pastille de comptage affichée après l'intitulé. */
    badge?: ReactNode;
}

interface TabsProps<T extends string> {
    tabs: Tab<T>[];
    activeTab: T;
    onTabChange: (id: T) => void;
    className?: string;
    idPrefix?: string;
    panelId?: string;
    /** Nom accessible de la liste d'onglets. */
    label?: string;
    /** `underline` souligne l'onglet actif ; `pill` le remplit. */
    variant?: "underline" | "pill";
}

/**
 * Implémentation unique du motif ARIA « tabs » : un seul point de tabulation,
 * navigation aux flèches, `aria-controls` vers le panneau. Administration et
 * Activité en dupliquaient ou en omettaient la logique.
 */
export default function Tabs<T extends string>({
    tabs,
    activeTab,
    onTabChange,
    className = "",
    idPrefix = "tabs",
    panelId,
    label,
    variant = "underline",
}: TabsProps<T>) {
    const buttons = useRef<Array<HTMLButtonElement | null>>([]);
    const activeIndex = tabs.findIndex((tab) => tab.id === activeTab);

    // Une barre d'onglets peut défiler horizontalement : sans cela, la flèche
    // pouvait déplacer le focus sur un onglet resté hors du cadre.
    useEffect(() => {
        buttons.current[activeIndex]?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }, [activeIndex]);

    const moveFocus = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
        const target = event.key === "ArrowRight" ? (index + 1) % tabs.length
            : event.key === "ArrowLeft" ? (index - 1 + tabs.length) % tabs.length
                : event.key === "Home" ? 0
                    : event.key === "End" ? tabs.length - 1
                        : null;
        if (target === null) return;
        event.preventDefault();
        const next = tabs[target];
        if (!next) return;
        onTabChange(next.id);
        buttons.current[target]?.focus();
    };

    return (
        <div
            className={`server-tabs server-tabs--${variant} ${className}`.trim()}
            role="tablist"
            aria-label={label}
            aria-orientation="horizontal"
        >
            {tabs.map((tab, index) => (
                <button
                    ref={(element) => { buttons.current[index] = element; }}
                    key={tab.id}
                    id={`${idPrefix}-tab-${tab.id}`}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === tab.id}
                    aria-controls={panelId ?? `${idPrefix}-panel-${tab.id}`}
                    tabIndex={activeTab === tab.id ? 0 : -1}
                    onClick={() => onTabChange(tab.id)}
                    onKeyDown={(event) => moveFocus(event, index)}
                    className={`tab-btn ${activeTab === tab.id ? "tab-btn--active" : ""}`.trim()}
                >
                    {React.isValidElement(tab.icon) ? tab.icon : null}
                    {tab.label}
                    {tab.badge}
                </button>
            ))}
        </div>
    );
}
