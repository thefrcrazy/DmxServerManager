import React from "react";

interface CardProps {
    title?: string;
    icon?: React.ReactNode;
    children: React.ReactNode;
    className?: string;
    onClick?: () => void;
    footer?: React.ReactNode;
    headerAction?: React.ReactNode;
}

const Card: React.FC<CardProps> = ({
    title,
    icon,
    children,
    className = "",
    onClick,
    footer,
    headerAction
}) => {
    return (
        <div 
            className={`card ${onClick ? "card--clickable" : ""} ${className}`}
            onClick={onClick}
        >
            {(title || icon) && (
                <div className="card__header">
                    <div className="flex items-center gap-2">
                        {icon && <div className="card__icon-sm">{icon}</div>}
                        {title && <h3 className="card__title">{title}</h3>}
                    </div>
                    {headerAction}
                </div>
            )}
            <div className="card__body">
                {children}
            </div>
            {footer && (
                <div className="card__footer">
                    {footer}
                </div>
            )}
        </div>
    );
};

export default Card;
