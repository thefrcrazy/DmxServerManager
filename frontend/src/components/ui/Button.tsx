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
    disabled?: boolean;
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
    const inactive = Boolean(disabled) || isLoading;
    const classes = [
        "btn",
        `btn--${variant}`,
        size !== "md" ? `btn--${size}` : "",
        fullWidth ? "btn--full" : "",
        inactive ? "btn--disabled" : "",
        className
    ].filter(Boolean).join(" ");

    const content = (
        <>
            {isLoading && <div className="spinner spinner--sm" aria-hidden="true" />}
            {!isLoading && icon}
            {children}
        </>
    );

    if (as === "link" && "to" in props) {
        const { to, ...anchorProps } = props as ButtonAsLinkProps;
        // Un lien n'a pas d'état désactivé natif : sans `href` ni gestion du
        // clavier il reste annoncé comme lien, mais n'est plus activable. La
        // version précédente rendait un lien pleinement cliquable.
        if (inactive) {
            return (
                <a
                    {...anchorProps}
                    className={classes}
                    role="link"
                    aria-disabled="true"
                    tabIndex={-1}
                    onClick={(event) => event.preventDefault()}
                >
                    {content}
                </a>
            );
        }
        return (
            <Link {...anchorProps} to={to} className={classes}>
                {content}
            </Link>
        );
    }

    return (
        <button
            className={classes}
            disabled={inactive}
            aria-busy={isLoading || undefined}
            {...(props as ButtonHTMLAttributes<HTMLButtonElement>)}
        >
            {content}
        </button>
    );
};

export default Button;
