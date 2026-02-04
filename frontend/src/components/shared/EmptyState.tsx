import React from "react";

interface EmptyStateProps {
    icon: React.ReactNode;
    title: string;
    description: string;
    action?: React.ReactNode;
    className?: string;
}

export default function EmptyState({ icon, title, description, action, className = "" }: EmptyStateProps) {
    return (
        <div className={`empty-state ${className}`}>
            <div className="empty-state__icon">
                {icon}
            </div>
            <h3 className="empty-state__title">{title}</h3>
            <p className="empty-state__description">{description}</p>
            {action && action}
        </div>
    );
}
