import { Search, LayoutGrid, List } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Input, Select } from "@/components/ui";
import { gameProfileVisual } from "@/constants/gameProfiles";

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
        <div className="server-filters">
            <div className="server-filters__search">
                <Input
                    placeholder={t("common.search")}
                    aria-label={t("common.search")}
                    value={search}
                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => onSearchChange(e.target.value)}
                    icon={<Search size={18} />}
                />
            </div>

            <div className="server-filters__profile">
                <Select
                    options={[
                        { value: "all", label: t("common.all_games") },
                        ...gameTypes.map(type => ({ value: type, label: gameProfileVisual(type).label }))
                    ]}
                    value={gameType}
                    onChange={(value: string) => onGameTypeChange(value)}
                    aria-label={t("common.all_games")}
                    placeholder={t("common.all_games")}
                />
            </div>

            <div className="view-toggle">
                <button
                    onClick={() => onViewModeChange("list")}
                    className={`btn btn--icon btn--ghost ${viewMode === "list" ? "active" : ""}`}
                    title={t("common.list_view")}
                    aria-label={t("common.list_view")}
                    aria-pressed={viewMode === "list"}
                >
                    <List size={20} />
                </button>
                <button
                    onClick={() => onViewModeChange("grid")}
                    className={`btn btn--icon btn--ghost ${viewMode === "grid" ? "active" : ""}`}
                    title={t("common.grid_view")}
                    aria-label={t("common.grid_view")}
                    aria-pressed={viewMode === "grid"}
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
