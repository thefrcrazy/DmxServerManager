import { Search, LayoutGrid, List } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Input, Select } from "@/components/ui";

interface ServerFiltersProps {
    search: string;
    onSearchChange: (value: string) => void;
    gameType: string;
    onGameTypeChange: (value: string) => void;
    viewMode: "grid" | "list";
    onViewModeChange: (mode: "grid" | "list") => void;
    gameTypes: string[];
    action?: React.ReactNode;
}

export default function ServerFilters({
    search,
    onSearchChange,
    gameType,
    onGameTypeChange,
    viewMode,
    onViewModeChange,
    gameTypes,
    action
}: ServerFiltersProps) {
    const { t } = useLanguage();

    return (
        <div className="server-filters" style={{ display: "flex", gap: "1rem", marginBottom: "1.5rem", flexWrap: "wrap", alignItems: "center" }}>
            <div style={{ flex: 1, minWidth: "200px" }}>
                <Input
                    placeholder={t("common.search") || "Search..."}
                    value={search}
                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => onSearchChange(e.target.value)}
                    icon={<Search size={18} />}
                />
            </div>

            <div style={{ width: "200px" }}>
                <Select
                    options={[
                        { value: "all", label: t("common.all_games") || "All Games" },
                        ...gameTypes.map(type => ({ value: type, label: type.charAt(0).toUpperCase() + type.slice(1) }))
                    ]}
                    value={gameType}
                    onChange={(value: string) => onGameTypeChange(value)}
                />
            </div>

            <div className="view-toggle" style={{ display: "flex", background: "var(--bg-card)", padding: "0.25rem", borderRadius: "8px", border: "1px solid var(--border-color)" }}>
                <button
                    onClick={() => onViewModeChange("list")}
                    className={`btn btn--icon btn--ghost ${viewMode === "list" ? "active" : ""}`}
                    style={{ background: viewMode === "list" ? "var(--bg-hover)" : "transparent", color: viewMode === "list" ? "var(--primary)" : "var(--text-muted)" }}
                    title="List View"
                >
                    <List size={20} />
                </button>
                <button
                    onClick={() => onViewModeChange("grid")}
                    className={`btn btn--icon btn--ghost ${viewMode === "grid" ? "active" : ""}`}
                    style={{ background: viewMode === "grid" ? "var(--bg-hover)" : "transparent", color: viewMode === "grid" ? "var(--primary)" : "var(--text-muted)" }}
                    title="Grid View"
                >
                    <LayoutGrid size={20} />
                </button>
            </div>

            {action && (
                <div className="filter-actions">
                    {action}
                </div>
            )}
        </div>
    );
}
