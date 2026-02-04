import React from "react";
import { LucideIcon } from "lucide-react";

interface Tab {
    id: string;
    label: string;
    icon?: LucideIcon | React.ReactNode;
}

interface TabsProps {
    tabs: Tab[];
    activeTab: string;
    onTabChange: (id: any) => void;
    className?: string;
}

const Tabs: React.FC<TabsProps> = ({ tabs, activeTab, onTabChange, className = "" }) => {
    return (
        <div className={`server-tabs ${className}`}>
            {tabs.map((tab) => (
                <button
                    key={tab.id}
                    onClick={() => onTabChange(tab.id)}
                    className={`tab-btn ${activeTab === tab.id ? "tab-btn--active" : ""}`}
                >
                    {/* Render icon if it's a ReactNode or a Component */}
                    {React.isValidElement(tab.icon) ? (
                        tab.icon
                    ) : (
                        // @ts-ignore - Check if it's a component
                        tab.icon && <tab.icon size={18} />
                    )}
                    {tab.label}
                </button>
            ))}
        </div>
    );
};

export default Tabs;
