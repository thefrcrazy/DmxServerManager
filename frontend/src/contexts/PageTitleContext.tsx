import { createContext, useContext, useState, ReactNode, useCallback } from "react";

interface BackLink {
    to: string;
    state?: Record<string, unknown>;
}

interface PageTitleContextType {
    title: string;
    subtitle: string;
    backLink: BackLink | null;
    setPageTitle: (title: string, subtitle?: string, backLink?: BackLink | null) => void;
}

const PageTitleContext = createContext<PageTitleContextType | null>(null);

export function PageTitleProvider({ children }: { children: ReactNode }) {
    const [title, setTitle] = useState("");
    const [subtitle, setSubtitle] = useState("");
    const [backLink, setBackLink] = useState<BackLink | null>(null);

    const setPageTitle = useCallback((
        newTitle: string,
        newSubtitle: string = "",
        newBackLink: BackLink | null = null,
    ) => {
        setTitle(newTitle);
        setSubtitle(newSubtitle);
        setBackLink(newBackLink);
    }, []);

    return (
        <PageTitleContext.Provider value={{ title, subtitle, backLink, setPageTitle }}>
            {children}
        </PageTitleContext.Provider>
    );
}

export function usePageTitle() {
    const context = useContext(PageTitleContext);
    if (!context) {
        throw new Error("usePageTitle must be used within a PageTitleProvider");
    }
    return context;
}
