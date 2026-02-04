import React from "react";

interface StatPillProps {
    icon: React.ReactNode;
    label: string;
    value: React.ReactNode;
    variant?: "default" | "success" | "warning" | "danger" | "muted" | "purple";
    suffix?: React.ReactNode;
    sublabel?: string;
}

export default function StatPill({
    icon,
    label,
    value,
    variant = "default",
    suffix,
    sublabel
}: StatPillProps) {
    return (
        <div className="stat-pill">
            <div className={`stat-pill__icon stat-pill__icon--${variant}`}>
                {icon}
            </div>
            <div className="stat-pill__content">
                <span className="stat-pill__label">{label}</span>
                <span className={`stat-pill__value ${variant !== "default" ? `stat-pill__value--${variant}` : ""}`}>
                    {value}
                    {suffix && <span className="stat-pill__suffix">{suffix}</span>}
                    {sublabel && <span className="stat-pill__sublabel">{sublabel}</span>}
                </span>
            </div>
        </div>
    );
}
