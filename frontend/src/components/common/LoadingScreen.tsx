import { useLanguage } from '../../contexts/LanguageContext';

interface LoadingScreenProps {
    message?: string;
}

export default function LoadingScreen({ message }: LoadingScreenProps) {
    const { t } = useLanguage();

    return (
        <div className="loading-screen">
            <div className="spinner"></div>
            <p className="text-muted">{message || t('common.loading')}</p>
        </div>
    );
}
