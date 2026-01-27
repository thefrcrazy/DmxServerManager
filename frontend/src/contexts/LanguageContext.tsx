import { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { fr } from '../i18n/fr';
import { en } from '../i18n/en';
import { TranslationType } from '../i18n/fr';

type Language = 'fr' | 'en';

type LanguageContextType = {
    language: Language;
    setLanguage: (lang: Language) => void;
    t: (key: string) => string;
    translations: TranslationType;
};

const LanguageContext = createContext<LanguageContextType | undefined>(undefined);

// Helper to get nested property from object by string key (e.g. "common.save")
const getNestedValue = (obj: any, path: string): string => {
    return path.split('.').reduce((prev, curr) => {
        return prev ? prev[curr] : null;
    }, obj) || path;
};

export function LanguageProvider({ children }: { children: ReactNode }) {
    const [language, setLanguageState] = useState<Language>(() => {
        const saved = localStorage.getItem('draveur_language');
        return (saved === 'fr' || saved === 'en') ? saved : 'fr';
    });

    const setLanguage = (lang: Language) => {
        setLanguageState(lang);
        localStorage.setItem('draveur_language', lang);
        document.documentElement.lang = lang;
    };

    useEffect(() => {
        document.documentElement.lang = language;
    }, []);

    const translations = language === 'fr' ? fr : en;

    const t = (path: string): string => {
        return getNestedValue(translations, path);
    };

    return (
        <LanguageContext.Provider value={{ language, setLanguage, t, translations }}>
            {children}
        </LanguageContext.Provider>
    );
}

export function useLanguage() {
    const context = useContext(LanguageContext);
    if (context === undefined) {
        throw new Error('useLanguage must be used within a LanguageProvider');
    }
    return context;
}
