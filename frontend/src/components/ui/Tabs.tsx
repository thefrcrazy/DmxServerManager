import React, { KeyboardEvent, useRef } from "react";
import { LucideIcon } from "lucide-react";

interface Tab<T extends string> {
    id: T;
    label: string;
    icon?: LucideIcon | React.ReactNode;
}

interface TabsProps<T extends string> {
    tabs: Tab<T>[];
    activeTab: T;
    onTabChange: (id: T) => void;
    className?: string;
    idPrefix?: string;
    panelId?: string;
}

function Tabs<T extends string>({ tabs, activeTab, onTabChange, className = "", idPrefix = "tabs", panelId }: TabsProps<T>) {
    const buttons = useRef<Array<HTMLButtonElement | null>>([]);
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
        <div className={`server-tabs ${className}`} role="tablist">
            {tabs.map((tab, index) => (
                <button
                    ref={(element) => { buttons.current[index] = element; }}
                    key={tab.id}
                    id={`${idPrefix}-tab-${tab.id}`}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === tab.id}
                    aria-controls={panelId}
                    tabIndex={activeTab === tab.id ? 0 : -1}
                    onClick={() => onTabChange(tab.id)}
                    onKeyDown={(event) => moveFocus(event, index)}
                    className={`tab-btn ${activeTab === tab.id ? "tab-btn--active" : ""}`}
                >
                    {/* Render icon if it's a ReactNode or a Component */}
                    {React.isValidElement(tab.icon) ? (
                        tab.icon
                    ) : (
                        // @ts-expect-error - Check if it's a component
                        tab.icon && <tab.icon size={18} />
                    )}
                    {tab.label}
                </button>
            ))}
        </div>
    );
}

export default Tabs;
