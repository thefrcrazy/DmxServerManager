import React, { ButtonHTMLAttributes } from "react";
import { Link } from "react-router-dom";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "danger-solid" | "success";
type ButtonSize = "sm" | "md" | "lg" | "icon";

interface BaseButtonProps {
    variant?: ButtonVariant;
    size?: ButtonSize;
    isLoading?: boolean;
    icon?: React.ReactNode;
    fullWidth?: boolean;
    className?: string;
    children?: React.ReactNode;
}

// Button as button element
interface ButtonAsButtonProps extends BaseButtonProps, ButtonHTMLAttributes<HTMLButtonElement> {
    as?: "button";
    to?: never;
}

// Button as React Router Link
interface ButtonAsLinkProps extends BaseButtonProps, React.AnchorHTMLAttributes<HTMLAnchorElement> {
    as: "link";
    to: string;
}

type ButtonProps = ButtonAsButtonProps | ButtonAsLinkProps;

const Button: React.FC<ButtonProps> = ({
    variant = "primary",
    size = "md",
    isLoading = false,
    icon,
    fullWidth = false,
    className = "",
    children,
    disabled,
    as = "button",
    ...props
}) => {
    const classes = [
        "btn",
        `btn--${variant}`,
        size !== "md" ? `btn--${size}` : "",
        fullWidth ? "btn--full" : "",
        className
    ].filter(Boolean).join(" ");

    const content = (
        <>
            {isLoading && <div className="spinner spinner--sm" />}
            {!isLoading && icon}
            {children}
        </>
    );

    if (as === "link" && "to" in props) {
        return (
            <Link to={props.to} className={classes} {...(props as any)}>
                {content}
            </Link>
        );
    }

    return (
        <button className={classes} disabled={disabled || isLoading} {...(props as ButtonHTMLAttributes<HTMLButtonElement>)}>
            {content}
        </button>
    );
};

export default Button;
